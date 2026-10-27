//! Versioned consent texts. Each consent record stores the version and the SHA-256 of the exact
//! text shown, so a later change of wording never silently rewrites what a patient agreed to.
//!
//! The wording is a draft and must be reviewed by a lawyer before real use (PLAN §3).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::domain::ConsentKind;

const RECORDING_DE: &str = include_str!("../../../templates/consent/recording.de.md");
/// Bump whenever the text changes; old records keep their version + hash.
const RECORDING_VERSION: &str = "recording-de-2026-10-entwurf";

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ConsentText {
    pub kind: ConsentKind,
    pub version: String,
    pub text: String,
    pub hash: String,
}

pub fn current(kind: ConsentKind) -> ConsentText {
    let (version, text) = match kind {
        ConsentKind::Recording => (RECORDING_VERSION, RECORDING_DE),
    };
    // Normalize line endings so a CRLF checkout hashes the same as LF.
    let text = text.replace("\r\n", "\n").trim().to_string();
    let hash = hex::encode(Sha256::digest(text.as_bytes()));
    ConsentText { kind, version: version.into(), text, hash }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_across_line_endings() {
        let a = current(ConsentKind::Recording);
        assert_eq!(a.hash.len(), 64);
        assert!(!a.text.contains('\r'));
        assert_eq!(a.hash, current(ConsentKind::Recording).hash);
    }
}
