//! Downloadable model files (speech and language models): catalog entry and hash check.

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::Result;

/// A model file with a pinned download. Hashes are the SHA-256 of the file on Hugging Face.
#[derive(Debug, Clone, Copy)]
pub struct ModelInfo {
    pub id: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

/// Streaming SHA-256 of a file (models are up to 15 GB).
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}
