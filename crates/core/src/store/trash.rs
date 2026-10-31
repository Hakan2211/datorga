//! Papierkorb (slice 43): deleting a session with a signed note, or a signed Bericht, keeps a
//! snapshot of every row the delete removes (found by walking the `ON DELETE CASCADE` foreign
//! keys) for 30 days. Nothing else in the store has to know: trashed rows are simply gone until
//! they are restored. Audio folders stay on disk until the item is purged.

use std::collections::{BTreeMap, BTreeSet};

use base64::Engine as _;
use rusqlite::types::Value;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

use super::{new_id, Store, ACTOR};
use crate::audit::{self, Entry};
use crate::domain::*;
use crate::{Error, Result};

/// Days an item stays restorable.
pub const TRASH_DAYS: i64 = 30;

/// Never snapshotted: jobs are transient (a restored queued job would run again), the audit log
/// is append-only, and the trash doesn't contain itself.
const SKIP_TABLES: &[&str] = &["job", "audit_event", "trash_item"];

/// What a delete did: removed for good, or moved to the Papierkorb.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DeleteOutcome {
    pub trashed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TrashItem {
    pub id: String,
    pub patient_id: String,
    pub patient_name: String,
    /// `session` | `bericht`
    pub kind: String,
    pub entity_id: String,
    /// Bericht: the Antragsart.
    pub detail: Option<String>,
    pub occurred_at: Option<String>,
    pub deleted_at: String,
    /// When it is purged.
    pub purge_at: String,
}

/// The removed rows of one table.
#[derive(Serialize, Deserialize)]
struct TableRows {
    table: String,
    columns: Vec<String>,
    rows: Vec<Vec<serde_json::Value>>,
}

struct Edge {
    child: String,
    from: String,
    parent: String,
    to: String,
}

/// Every `ON DELETE CASCADE` foreign key in the schema.
fn cascade_edges(conn: &Connection) -> Result<Vec<Edge>> {
    let mut stmt = conn.prepare(
        "SELECT m.name, f.\"from\", f.\"table\", f.\"to\" FROM sqlite_master m, pragma_foreign_key_list(m.name) f
         WHERE m.type = 'table' AND f.on_delete = 'CASCADE'",
    )?;
    let edges = stmt
        .query_map([], |r| Ok(Edge { child: r.get(0)?, from: r.get(1)?, parent: r.get(2)?, to: r.get(3)? }))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(edges.into_iter().filter(|e| !SKIP_TABLES.contains(&e.child.as_str())).collect())
}

fn rowid_list(ids: &BTreeSet<i64>) -> String {
    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

fn to_json(v: Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Integer(i) => json!(i),
        Value::Real(f) => json!(f),
        Value::Text(s) => json!(s),
        Value::Blob(b) => json!({ "b64": base64::engine::general_purpose::STANDARD.encode(b) }),
    }
}

fn from_json(v: &serde_json::Value) -> Result<Value> {
    Ok(match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::String(s) => Value::Text(s.clone()),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::Integer(i),
            None => Value::Real(n.as_f64().unwrap_or_default()),
        },
        serde_json::Value::Object(o) => {
            let b64 = o.get("b64").and_then(|b| b.as_str()).ok_or_else(|| Error::Invalid("Papierkorb: unbekannter Wert".into()))?;
            Value::Blob(base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| Error::Invalid(format!("Papierkorb: {e}")))?)
        }
        _ => return Err(Error::Invalid("Papierkorb: unbekannter Wert".into())),
    })
}

/// The rows deleting `roots` would remove: the roots plus everything that cascades from them.
fn snapshot(conn: &Connection, roots: &[(&str, &str)]) -> Result<Vec<TableRows>> {
    let edges = cascade_edges(conn)?;
    let mut found: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut fresh: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    for (table, id) in roots {
        let rowid: Option<i64> =
            conn.query_row(&format!("SELECT rowid FROM \"{table}\" WHERE id = ?1"), [id], |r| r.get(0)).optional()?;
        if let Some(rowid) = rowid {
            fresh.entry(table.to_string()).or_default().insert(rowid);
        }
    }
    while !fresh.is_empty() {
        let mut next: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
        for (table, ids) in &fresh {
            let known = found.entry(table.clone()).or_default();
            if known.is_empty() {
                order.push(table.clone());
            }
            known.extend(ids);
        }
        for (parent, ids) in &fresh {
            for e in edges.iter().filter(|e| &e.parent == parent) {
                let sql = format!(
                    "SELECT rowid FROM \"{}\" WHERE \"{}\" IN (SELECT \"{}\" FROM \"{}\" WHERE rowid IN ({}))",
                    e.child, e.from, e.to, e.parent, rowid_list(ids)
                );
                let mut stmt = conn.prepare(&sql)?;
                for rowid in stmt.query_map([], |r| r.get::<_, i64>(0))? {
                    let rowid = rowid?;
                    if !found.get(&e.child).is_some_and(|s| s.contains(&rowid)) {
                        next.entry(e.child.clone()).or_default().insert(rowid);
                    }
                }
            }
        }
        fresh = next;
    }
    let mut out = Vec::new();
    for table in order {
        let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\" WHERE rowid IN ({})", rowid_list(&found[&table])))?;
        let columns: Vec<String> = stmt.column_names().into_iter().map(str::to_string).collect();
        let n = columns.len();
        let rows = stmt
            .query_map([], |r| (0..n).map(|i| r.get::<_, Value>(i).map(to_json)).collect::<rusqlite::Result<Vec<_>>>())?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        out.push(TableRows { table, columns, rows });
    }
    Ok(out)
}

/// Audio folders named in a snapshot.
fn audio_dirs(payload: &str) -> Vec<String> {
    let Ok(tables) = serde_json::from_str::<Vec<TableRows>>(payload) else { return Vec::new() };
    tables
        .iter()
        .filter(|t| t.table == "audio_asset")
        .flat_map(|t| {
            let i = t.columns.iter().position(|c| c == "path");
            t.rows.iter().filter_map(move |r| i.and_then(|i| r[i].as_str().map(str::to_string)))
        })
        .collect()
}

fn purge_cutoff() -> String {
    let t = time::OffsetDateTime::now_utc() - time::Duration::days(TRASH_DAYS);
    t.format(&time::format_description::well_known::Rfc3339).expect("rfc3339 formatting cannot fail")
}

fn plus_days(ts: &str) -> String {
    time::OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339)
        .map(|t| (t + time::Duration::days(TRASH_DAYS)).format(&time::format_description::well_known::Rfc3339).expect("rfc3339"))
        .unwrap_or_default()
}

pub(super) struct TrashEntry<'a> {
    pub patient_id: &'a str,
    pub kind: &'a str,
    pub entity_id: &'a str,
    pub detail: Option<&'a str>,
    pub occurred_at: Option<&'a str>,
}

impl Store {
    /// Inside the caller's transaction, before it deletes `roots`: stores what the delete will remove.
    pub(super) fn move_to_trash(conn: &Connection, item: TrashEntry<'_>, roots: &[(&str, &str)]) -> Result<()> {
        let payload = serde_json::to_string(&snapshot(conn, roots)?)?;
        let id = new_id();
        conn.execute(
            "INSERT INTO trash_item (id, patient_id, kind, entity_id, detail, occurred_at, deleted_at, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, item.patient_id, item.kind, item.entity_id, item.detail, item.occurred_at, now(), payload],
        )?;
        Ok(())
    }

    /// Newest deletion first.
    pub fn list_trash(&self) -> Result<Vec<TrashItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.patient_id, p.first_name || ' ' || p.last_name, t.kind, t.entity_id, t.detail, t.occurred_at, t.deleted_at
             FROM trash_item t JOIN patient p ON p.id = t.patient_id ORDER BY t.deleted_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            let deleted_at: String = r.get(7)?;
            Ok(TrashItem {
                id: r.get(0)?,
                patient_id: r.get(1)?,
                patient_name: r.get(2)?,
                kind: r.get(3)?,
                entity_id: r.get(4)?,
                detail: r.get(5)?,
                occurred_at: r.get(6)?,
                purge_at: plus_days(&deleted_at),
                deleted_at,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Puts the rows back. Rows whose outside parent is gone meanwhile (e.g. a Bericht that cited
    /// a fact of a restored session was deleted) are left out; the item itself must come back.
    pub fn restore_trash(&self, id: &str) -> Result<()> {
        let (kind, entity_id, payload): (String, String, String) = self
            .conn
            .query_row("SELECT kind, entity_id, payload FROM trash_item WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("trash {id}")))?;
        let tables: Vec<TableRows> = serde_json::from_str(&payload)?;
        let tx = self.conn.unchecked_transaction()?;
        tx.pragma_update(None, "defer_foreign_keys", "ON")?;
        let mut inserted: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
        for t in &tables {
            // The schema may have changed since: keep the columns that still exist.
            let existing: BTreeSet<String> = tx
                .prepare(&format!("SELECT name FROM pragma_table_info('{}')", t.table))?
                .query_map([], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            let keep: Vec<usize> = (0..t.columns.len()).filter(|&i| existing.contains(&t.columns[i])).collect();
            if keep.is_empty() {
                continue;
            }
            let cols = keep.iter().map(|&i| format!("\"{}\"", t.columns[i])).collect::<Vec<_>>().join(", ");
            let marks = (1..=keep.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
            let mut stmt = tx.prepare(&format!("INSERT INTO \"{}\" ({cols}) VALUES ({marks})", t.table))?;
            for row in &t.rows {
                let values = keep.iter().map(|&i| from_json(&row[i])).collect::<Result<Vec<_>>>()?;
                stmt.execute(rusqlite::params_from_iter(values))?;
                inserted.entry(t.table.clone()).or_default().insert(tx.last_insert_rowid());
            }
        }
        // Drop restored rows whose parent no longer exists (cascading to their own children),
        // until nothing we inserted points nowhere.
        loop {
            let mut stmt = tx.prepare("SELECT \"table\", rowid FROM pragma_foreign_key_check")?;
            let broken: Vec<(String, i64)> = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|(t, rowid)| inserted.get(t).is_some_and(|s| s.contains(rowid)))
                .collect();
            if broken.is_empty() {
                break;
            }
            for (table, rowid) in broken {
                tx.execute(&format!("DELETE FROM \"{table}\" WHERE rowid = ?1"), [rowid])?;
                if let Some(s) = inserted.get_mut(&table) {
                    s.remove(&rowid);
                }
            }
        }
        let root = if kind == "session" { "session" } else { "report" };
        let back: bool = tx.query_row(&format!("SELECT EXISTS (SELECT 1 FROM {root} WHERE id = ?1)"), [&entity_id], |r| r.get(0))?;
        if !back {
            return Err(Error::Invalid("Kann nicht wiederhergestellt werden: der Behandlungsfall existiert nicht mehr".into()));
        }
        tx.execute("DELETE FROM trash_item WHERE id = ?1", [id])?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: &format!("{kind}.restore"), entity_type: if kind == "session" { "session" } else { "report" },
            entity_id: Some(&entity_id), meta: json!({}),
        })?;
        tx.commit()?;
        self.sync_review_statuses()?;
        Ok(())
    }

    /// "Endgültig löschen" for one item (`Some`) or the whole Papierkorb (`None`).
    pub fn purge_trash(&self, id: Option<&str>) -> Result<usize> {
        self.purge_where("id = ?1 OR ?1 IS NULL", id)
    }

    /// Items older than 30 days; run when the database opens.
    pub fn purge_expired_trash(&self) -> Result<usize> {
        self.purge_where("deleted_at < ?1", Some(&purge_cutoff()))
    }

    fn purge_where(&self, filter: &str, arg: Option<&str>) -> Result<usize> {
        let mut stmt = self.conn.prepare(&format!("SELECT id, payload FROM trash_item WHERE {filter}"))?;
        let items: Vec<(String, String)> =
            stmt.query_map([arg], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?;
        if items.is_empty() {
            return Ok(0);
        }
        let tx = self.conn.unchecked_transaction()?;
        for (id, _) in &items {
            tx.execute("DELETE FROM trash_item WHERE id = ?1", [id])?;
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "trash.purge", entity_type: "trash", entity_id: None, meta: json!({ "items": items.len() }) })?;
        tx.commit()?;
        for (_, payload) in &items {
            for dir in audio_dirs(payload) {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
        Ok(items.len())
    }

    /// Audio folders of a patient's trashed sessions, for the erasure (the rows go by cascade).
    pub(super) fn trash_audio_dirs(&self, patient_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT payload FROM trash_item WHERE patient_id = ?1 AND kind = 'session'")?;
        let payloads: Vec<String> = stmt.query_map([patient_id], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
        Ok(payloads.iter().flat_map(|p| audio_dirs(p)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit;

    fn setup() -> (Store, Patient, Episode) {
        let store = Store::open_in_memory().unwrap();
        let p = store
            .create_patient(PatientInput { first_name: "Anna".into(), last_name: "Beispiel".into(), ..Default::default() })
            .unwrap();
        let ep = store
            .create_episode(EpisodeInput { patient_id: p.id.clone(), method: Method::Vt, age_group: AgeGroup::Adult, setting: Setting::Einzel })
            .unwrap();
        (store, p, ep)
    }

    fn session(store: &Store, p: &Patient, ep: &Episode) -> Session {
        store
            .create_session(SessionInput {
                patient_id: p.id.clone(), episode_id: Some(ep.id.clone()), scheduled_at: Some("2026-10-20T08:00:00Z".into()),
                modality: None, planned_minutes: None,
            })
            .unwrap()
    }

    fn count(store: &Store, table: &str) -> i64 {
        store.conn().query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn signed_session_goes_to_the_trash_and_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let (store, p, ep) = setup();
        let s = session(&store, &p, &ep);
        let (src, mut w) = store.start_source(&s.id, SourceKind::Dictation, dir.path()).unwrap();
        w.write(&[1, 2, 3]).unwrap();
        store.finish_source(&src.id, "stopped", &w).unwrap();
        let note = store.create_session_note(&s.id).unwrap();
        store.save_section(&note.id, "verlauf", "<p>Verlauf</p>").unwrap();
        store.sign_report(&note.id, "Dr. Test").unwrap();
        let audio = dir.path().join(&src.id);
        assert!(audio.exists());

        // An unsigned session is deleted for good.
        let other = session(&store, &p, &ep);
        assert!(!store.delete_session(&other.id).unwrap().trashed);

        assert!(store.delete_session(&s.id).unwrap().trashed);
        assert!(store.session(&s.id).is_err() && store.report(&note.id).is_err(), "gone everywhere");
        assert!(audio.exists(), "audio stays until the purge");
        let items = store.list_trash().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].kind.as_str(), items[0].patient_name.as_str()), ("session", "Anna Beispiel"));
        assert_eq!(items[0].occurred_at.as_deref(), Some("2026-10-20T08:00:00Z"));

        store.restore_trash(&items[0].id).unwrap();
        let back = store.report(&note.id).unwrap();
        assert_eq!(back.status, ReportStatus::Signed);
        assert!(back.sections.iter().any(|s| s.body == "<p>Verlauf</p>"));
        assert_eq!(store.list_sources(&s.id).unwrap().len(), 1, "the recording is back");
        assert!(store.list_trash().unwrap().is_empty());

        // Purged by hand: the audio goes too.
        store.delete_session(&s.id).unwrap();
        assert_eq!(store.purge_trash(None).unwrap(), 1);
        assert!(!audio.exists());

        let events = audit::list(store.conn(), 50).unwrap();
        for action in ["session.delete", "session.restore", "trash.purge"] {
            assert!(events.iter().any(|e| e.action == action), "{action} audited");
        }
        assert!(events.iter().all(|e| !e.meta_json.contains("Verlauf")));
        assert_eq!(audit::verify(store.conn()).unwrap(), None);
    }

    #[test]
    fn signed_bericht_comes_back_with_its_antrag() {
        let (store, _p, ep) = setup();
        let draft = store.create_application(&ep.id, ApplicationKind::Erstantrag).unwrap().report_id.unwrap();
        assert!(!store.delete_report(&draft).unwrap().trashed, "a draft is deleted for good");

        let app = store.create_application(&ep.id, ApplicationKind::Umwandlung).unwrap();
        let bericht = app.report_id.clone().unwrap();
        let first = store.report(&bericht).unwrap().sections[0].key.clone();
        store.save_section(&bericht, &first, "<p>Plan</p>").unwrap();
        store.sign_report(&bericht, "Dr. Test").unwrap();
        assert!(store.delete_report(&bericht).unwrap().trashed);
        assert_eq!(count(&store, "application"), 0);
        let item = &store.list_trash().unwrap()[0];
        assert_eq!((item.kind.as_str(), item.detail.as_deref()), ("bericht", Some("umwandlung")));

        store.restore_trash(&item.id).unwrap();
        assert_eq!(store.report(&bericht).unwrap().status, ReportStatus::Signed);
        let report_id: String = store.conn().query_row("SELECT report_id FROM application WHERE id = ?1", [&app.id], |r| r.get(0)).unwrap();
        assert_eq!(report_id, bericht);
    }

    #[test]
    fn restore_fails_cleanly_when_the_case_is_gone() {
        let (store, _p, ep) = setup();
        let bericht = store.create_application(&ep.id, ApplicationKind::Erstantrag).unwrap().report_id.unwrap();
        store.sign_report(&bericht, "Dr. Test").unwrap();
        store.delete_report(&bericht).unwrap();
        store.conn().execute("DELETE FROM episode", []).unwrap();
        let item = store.list_trash().unwrap().remove(0);
        assert!(matches!(store.restore_trash(&item.id), Err(Error::Invalid(_))));
        assert_eq!(count(&store, "report"), 0, "nothing half-restored");
        assert_eq!(store.list_trash().unwrap().len(), 1, "the item stays");
    }

    #[test]
    fn old_items_are_purged_and_erasure_empties_the_trash() {
        let dir = tempfile::tempdir().unwrap();
        let (store, p, ep) = setup();
        for _ in 0..2 {
            let s = session(&store, &p, &ep);
            let (src, w) = store.start_source(&s.id, SourceKind::Dictation, dir.path()).unwrap();
            store.finish_source(&src.id, "stopped", &w).unwrap();
            let note = store.create_session_note(&s.id).unwrap();
            store.sign_report(&note.id, "Dr. Test").unwrap();
            store.delete_session(&s.id).unwrap();
        }
        store.conn().execute("UPDATE trash_item SET deleted_at = '2026-01-01T00:00:00Z' WHERE rowid = 1", []).unwrap();
        assert_eq!(store.purge_expired_trash().unwrap(), 1);
        assert_eq!(store.list_trash().unwrap().len(), 1);

        store.erase_patient(&p.id).unwrap();
        assert_eq!(count(&store, "trash_item"), 0);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "trashed audio erased too");
    }
}
