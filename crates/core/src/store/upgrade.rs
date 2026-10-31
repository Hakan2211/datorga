//! Safety around app updates: before migrations change an existing database, a copy of the old
//! one goes to `vor-update/` next to it; a database from a newer app version is never opened.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::MIGRATIONS;
use crate::domain::now;
use crate::{Error, Result};

/// Copies kept in `vor-update/`; older ones are deleted after a new copy.
pub const KEEP: usize = 3;

/// `<db dir>/vor-update`, where the copies before an update go.
pub fn pre_update_dir(db: &Path) -> PathBuf {
    db.parent().unwrap_or(Path::new(".")).join("vor-update")
}

pub(super) fn schema_version(conn: &Connection) -> Result<usize> {
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? as usize)
}

/// Runs before `migrate`. Refuses a schema newer than this build knows (an older version
/// started after an update would otherwise run on tables it doesn't understand). An existing
/// database that is about to be migrated is copied first (`VACUUM INTO`, same key); returns
/// the copy's path.
pub(super) fn guard(conn: &Connection, db: Option<&Path>) -> Result<Option<PathBuf>> {
    let current = schema_version(conn)?;
    let latest = MIGRATIONS.len();
    if current > latest {
        return Err(Error::Upgrade(format!(
            "Diese Daten wurden schon von einer neueren Theralo-Version geändert (Stand {current}, diese Version kennt {latest}). Bitte die neueste Version installieren."
        )));
    }
    let Some(db) = db else { return Ok(None) };
    if current == 0 || current == latest {
        return Ok(None);
    }
    let dir = pre_update_dir(db);
    std::fs::create_dir_all(&dir)?;
    // Timestamp first: names sort by age.
    let stamp: String = now().chars().take(19).map(|c| if c == ':' { '-' } else { c }).collect();
    let copy = dir.join(format!("{stamp}-stand{current}.db"));
    if copy.exists() {
        std::fs::remove_file(&copy)?;
    }
    conn.execute("VACUUM INTO ?1", [copy.to_string_lossy()])?;
    prune(&dir)?;
    Ok(Some(copy))
}

/// Keeps the newest `KEEP` copies.
fn prune(dir: &Path) -> Result<()> {
    let mut copies: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "db"))
        .collect();
    copies.sort();
    let excess = copies.len().saturating_sub(KEEP);
    for old in &copies[..excess] {
        std::fs::remove_file(old)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::MasterKey;
    use crate::store::Store;

    fn copies(db: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(pre_update_dir(db)).map_or(vec![], |d| d.map(|e| e.unwrap().path()).collect());
        v.sort();
        v
    }

    fn set_version(path: &Path, key: &MasterKey, v: i64) {
        let s = Store::open(path, key).unwrap();
        s.conn().pragma_update(None, "user_version", v).unwrap();
    }

    #[test]
    fn new_and_current_databases_are_not_copied() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("thera.db");
        let key = MasterKey::generate().unwrap();
        Store::open(&path, &key).unwrap();
        Store::open(&path, &key).unwrap();
        assert!(copies(&path).is_empty());
    }

    #[test]
    fn migration_copies_the_old_database_first() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("thera.db");
        let key = MasterKey::generate().unwrap();
        {
            let s = Store::open(&path, &key).unwrap();
            s.set_setting("therapist_name", &serde_json::json!("Dr. Alt")).unwrap();
        }
        // Pretend the last migration hasn't run yet; call the guard directly (re-running that
        // migration's SQL on top would fail).
        let latest = MIGRATIONS.len() as i64;
        set_version(&path, &key, latest - 1);
        let conn = Connection::open(&path).unwrap();
        #[cfg(feature = "sqlcipher")]
        conn.pragma_update(None, "key", key.sqlcipher_pragma()).unwrap();
        let copy = guard(&conn, Some(&path)).unwrap().expect("copied");
        assert!(copy.file_name().unwrap().to_string_lossy().ends_with(&format!("-stand{}.db", latest - 1)));
        drop(conn);

        // The copy opens with the same key and holds the old data at the old version.
        let old = Connection::open(&copy).unwrap();
        #[cfg(feature = "sqlcipher")]
        old.pragma_update(None, "key", key.sqlcipher_pragma()).unwrap();
        assert_eq!(schema_version(&old).unwrap() as i64, latest - 1);
        let name: String = old.query_row("SELECT value_json FROM app_setting WHERE key = 'therapist_name'", [], |r| r.get(0)).unwrap();
        assert!(name.contains("Dr. Alt"), "{name}");
    }

    #[test]
    fn only_the_newest_copies_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("thera.db");
        let backups = pre_update_dir(&path);
        std::fs::create_dir_all(&backups).unwrap();
        for name in ["2026-01-01T00-00-00-stand20.db", "2026-02-01T00-00-00-stand21.db", "2026-03-01T00-00-00-stand22.db", "2026-04-01T00-00-00-stand23.db"] {
            std::fs::write(backups.join(name), b"x").unwrap();
        }
        prune(&backups).unwrap();
        let left: Vec<String> = copies(&path).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(left, ["2026-02-01T00-00-00-stand21.db", "2026-03-01T00-00-00-stand22.db", "2026-04-01T00-00-00-stand23.db"]);
    }

    #[test]
    fn a_newer_database_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("thera.db");
        let key = MasterKey::generate().unwrap();
        set_version(&path, &key, MIGRATIONS.len() as i64 + 1);
        let err = Store::open(&path, &key).err().expect("refused");
        assert!(matches!(&err, Error::Upgrade(m) if m.contains("neueren Theralo-Version")), "{err:?}");
        assert!(copies(&path).is_empty());
    }
}
