//! Master key handling.
//!
//! The 256-bit master key keys the SQLCipher database (and, later, per-file audio DEKs).
//! It lives in the OS keychain (via a [`KeyStore`] implemented by the app shell) and is
//! additionally wrapped under a recovery key the therapist prints once ("Notfall-Kit").
//! Losing the keychain entry is then recoverable; losing both is not.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{Error, Result};

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MasterKey([u8; 32]);

impl MasterKey {
    pub fn generate() -> Result<Self> {
        Ok(Self(random_32()?))
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Result<Self> {
        Ok(Self(decode_32(s)?))
    }

    /// Raw-key form for `PRAGMA key` (skips SQLCipher's passphrase KDF; the key is already random).
    pub fn sqlcipher_pragma(&self) -> String {
        format!("x'{}'", self.to_hex())
    }
}

/// Printed once at first run. 64 hex chars shown in 16 groups of 4.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct RecoveryKey([u8; 32]);

impl RecoveryKey {
    pub fn generate() -> Result<Self> {
        Ok(Self(random_32()?))
    }

    pub fn display(&self) -> String {
        let h = hex::encode(self.0).to_uppercase();
        h.as_bytes()
            .chunks(4)
            .map(|c| std::str::from_utf8(c).expect("hex is ascii"))
            .collect::<Vec<_>>()
            .join("-")
    }

    pub fn parse(input: &str) -> Result<Self> {
        let cleaned: String = input.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        Ok(Self(decode_32(&cleaned)?))
    }
}

/// `nonce || ciphertext`, hex-encoded. Safe to store on disk next to the database.
pub fn wrap(master: &MasterKey, recovery: &RecoveryKey) -> Result<String> {
    let cipher = Aes256Gcm::new_from_slice(&recovery.0).map_err(|e| Error::Crypto(e.to_string()))?;
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).map_err(|e| Error::Crypto(e.to_string()))?;
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), master.0.as_slice())
        .map_err(|e| Error::Crypto(e.to_string()))?;
    let mut out = nonce.to_vec();
    out.extend(ct);
    Ok(hex::encode(out))
}

pub fn unwrap(wrapped: &str, recovery: &RecoveryKey) -> Result<MasterKey> {
    let raw = hex::decode(wrapped.trim()).map_err(|e| Error::Crypto(e.to_string()))?;
    if raw.len() < 12 {
        return Err(Error::Crypto("wrapped key too short".into()));
    }
    let (nonce, ct) = raw.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(&recovery.0).map_err(|e| Error::Crypto(e.to_string()))?;
    let mut pt = cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| Error::Crypto("Wiederherstellungsschlüssel ist falsch".into()))?;
    let key: [u8; 32] = pt
        .as_slice()
        .try_into()
        .map_err(|_| Error::Crypto("wrapped key has wrong length".into()))?;
    pt.zeroize();
    Ok(MasterKey(key))
}

/// Where the master key lives at rest. The app shell implements this over the OS keychain.
pub trait KeyStore {
    fn load(&self) -> Result<Option<MasterKey>>;
    fn store(&self, key: &MasterKey) -> Result<()>;
}

/// For tests and the headless CLI.
#[derive(Default)]
pub struct MemoryKeyStore(std::sync::Mutex<Option<MasterKey>>);

impl KeyStore for MemoryKeyStore {
    fn load(&self) -> Result<Option<MasterKey>> {
        Ok(self.0.lock().expect("poisoned").clone())
    }
    fn store(&self, key: &MasterKey) -> Result<()> {
        *self.0.lock().expect("poisoned") = Some(key.clone());
        Ok(())
    }
}

pub enum Unlock {
    /// Key loaded from the keystore.
    Existing(MasterKey),
    /// First run: the recovery key must be shown to the therapist exactly once.
    Created { key: MasterKey, recovery: RecoveryKey },
}

/// Load the master key, or create it (plus recovery key and wrapped copy) on first run.
/// `wrapped_path` holds the recovery-wrapped key; its presence without a keystore entry
/// means the keychain was lost and the user must enter the recovery key.
pub fn unlock_or_init(store: &dyn KeyStore, wrapped_path: &std::path::Path) -> Result<Unlock> {
    if let Some(key) = store.load()? {
        return Ok(Unlock::Existing(key));
    }
    if wrapped_path.exists() {
        return Err(Error::Crypto("recovery_required".into()));
    }
    let key = MasterKey::generate()?;
    let recovery = RecoveryKey::generate()?;
    write_wrapped(wrapped_path, &wrap(&key, &recovery)?)?;
    store.store(&key)?;
    Ok(Unlock::Created { key, recovery })
}

/// Issue a new recovery key: `key` (the live master key) is wrapped under it and replaces
/// `wrapped_path` atomically. Every earlier recovery key stops working.
pub fn rewrap(key: &MasterKey, wrapped_path: &std::path::Path) -> Result<RecoveryKey> {
    let recovery = RecoveryKey::generate()?;
    write_wrapped(wrapped_path, &wrap(key, &recovery)?)?;
    Ok(recovery)
}

/// Parses `recovery` and checks it opens `wrapped_path`, so a kit never prints a dead key.
pub fn check_recovery(wrapped_path: &std::path::Path, recovery: &str) -> Result<RecoveryKey> {
    let r = RecoveryKey::parse(recovery)?;
    unwrap(&std::fs::read_to_string(wrapped_path)?, &r)?;
    Ok(r)
}

/// temp + fsync + rename: a crash leaves either the old or the new wrapped key, never half.
pub(crate) fn write_wrapped(path: &std::path::Path, contents: &str) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("wrapped.tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(contents.as_bytes())?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Restore the keychain entry from the printed recovery key.
pub fn recover(store: &dyn KeyStore, wrapped_path: &std::path::Path, recovery: &str) -> Result<MasterKey> {
    let wrapped = std::fs::read_to_string(wrapped_path)?;
    let key = unwrap(&wrapped, &RecoveryKey::parse(recovery)?)?;
    store.store(&key)?;
    Ok(key)
}

fn random_32() -> Result<[u8; 32]> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| Error::Crypto(e.to_string()))?;
    Ok(b)
}

fn decode_32(s: &str) -> Result<[u8; 32]> {
    hex::decode(s)
        .map_err(|e| Error::Crypto(e.to_string()))?
        .try_into()
        .map_err(|_| Error::Crypto("expected 32 bytes".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_roundtrip_and_wrong_key() {
        let k = MasterKey::generate().unwrap();
        let r = RecoveryKey::generate().unwrap();
        let w = wrap(&k, &r).unwrap();
        let parsed = RecoveryKey::parse(&r.display()).unwrap();
        assert_eq!(unwrap(&w, &parsed).unwrap().as_bytes(), k.as_bytes());
        assert!(unwrap(&w, &RecoveryKey::generate().unwrap()).is_err());
    }

    #[test]
    fn first_run_then_keychain_loss_then_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let wrapped = dir.path().join("master.wrapped");
        let store = MemoryKeyStore::default();
        let (key, recovery) = match unlock_or_init(&store, &wrapped).unwrap() {
            Unlock::Created { key, recovery } => (key, recovery),
            Unlock::Existing(_) => panic!("expected first run"),
        };
        assert!(matches!(unlock_or_init(&store, &wrapped).unwrap(), Unlock::Existing(_)));

        let lost = MemoryKeyStore::default();
        assert!(unlock_or_init(&lost, &wrapped).is_err());
        let restored = recover(&lost, &wrapped, &recovery.display()).unwrap();
        assert_eq!(restored.as_bytes(), key.as_bytes());
    }

    #[test]
    fn rewrap_replaces_old_recovery_key() {
        let dir = tempfile::tempdir().unwrap();
        let wrapped = dir.path().join("master.wrapped");
        let store = MemoryKeyStore::default();
        let Unlock::Created { key, recovery: old } = unlock_or_init(&store, &wrapped).unwrap() else {
            panic!("expected first run")
        };
        let new = rewrap(&key, &wrapped).unwrap();
        assert!(!dir.path().join("master.wrapped.tmp").exists());
        assert!(check_recovery(&wrapped, &new.display()).is_ok());
        assert!(check_recovery(&wrapped, &old.display()).is_err());

        let lost = MemoryKeyStore::default();
        assert!(recover(&lost, &wrapped, &old.display()).is_err(), "old printed key is dead");
        assert!(lost.load().unwrap().is_none());
        let restored = recover(&lost, &wrapped, &new.display()).unwrap();
        assert_eq!(restored.as_bytes(), key.as_bytes());
        // Lower case and spaces instead of dashes still parse.
        let typed = new.display().to_lowercase().replace('-', " ");
        assert!(check_recovery(&wrapped, &typed).is_ok());
    }
}
