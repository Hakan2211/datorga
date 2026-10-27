//! Encrypted backup archive (`.therabak`) and restore.
//!
//! The archive holds a consistent snapshot of `thera.db` and the encrypted audio chunks. The
//! Notfall-Schlüssel is the backup key: the header carries the master key wrapped under the
//! recovery key that was current when the backup was made, so a backup restores on a new machine
//! with the printed kit alone. The payload itself is keyed from the master key, so issuing a new
//! recovery key only rewrites the header ([`rewrap_file`]), never the payload.
//!
//! Layout: `MAGIC | header_len u32 LE | header JSON | frames`, each frame
//! `ct_len u32 LE | last u8 | AES-256-GCM ciphertext` of up to [`CHUNK`] plaintext bytes. The nonce
//! binds the chunk index and the last flag; the AAD binds the header fields that never change.
//! Truncation, reordering and appended data all fail. The plaintext stream is a list of entries
//! `1 | path_len u16 | path | size u64 | bytes`, closed by `0`.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::crypto::{self, MasterKey, RecoveryKey};
use crate::store::Store;
use crate::{Error, Result};

pub const EXTENSION: &str = "therabak";
const MAGIC: &[u8; 8] = b"THERABK\x01";
const FORMAT: u32 = 1;
const CHUNK: usize = 1 << 20;
const DB_FILE: &str = "thera.db";
const AUDIO_DIR: &str = "audio";
const MAX_HEADER: u32 = 64 * 1024;

/// Unencrypted archive header. Holds no patient data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub format: u32,
    pub created_at: String,
    pub app_version: String,
    /// [`key_id`] of the master key; tells whether the keychain key opens this backup.
    pub key_id: String,
    /// Master key wrapped under the recovery key (same format as `master.wrapped`).
    pub wrapped: String,
    salt: String,
    nonce_prefix: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct BackupSummary {
    pub created_at: String,
    pub app_version: String,
    /// Files in the archive: the database plus audio chunks.
    #[ts(type = "number")]
    pub files: u64,
    /// Plaintext bytes (the database and audio are still encrypted inside).
    #[ts(type = "number")]
    pub bytes: u64,
}

/// Non-secret fingerprint of a master key.
pub fn key_id(master: &MasterKey) -> String {
    let mut h = Sha256::new();
    h.update(b"thera-key-id-v1\0");
    h.update(master.as_bytes());
    hex::encode(&h.finalize()[..16])
}

/// Writes the archive to `out` (temp + fsync + rename). `db_snapshot` must be a consistent copy
/// ([`Store::snapshot_to`]); `wrapped` is the current `master.wrapped`.
pub fn create(out: &Path, db_snapshot: &Path, audio_root: &Path, master: &MasterKey, wrapped: &str, app_version: &str) -> Result<BackupSummary> {
    let mut salt = [0u8; 32];
    let mut prefix = [0u8; 7];
    getrandom::fill(&mut salt).map_err(|e| Error::Crypto(e.to_string()))?;
    getrandom::fill(&mut prefix).map_err(|e| Error::Crypto(e.to_string()))?;
    let header = Header {
        format: FORMAT,
        created_at: crate::domain::now(),
        app_version: app_version.to_string(),
        key_id: key_id(master),
        wrapped: wrapped.trim().to_string(),
        salt: hex::encode(salt),
        nonce_prefix: hex::encode(prefix),
    };

    let tmp = out.with_extension("therabak.tmp");
    let result = (|| {
        let mut file = BufWriter::new(File::create(&tmp)?);
        write_header(&mut file, &header)?;
        let mut enc = Encryptor::new(file, &header, master)?;
        let mut summary = BackupSummary { created_at: header.created_at.clone(), app_version: header.app_version.clone(), files: 0, bytes: 0 };
        add_file(&mut enc, DB_FILE, db_snapshot, &mut summary)?;
        for (rel, path) in audio_files(audio_root)? {
            add_file(&mut enc, &rel, &path, &mut summary)?;
        }
        enc.write_all(&[0])?;
        let file = enc.finish()?;
        let file = file.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        Ok(summary)
    })();
    match result {
        Ok(summary) => {
            fs::rename(&tmp, out)?;
            Ok(summary)
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

pub fn inspect(path: &Path) -> Result<Header> {
    read_header(&mut BufReader::new(File::open(path)?))
}

/// How a backup's master key was obtained.
pub enum Unlocked {
    /// The keychain already holds this backup's master key.
    Keychain,
    /// Opened with the Notfall-Schlüssel printed when the backup was made.
    Recovery,
}

/// The master key for `header`: the keychain key when it matches, otherwise unwrapped with
/// `recovery`.
pub fn unlock(header: &Header, keychain: Option<&MasterKey>, recovery: Option<&str>) -> Result<(MasterKey, Unlocked)> {
    if let Some(k) = keychain.filter(|k| key_id(k) == header.key_id) {
        return Ok((k.clone(), Unlocked::Keychain));
    }
    let recovery = recovery.filter(|r| !r.trim().is_empty()).ok_or_else(|| Error::Crypto("recovery_required".into()))?;
    let master = crypto::unwrap(&header.wrapped, &RecoveryKey::parse(recovery)?)
        .map_err(|_| Error::Crypto("Der Notfall-Schlüssel passt nicht zu dieser Datensicherung".into()))?;
    if key_id(&master) != header.key_id {
        return Err(Error::Crypto("Datensicherung ist beschädigt (Schlüsselkennung)".into()));
    }
    Ok((master, Unlocked::Recovery))
}

/// Decrypts the archive into `dest` (must not exist). Every chunk is authenticated; entry paths
/// are limited to `thera.db` and `audio/…`.
pub fn extract(path: &Path, master: &MasterKey, dest: &Path) -> Result<BackupSummary> {
    let mut file = BufReader::new(File::open(path)?);
    let header = read_header(&mut file)?;
    if key_id(master) != header.key_id {
        return Err(Error::Crypto("Schlüssel passt nicht zu dieser Datensicherung".into()));
    }
    fs::create_dir(dest)?;
    let mut dec = Decryptor::new(file, &header, master)?;
    let mut summary = BackupSummary { created_at: header.created_at.clone(), app_version: header.app_version.clone(), files: 0, bytes: 0 };
    let mut saw_db = false;
    loop {
        let mut tag = [0u8; 1];
        dec.read_exact(&mut tag)?;
        match tag[0] {
            0 => break,
            1 => {}
            _ => return Err(corrupt()),
        }
        let mut len = [0u8; 2];
        dec.read_exact(&mut len)?;
        let mut name = vec![0u8; u16::from_le_bytes(len) as usize];
        dec.read_exact(&mut name)?;
        let rel = String::from_utf8(name).map_err(|_| corrupt())?;
        let target = dest.join(safe_rel(&rel)?);
        saw_db |= rel == DB_FILE;
        let mut size = [0u8; 8];
        dec.read_exact(&mut size)?;
        let size = u64::from_le_bytes(size);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&target)?;
        let copied = io::copy(&mut (&mut dec).take(size), &mut out)?;
        if copied != size {
            return Err(corrupt());
        }
        out.sync_all()?;
        summary.files += 1;
        summary.bytes += size;
    }
    dec.expect_end()?;
    if !saw_db {
        return Err(corrupt());
    }
    Ok(summary)
}

/// Restore check: decrypts into `scratch`, opens the database with `master` and walks the audit
/// chain. `scratch` is always removed afterwards.
pub fn verify(path: &Path, master: &MasterKey, scratch: &Path) -> Result<BackupSummary> {
    let _ = fs::remove_dir_all(scratch);
    let result = (|| {
        let summary = extract(path, master, scratch)?;
        let store = Store::open(&scratch.join(DB_FILE), master)?;
        if let Some(seq) = crate::audit::verify(store.conn())? {
            return Err(Error::Invalid(format!("Protokollkette der Sicherung bei Eintrag {seq} unterbrochen")));
        }
        Ok(summary)
    })();
    let _ = fs::remove_dir_all(scratch);
    result
}

/// After a new Notfall-Schlüssel: re-wraps the header of a backup of `master` under `recovery`
/// (temp + rename; the payload is copied unchanged). `false` = a backup of another database.
pub fn rewrap_file(path: &Path, master: &MasterKey, recovery: &RecoveryKey) -> Result<bool> {
    let mut src = BufReader::new(File::open(path)?);
    let mut header = read_header(&mut src)?;
    if header.key_id != key_id(master) {
        return Ok(false);
    }
    header.wrapped = crypto::wrap(master, recovery)?;
    let tmp = path.with_extension("therabak.tmp");
    let result = (|| {
        let mut out = BufWriter::new(File::create(&tmp)?);
        write_header(&mut out, &header)?;
        io::copy(&mut src, &mut out)?;
        out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        Ok(())
    })();
    drop(src);
    match result {
        Ok(()) => {
            fs::rename(&tmp, path)?;
            Ok(true)
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Result of [`install`].
pub struct Installed {
    pub summary: BackupSummary,
    /// Where the data that was there before now lives (moved, never deleted). `None` if the
    /// data folder held no database.
    pub previous: Option<PathBuf>,
}

/// Replaces the database and audio in `data_dir` with the backup. The store must be closed.
/// The backup is extracted and opened in a staging folder first; only then is the current data
/// moved to `vor-wiederherstellung-<time>/` and the restored files moved in. `wrapped` replaces
/// `master.wrapped` when given (restore with the Notfall-Schlüssel). Audio paths are rewritten
/// to `data_dir/audio`, so a backup restores into any folder.
pub fn install(archive: &Path, master: &MasterKey, data_dir: &Path, wrapped: Option<&str>) -> Result<Installed> {
    let staging = data_dir.join("restore-staging");
    let _ = fs::remove_dir_all(&staging);
    let summary = match stage(archive, master, &staging, &data_dir.join(AUDIO_DIR)) {
        Ok(s) => s,
        Err(e) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(e);
        }
    };

    let mut current: Vec<PathBuf> = db_files(data_dir)?;
    if data_dir.join(AUDIO_DIR).exists() {
        current.push(data_dir.join(AUDIO_DIR));
    }
    if wrapped.is_some() && data_dir.join("master.wrapped").exists() {
        current.push(data_dir.join("master.wrapped"));
    }
    let previous = if current.is_empty() {
        None
    } else {
        let dir = data_dir.join(format!("vor-wiederherstellung-{}", crate::render::file_stamp(&crate::domain::now())));
        fs::create_dir(&dir)?;
        Some(dir)
    };

    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let result = (|| -> Result<()> {
        if let Some(dir) = &previous {
            for p in &current {
                let to = dir.join(p.file_name().expect("named"));
                fs::rename(p, &to)?;
                moved.push((to, p.clone()));
            }
        }
        for p in db_files(&staging)? {
            fs::rename(&p, data_dir.join(p.file_name().expect("named")))?;
        }
        if staging.join(AUDIO_DIR).exists() {
            fs::rename(staging.join(AUDIO_DIR), data_dir.join(AUDIO_DIR))?;
        }
        if let Some(w) = wrapped {
            crypto::write_wrapped(&data_dir.join("master.wrapped"), w)?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        // Put the previous data back; restored files that made it in go back to staging's place.
        for p in db_files(data_dir).unwrap_or_default() {
            let _ = fs::remove_file(p);
        }
        if moved.iter().any(|(_, orig)| orig.ends_with(AUDIO_DIR)) {
            let _ = fs::remove_dir_all(data_dir.join(AUDIO_DIR));
        }
        for (from, to) in moved.iter().rev() {
            let _ = fs::rename(from, to);
        }
        if let Some(dir) = &previous {
            let _ = fs::remove_dir(dir);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }
    let _ = fs::remove_dir_all(&staging);
    Ok(Installed { summary, previous })
}

fn stage(archive: &Path, master: &MasterKey, staging: &Path, final_audio: &Path) -> Result<BackupSummary> {
    let summary = extract(archive, master, staging)?;
    let store = Store::open(&staging.join(DB_FILE), master)?;
    if let Some(seq) = crate::audit::verify(store.conn())? {
        return Err(Error::Invalid(format!("Protokollkette der Sicherung bei Eintrag {seq} unterbrochen")));
    }
    store.relocate_audio(final_audio)?;
    Ok(summary)
}

/// `thera.db` and its `-wal`/`-shm` companions present in `dir`.
fn db_files(dir: &Path) -> Result<Vec<PathBuf>> {
    Ok(["", "-wal", "-shm"]
        .iter()
        .map(|s| dir.join(format!("{DB_FILE}{s}")))
        .filter(|p| p.exists())
        .collect())
}

fn corrupt() -> Error {
    Error::Crypto("Datensicherung ist beschädigt oder unvollständig".into())
}

fn safe_rel(rel: &str) -> Result<PathBuf> {
    let p = Path::new(rel);
    let ok = p.components().all(|c| matches!(c, Component::Normal(_)))
        && (rel == DB_FILE || rel.starts_with(&format!("{AUDIO_DIR}/")));
    if !ok {
        return Err(corrupt());
    }
    Ok(p.to_path_buf())
}

/// Encrypted audio chunk files below `root`, as (`audio/<dir>/<file>`, path). Temp files of a
/// chunk in flight are skipped.
fn audio_files(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    let mut stack = vec![(root.to_path_buf(), AUDIO_DIR.to_string())];
    while let Some((dir, rel)) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let child = format!("{rel}/{name}");
            if entry.file_type()?.is_dir() {
                stack.push((entry.path(), child));
            } else if !name.ends_with(".tmp") {
                out.push((child, entry.path()));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn add_file<W: Write>(enc: &mut Encryptor<W>, rel: &str, path: &Path, summary: &mut BackupSummary) -> Result<()> {
    let mut f = File::open(path)?;
    let size = f.metadata()?.len();
    enc.write_all(&[1])?;
    enc.write_all(&(rel.len() as u16).to_le_bytes())?;
    enc.write_all(rel.as_bytes())?;
    enc.write_all(&size.to_le_bytes())?;
    let copied = io::copy(&mut (&mut f).take(size), enc)?;
    if copied != size {
        return Err(Error::Invalid(format!("{rel} changed during backup")));
    }
    summary.files += 1;
    summary.bytes += size;
    Ok(())
}

fn write_header<W: Write>(w: &mut W, h: &Header) -> Result<()> {
    let json = serde_json::to_vec(h)?;
    w.write_all(MAGIC)?;
    w.write_all(&(json.len() as u32).to_le_bytes())?;
    w.write_all(&json)?;
    Ok(())
}

fn read_header<R: Read>(r: &mut R) -> Result<Header> {
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic).map_err(|_| not_a_backup())?;
    if &magic != MAGIC {
        return Err(not_a_backup());
    }
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len);
    if len > MAX_HEADER {
        return Err(corrupt());
    }
    let mut json = vec![0u8; len as usize];
    r.read_exact(&mut json)?;
    let h: Header = serde_json::from_slice(&json).map_err(|_| corrupt())?;
    if h.format != FORMAT {
        return Err(Error::Invalid(format!("Unbekanntes Sicherungsformat {}", h.format)));
    }
    Ok(h)
}

fn not_a_backup() -> Error {
    Error::Invalid("Keine thera-Datensicherung".into())
}

/// Per-backup payload key and the AAD shared by all chunks.
fn chunk_params(h: &Header, master: &MasterKey) -> Result<(Aes256Gcm, [u8; 7], Vec<u8>)> {
    let salt = hex::decode(&h.salt).map_err(|_| corrupt())?;
    let prefix: [u8; 7] = hex::decode(&h.nonce_prefix).map_err(|_| corrupt())?.try_into().map_err(|_| corrupt())?;
    let mut kh = Sha256::new();
    kh.update(b"thera-backup-v1\0");
    kh.update(&salt);
    kh.update(master.as_bytes());
    let key: Zeroizing<[u8; 32]> = Zeroizing::new(kh.finalize().into());
    let cipher = Aes256Gcm::new_from_slice(key.as_slice()).map_err(|e| Error::Crypto(e.to_string()))?;
    let mut ah = Sha256::new();
    for part in [h.format.to_string().as_str(), &h.created_at, &h.app_version, &h.key_id, &h.salt, &h.nonce_prefix] {
        ah.update((part.len() as u32).to_le_bytes());
        ah.update(part.as_bytes());
    }
    Ok((cipher, prefix, ah.finalize().to_vec()))
}

fn nonce(prefix: &[u8; 7], index: u32, last: bool) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..7].copy_from_slice(prefix);
    n[7..11].copy_from_slice(&index.to_be_bytes());
    n[11] = last as u8;
    n
}

struct Encryptor<W: Write> {
    out: W,
    cipher: Aes256Gcm,
    prefix: [u8; 7],
    aad: Vec<u8>,
    index: u32,
    buf: Zeroizing<Vec<u8>>,
}

impl<W: Write> Encryptor<W> {
    fn new(out: W, h: &Header, master: &MasterKey) -> Result<Self> {
        let (cipher, prefix, aad) = chunk_params(h, master)?;
        Ok(Self { out, cipher, prefix, aad, index: 0, buf: Zeroizing::new(Vec::with_capacity(CHUNK)) })
    }

    fn flush_chunk(&mut self, last: bool) -> io::Result<()> {
        let n = nonce(&self.prefix, self.index, last);
        let ct = self
            .cipher
            .encrypt(Nonce::from_slice(&n), Payload { msg: &self.buf, aad: &self.aad })
            .map_err(|_| io::Error::other("encrypt"))?;
        self.out.write_all(&(ct.len() as u32).to_le_bytes())?;
        self.out.write_all(&[last as u8])?;
        self.out.write_all(&ct)?;
        self.buf.clear();
        self.index = self.index.checked_add(1).ok_or_else(|| io::Error::other("backup too large"))?;
        Ok(())
    }

    fn finish(mut self) -> Result<W> {
        self.flush_chunk(true)?;
        self.out.flush()?;
        Ok(self.out)
    }
}

impl<W: Write> Write for Encryptor<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.buf.len() == CHUNK {
            self.flush_chunk(false)?;
        }
        let n = data.len().min(CHUNK - self.buf.len());
        self.buf.extend_from_slice(&data[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Decryptor<R: Read> {
    src: R,
    cipher: Aes256Gcm,
    prefix: [u8; 7],
    aad: Vec<u8>,
    index: u32,
    buf: Zeroizing<Vec<u8>>,
    pos: usize,
    done: bool,
}

impl<R: Read> Decryptor<R> {
    fn new(src: R, h: &Header, master: &MasterKey) -> Result<Self> {
        let (cipher, prefix, aad) = chunk_params(h, master)?;
        Ok(Self { src, cipher, prefix, aad, index: 0, buf: Zeroizing::new(Vec::new()), pos: 0, done: false })
    }

    fn next_chunk(&mut self) -> io::Result<()> {
        let bad = || io::Error::new(io::ErrorKind::InvalidData, "Datensicherung ist beschädigt oder unvollständig");
        let mut frame = [0u8; 5];
        self.src.read_exact(&mut frame).map_err(|_| bad())?;
        let len = u32::from_le_bytes(frame[..4].try_into().expect("4 bytes")) as usize;
        let last = match frame[4] {
            0 => false,
            1 => true,
            _ => return Err(bad()),
        };
        if len > CHUNK + 16 {
            return Err(bad());
        }
        let mut ct = vec![0u8; len];
        self.src.read_exact(&mut ct).map_err(|_| bad())?;
        let n = nonce(&self.prefix, self.index, last);
        let pt = self.cipher.decrypt(Nonce::from_slice(&n), Payload { msg: &ct, aad: &self.aad }).map_err(|_| bad())?;
        self.buf = Zeroizing::new(pt);
        self.pos = 0;
        self.index += 1;
        self.done = last;
        Ok(())
    }

    /// The end tag was read: the final chunk must be exhausted and nothing may follow it.
    fn expect_end(&mut self) -> Result<()> {
        let mut extra = [0u8; 1];
        if self.pos != self.buf.len() || !self.done || self.src.read(&mut extra)? != 0 {
            return Err(corrupt());
        }
        Ok(())
    }
}

impl<R: Read> Read for Decryptor<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.pos == self.buf.len() {
            if self.done {
                return Ok(0);
            }
            self.next_chunk()?;
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{MemoryKeyStore, Unlock};
    use crate::domain::*;

    /// A data folder with a real (keyed) store, one patient and a fake audio recording.
    struct Fixture {
        _dir: tempfile::TempDir,
        data: PathBuf,
        master: MasterKey,
        recovery: RecoveryKey,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        fs::create_dir(&data).unwrap();
        let Unlock::Created { key: master, recovery } = crypto::unlock_or_init(&MemoryKeyStore::default(), &data.join("master.wrapped")).unwrap() else {
            panic!("first run")
        };
        let store = Store::open(&data.join(DB_FILE), &master).unwrap();
        let p = store
            .create_patient(PatientInput { first_name: "Anna".into(), last_name: "Beispiel".into(), ..Default::default() })
            .unwrap();
        let s = store
            .create_session(SessionInput { patient_id: p.id, episode_id: None, scheduled_at: None, modality: None })
            .unwrap();
        let (src, mut w) = store.start_source(&s.id, SourceKind::Dictation, &data.join(AUDIO_DIR)).unwrap();
        w.write(&vec![1000i16; 16_000 * 6]).unwrap();
        store.finish_source(&src.id, "stopped", &w).unwrap();
        Fixture { _dir: dir, data, master, recovery }
    }

    fn backup(f: &Fixture, name: &str) -> PathBuf {
        let snap = f.data.join("snap.db");
        let _ = fs::remove_file(&snap);
        Store::open(&f.data.join(DB_FILE), &f.master).unwrap().snapshot_to(&snap).unwrap();
        if Store::is_encrypted() {
            assert!(!fs::read(&snap).unwrap().starts_with(b"SQLite format 3"), "snapshot stays encrypted");
        }
        let out = f.data.parent().unwrap().join(name);
        let wrapped = fs::read_to_string(f.data.join("master.wrapped")).unwrap();
        create(&out, &snap, &f.data.join(AUDIO_DIR), &f.master, &wrapped, "0.1.0").unwrap();
        fs::remove_file(&snap).unwrap();
        out
    }

    #[test]
    fn backup_verifies_and_restores_into_another_folder_with_the_recovery_key() {
        let f = fixture();
        let file = backup(&f, "a.therabak");
        let summary = verify(&file, &f.master, &f.data.parent().unwrap().join("verify")).unwrap();
        assert!(summary.files >= 2, "db + audio chunks: {summary:?}");
        assert!(!f.data.parent().unwrap().join("verify").exists(), "scratch removed");

        // New machine: empty folder, no keychain entry, only the printed key.
        let header = inspect(&file).unwrap();
        assert!(matches!(unlock(&header, None, None), Err(Error::Crypto(m)) if m == "recovery_required"));
        assert!(unlock(&header, None, Some(&RecoveryKey::generate().unwrap().display())).is_err());
        let (master, how) = unlock(&header, None, Some(&f.recovery.display())).unwrap();
        assert!(matches!(how, Unlocked::Recovery));
        let fresh = f.data.parent().unwrap().join("fresh");
        fs::create_dir(&fresh).unwrap();
        let done = install(&file, &master, &fresh, Some(&header.wrapped)).unwrap();
        assert!(done.previous.is_none());

        let store = Store::open(&fresh.join(DB_FILE), &master).unwrap();
        assert_eq!(store.list_patients(None).unwrap().len(), 1);
        let src_id: String = store.conn().query_row("SELECT source_id FROM audio_asset", [], |r| r.get(0)).unwrap();
        let (dir, key) = store.audio_location(&src_id).unwrap();
        assert!(dir.starts_with(&fresh), "audio path relocated: {}", dir.display());
        assert_eq!(crate::audio::read_pcm(&dir, &key).unwrap().len(), 16_000 * 6);
        // The restored master.wrapped opens with the same printed key.
        assert!(crypto::check_recovery(&fresh.join("master.wrapped"), &f.recovery.display()).is_ok());
    }

    #[test]
    fn restore_over_existing_data_moves_it_aside() {
        let f = fixture();
        let file = backup(&f, "a.therabak");
        // Data changes after the backup.
        Store::open(&f.data.join(DB_FILE), &f.master)
            .unwrap()
            .create_patient(PatientInput { first_name: "Neu".into(), last_name: "Person".into(), ..Default::default() })
            .unwrap();
        let header = inspect(&file).unwrap();
        let (master, how) = unlock(&header, Some(&f.master), None).unwrap();
        assert!(matches!(how, Unlocked::Keychain));
        let done = install(&file, &master, &f.data, None).unwrap();
        let aside = done.previous.unwrap();
        assert!(aside.join(DB_FILE).exists() && aside.join(AUDIO_DIR).exists());
        assert!(!aside.join("master.wrapped").exists(), "kept in place when restored via keychain");
        assert_eq!(Store::open(&f.data.join(DB_FILE), &f.master).unwrap().list_patients(None).unwrap().len(), 1);
        assert_eq!(Store::open(&aside.join(DB_FILE), &f.master).unwrap().list_patients(None).unwrap().len(), 2);
        assert!(!f.data.join("restore-staging").exists());
    }

    #[test]
    fn tampering_truncation_and_wrong_key_fail() {
        let f = fixture();
        let file = backup(&f, "a.therabak");
        let bytes = fs::read(&file).unwrap();
        let scratch = f.data.parent().unwrap().join("x");

        let mut flipped = bytes.clone();
        let at = bytes.len() - 40;
        flipped[at] ^= 1;
        fs::write(&file, &flipped).unwrap();
        assert!(verify(&file, &f.master, &scratch).is_err(), "flipped bit");

        fs::write(&file, &bytes[..bytes.len() - 10]).unwrap();
        assert!(verify(&file, &f.master, &scratch).is_err(), "truncated");

        let mut appended = bytes.clone();
        appended.extend_from_slice(&bytes[bytes.len() - 30..]);
        fs::write(&file, &appended).unwrap();
        assert!(verify(&file, &f.master, &scratch).is_err(), "trailing data");

        fs::write(&file, &bytes).unwrap();
        assert!(verify(&file, &MasterKey::generate().unwrap(), &scratch).is_err(), "other database");
        assert!(verify(&file, &f.master, &scratch).is_ok());

        fs::write(&file, b"not a backup at all").unwrap();
        assert!(matches!(inspect(&file), Err(Error::Invalid(_))));
    }

    #[test]
    fn rewrap_moves_backup_to_new_recovery_key() {
        let f = fixture();
        let file = backup(&f, "a.therabak");
        let before = fs::read(&file).unwrap();
        let new = RecoveryKey::generate().unwrap();
        assert!(rewrap_file(&file, &f.master, &new).unwrap());
        assert!(!rewrap_file(&file, &MasterKey::generate().unwrap(), &new).unwrap(), "foreign backup untouched");
        let header = inspect(&file).unwrap();
        assert!(unlock(&header, None, Some(&f.recovery.display())).is_err(), "old key no longer opens it");
        let (master, _) = unlock(&header, None, Some(&new.display())).unwrap();
        assert!(verify(&file, &master, &f.data.parent().unwrap().join("v")).is_ok());
        assert_ne!(before, fs::read(&file).unwrap());
    }

    #[test]
    fn entry_paths_cannot_escape() {
        for bad in ["../x", "audio/../../x", "/etc/passwd", "C:/x", "other.db", "audio\\..\\x"] {
            assert!(safe_rel(bad).is_err(), "{bad}");
        }
        assert!(safe_rel("audio/abc/000001.chunk").is_ok());
        assert!(safe_rel("thera.db").is_ok());
    }
}
