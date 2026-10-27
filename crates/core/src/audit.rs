//! Append-only, hash-chained audit log.
//!
//! Each event hashes the previous event's hash, so any edit or deletion in the middle of
//! the chain is detectable. `meta` must stay PII-free (IDs, counts, enums, hashes only):
//! that keeps the chain valid after a patient's hard erasure. [`append`] enforces it.

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::{now, AuditEvent};
use crate::{Error, Result};

pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

pub struct Entry<'a> {
    pub actor: &'a str,
    pub action: &'a str,
    pub entity_type: &'a str,
    pub entity_id: Option<&'a str>,
    pub meta: Value,
}

pub fn append(conn: &Connection, e: Entry<'_>) -> Result<()> {
    ensure_pii_free(&e.meta)?;
    let prev: String = conn
        .query_row("SELECT hash FROM audit_event ORDER BY seq DESC LIMIT 1", [], |r| r.get(0))
        .optional()?
        .unwrap_or_else(|| GENESIS.to_string());
    let at = now();
    let meta = serde_json::to_string(&e.meta)?;
    let hash = chain_hash(&prev, &at, e.actor, e.action, e.entity_type, e.entity_id, &meta);
    conn.execute(
        "INSERT INTO audit_event (at, actor, action, entity_type, entity_id, meta_json, prev_hash, hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![at, e.actor, e.action, e.entity_type, e.entity_id, meta, prev, hash],
    )?;
    Ok(())
}

pub fn list(conn: &Connection, limit: i64) -> Result<Vec<AuditEvent>> {
    let mut stmt = conn.prepare(
        "SELECT seq, at, actor, action, entity_type, entity_id, meta_json, prev_hash, hash
         FROM audit_event ORDER BY seq DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], row_to_event)?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Walks the chain. Returns the `seq` of the first broken event, or `None` if intact.
pub fn verify(conn: &Connection) -> Result<Option<i64>> {
    let mut stmt = conn.prepare(
        "SELECT seq, at, actor, action, entity_type, entity_id, meta_json, prev_hash, hash
         FROM audit_event ORDER BY seq ASC",
    )?;
    let mut prev = GENESIS.to_string();
    for ev in stmt.query_map([], row_to_event)? {
        let ev = ev?;
        let expected = chain_hash(
            &prev, &ev.at, &ev.actor, &ev.action, &ev.entity_type, ev.entity_id.as_deref(), &ev.meta_json,
        );
        if ev.prev_hash != prev || ev.hash != expected {
            return Ok(Some(ev.seq));
        }
        prev = ev.hash;
    }
    Ok(None)
}

fn chain_hash(
    prev: &str,
    at: &str,
    actor: &str,
    action: &str,
    entity_type: &str,
    entity_id: Option<&str>,
    meta: &str,
) -> String {
    let mut h = Sha256::new();
    for part in [prev, at, actor, action, entity_type, entity_id.unwrap_or(""), meta] {
        h.update(part.as_bytes());
        h.update([0x1f]); // unit separator: unambiguous field boundaries
    }
    hex::encode(h.finalize())
}

/// Strings in meta must be identifier-like: no whitespace, at most 64 chars.
/// That admits UUIDs, enum values, hashes and keys, and rejects names and free text.
fn ensure_pii_free(v: &Value) -> Result<()> {
    match v {
        Value::String(s) if s.len() > 64 || s.chars().any(char::is_whitespace) => Err(Error::Invalid(
            "audit meta must not contain free text (PII guard)".into(),
        )),
        Value::Array(a) => a.iter().try_for_each(ensure_pii_free),
        Value::Object(o) => o.values().try_for_each(ensure_pii_free),
        _ => Ok(()),
    }
}

fn row_to_event(r: &rusqlite::Row<'_>) -> rusqlite::Result<AuditEvent> {
    Ok(AuditEvent {
        seq: r.get(0)?,
        at: r.get(1)?,
        actor: r.get(2)?,
        action: r.get(3)?,
        entity_type: r.get(4)?,
        entity_id: r.get(5)?,
        meta_json: r.get(6)?,
        prev_hash: r.get(7)?,
        hash: r.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use serde_json::json;

    fn entry(action: &str) -> Entry<'_> {
        Entry { actor: "therapist", action, entity_type: "patient", entity_id: Some("p1"), meta: json!({"n": 1}) }
    }

    #[test]
    fn chain_verifies_and_detects_tampering() {
        let store = Store::open_in_memory().unwrap();
        let c = store.conn();
        for a in ["create", "update", "sign"] {
            append(c, entry(a)).unwrap();
        }
        assert_eq!(verify(c).unwrap(), None);

        // The triggers block normal edits...
        assert!(c.execute("UPDATE audit_event SET action='x' WHERE seq=2", []).is_err());
        // ...an attacker with raw file access can drop them, but the chain still exposes the edit.
        c.execute_batch("DROP TRIGGER audit_no_update; UPDATE audit_event SET action='delete' WHERE seq=2;")
            .unwrap();
        assert_eq!(verify(c).unwrap(), Some(2));
    }

    #[test]
    fn rejects_pii_in_meta() {
        let store = Store::open_in_memory().unwrap();
        let mut e = entry("create");
        e.meta = json!({"name": "Anna Müller"});
        assert!(append(store.conn(), e).is_err());
    }
}
