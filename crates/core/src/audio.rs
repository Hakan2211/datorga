//! Encrypted, crash-safe audio storage.
//!
//! A recording is a directory of chunks (`chunk_00000.pcm.enc`, ...). Each chunk holds mono
//! 16 kHz s16le PCM, sealed with AES-256-GCM under a per-recording key (DEK) that lives only
//! in the encrypted database. Plaintext audio never touches the disk; deleting the DB row
//! makes any leftover chunk files unreadable. Each chunk is written to a temp file, fsynced
//! and renamed, so a crash loses at most the chunk in flight.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{Error, Result};

pub const SAMPLE_RATE: u32 = 16_000;
/// 5 s per chunk: the most a crash can lose.
pub const CHUNK_SAMPLES: usize = 5 * SAMPLE_RATE as usize;
pub const CODEC: &str = "pcm_s16le_16k_mono_aesgcm";

const EXT: &str = "pcm.enc";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct AudioKey([u8; 32]);

impl AudioKey {
    pub fn generate() -> Result<Self> {
        let mut k = [0u8; 32];
        getrandom::fill(&mut k).map_err(|e| Error::Crypto(e.to_string()))?;
        Ok(Self(k))
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Result<Self> {
        let v = hex::decode(s).map_err(|e| Error::Crypto(e.to_string()))?;
        Ok(Self(v.try_into().map_err(|_| Error::Crypto("audio key has wrong length".into()))?))
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new_from_slice(&self.0).expect("32-byte key")
    }
}

/// The chunk index is bound as associated data, so chunks cannot be reordered or swapped.
fn aad(idx: u32) -> Vec<u8> {
    format!("thera-audio-v1:{idx}").into_bytes()
}

fn chunk_path(dir: &Path, idx: u32) -> PathBuf {
    dir.join(format!("chunk_{idx:05}.{EXT}"))
}

pub struct ChunkWriter {
    dir: PathBuf,
    cipher: Aes256Gcm,
    next: u32,
    hasher: Sha256,
    samples: u64,
}

impl ChunkWriter {
    pub fn create(dir: &Path, key: &AudioKey) -> Result<Self> {
        fs::create_dir_all(dir)?;
        Ok(Self { dir: dir.to_path_buf(), cipher: key.cipher(), next: 0, hasher: Sha256::new(), samples: 0 })
    }

    pub fn write(&mut self, pcm: &[i16]) -> Result<()> {
        let mut plain: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|e| Error::Crypto(e.to_string()))?;
        let aad = aad(self.next);
        let ct = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce), Payload { msg: &plain, aad: &aad })
            .map_err(|e| Error::Crypto(e.to_string()))?;
        self.hasher.update(&plain);
        plain.zeroize();

        let path = chunk_path(&self.dir, self.next);
        let tmp = path.with_extension("tmp");
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&nonce)?;
        f.write_all(&ct)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, &path)?;
        self.next += 1;
        self.samples += pcm.len() as u64;
        Ok(())
    }

    pub fn samples(&self) -> u64 {
        self.samples
    }

    /// SHA-256 of all plaintext PCM written so far.
    pub fn sha256(&self) -> String {
        hex::encode(self.hasher.clone().finalize())
    }
}

fn chunk_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("chunk_") && n.ends_with(EXT)))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(e) => return Err(e.into()),
    };
    files.sort();
    Ok(files)
}

/// Sample count from file sizes alone (no key needed); used to recover after a crash.
pub fn samples_on_disk(dir: &Path) -> Result<u64> {
    chunk_files(dir)?.iter().try_fold(0u64, |acc, p| {
        let len = fs::metadata(p)?.len();
        Ok(acc + len.saturating_sub((NONCE_LEN + TAG_LEN) as u64) / 2)
    })
}

/// Decrypts all chunks in order. Fails on any tampered, missing or reordered chunk.
pub fn read_pcm(dir: &Path, key: &AudioKey) -> Result<Vec<i16>> {
    let cipher = key.cipher();
    let mut out = Vec::new();
    for (idx, path) in chunk_files(dir)?.iter().enumerate() {
        if *path != chunk_path(dir, idx as u32) {
            return Err(Error::Crypto(format!("audio chunk {idx} is missing")));
        }
        let raw = fs::read(path)?;
        if raw.len() < NONCE_LEN + TAG_LEN {
            return Err(Error::Crypto(format!("audio chunk {idx} is truncated")));
        }
        let (nonce, ct) = raw.split_at(NONCE_LEN);
        let aad = aad(idx as u32);
        let mut plain = cipher
            .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad: &aad })
            .map_err(|_| Error::Crypto(format!("audio chunk {idx} failed authentication")))?;
        out.extend(plain.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])));
        plain.zeroize();
    }
    Ok(out)
}

/// Plain 16-bit mono WAV, for handing to the transcription sidecar. The caller deletes it.
pub fn write_wav(path: &Path, pcm: &[i16]) -> Result<()> {
    let data_len = (pcm.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend(b"RIFF");
    b.extend((36 + data_len).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes()); // fmt chunk size
    b.extend(1u16.to_le_bytes()); // PCM
    b.extend(1u16.to_le_bytes()); // mono
    b.extend(SAMPLE_RATE.to_le_bytes());
    b.extend((SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    b.extend(2u16.to_le_bytes()); // block align
    b.extend(16u16.to_le_bytes()); // bits per sample
    b.extend(b"data");
    b.extend(data_len.to_le_bytes());
    b.extend(pcm.iter().flat_map(|s| s.to_le_bytes()));
    fs::write(path, &b)?;
    b.zeroize();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(n: usize, seed: i16) -> Vec<i16> {
        (0..n).map(|i| (i as i16).wrapping_mul(31).wrapping_add(seed)).collect()
    }

    #[test]
    fn round_trip_and_no_plaintext_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let key = AudioKey::generate().unwrap();
        let mut w = ChunkWriter::create(dir.path(), &key).unwrap();
        let (a, b) = (pcm(CHUNK_SAMPLES, 1), pcm(1234, 2));
        w.write(&a).unwrap();
        w.write(&b).unwrap();
        assert_eq!(w.samples(), (CHUNK_SAMPLES + 1234) as u64);
        assert_eq!(samples_on_disk(dir.path()).unwrap(), w.samples());

        let all = read_pcm(dir.path(), &key).unwrap();
        assert_eq!(all, [a.clone(), b].concat());
        assert_eq!(w.sha256(), hex::encode(Sha256::digest(all.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<_>>())));

        let plain: Vec<u8> = a[..64].iter().flat_map(|s| s.to_le_bytes()).collect();
        for f in chunk_files(dir.path()).unwrap() {
            assert!(!fs::read(f).unwrap().windows(64).any(|w| w == plain), "plaintext must not appear on disk");
        }
        assert!(read_pcm(dir.path(), &AudioKey::generate().unwrap()).is_err(), "wrong key");
    }

    #[test]
    fn swapped_or_missing_chunks_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let key = AudioKey::generate().unwrap();
        let mut w = ChunkWriter::create(dir.path(), &key).unwrap();
        for i in 0..3 {
            w.write(&pcm(100, i)).unwrap();
        }
        let (p0, p1) = (chunk_path(dir.path(), 0), chunk_path(dir.path(), 1));
        let (b0, b1) = (fs::read(&p0).unwrap(), fs::read(&p1).unwrap());
        fs::write(&p0, &b1).unwrap();
        fs::write(&p1, &b0).unwrap();
        assert!(read_pcm(dir.path(), &key).is_err(), "reordered");
        fs::write(&p0, &b0).unwrap();
        fs::remove_file(&p1).unwrap();
        assert!(read_pcm(dir.path(), &key).is_err(), "gap");
    }

    #[test]
    fn wav_header_is_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.wav");
        write_wav(&path, &pcm(16_000, 0)).unwrap();
        let b = fs::read(&path).unwrap();
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(b.len(), 44 + 32_000);
        assert_eq!(u32::from_le_bytes(b[24..28].try_into().unwrap()), 16_000);
    }
}
