//! SQLite (SQLCipher in production) persistence. Every mutation writes an audit event in the
//! same transaction.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use crate::audit::{self, Entry};
use crate::crypto::MasterKey;
use crate::domain::*;
use crate::templates::{self, Template};
use crate::{Error, Result};

const ACTOR: &str = "therapist";

/// Ordered migrations; index + 1 = schema version (stored in `PRAGMA user_version`).
const MIGRATIONS: &[&str] = &[include_str!("migrations/V1__spine.sql")];

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating if needed) the database. With the `sqlcipher` feature the key is
    /// mandatory and the file is unreadable without it.
    pub fn open(path: &Path, key: &MasterKey) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn, key)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn, &MasterKey::from_bytes([7; 32]))
    }

    fn init(conn: Connection, key: &MasterKey) -> Result<Self> {
        #[cfg(feature = "sqlcipher")]
        {
            conn.pragma_update(None, "key", key.sqlcipher_pragma())?;
            // Fails with "file is not a database" when the key is wrong.
            conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))
                .map_err(|_| Error::Crypto("database key rejected".into()))?;
        }
        #[cfg(not(feature = "sqlcipher"))]
        let _ = key;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let store = Self { conn };
        store.migrate()?;
        store.seed_builtin_templates()?;
        Ok(store)
    }

    pub fn is_encrypted() -> bool {
        cfg!(feature = "sqlcipher")
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    fn migrate(&self) -> Result<()> {
        let current: usize = self.conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? as usize;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = self.conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(())
    }

    fn seed_builtin_templates(&self) -> Result<()> {
        for t in templates::builtin()? {
            self.conn.execute(
                "INSERT OR IGNORE INTO template (id, key, version, origin, definition, created_at)
                 VALUES (?1, ?2, ?3, 'builtin', ?4, ?5)",
                params![new_id(), t.key, t.version, serde_json::to_string(&t)?, now()],
            )?;
        }
        Ok(())
    }

    // ---------- templates ----------

    pub fn template(&self, key: &str) -> Result<Template> {
        let def: String = self
            .conn
            .query_row(
                "SELECT definition FROM template WHERE key = ?1 AND active = 1 ORDER BY created_at DESC LIMIT 1",
                [key],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("template {key}")))?;
        Ok(serde_json::from_str(&def)?)
    }

    pub fn list_templates(&self) -> Result<Vec<Template>> {
        let mut stmt = self.conn.prepare("SELECT definition FROM template WHERE active = 1 ORDER BY key")?;
        let defs = stmt.query_map([], |r| r.get::<_, String>(0))?;
        defs.map(|d| Ok(serde_json::from_str(&d?)?)).collect()
    }

    // ---------- patients ----------

    pub fn create_patient(&self, input: PatientInput) -> Result<Patient> {
        validate_patient(&input)?;
        let tx = self.conn.unchecked_transaction()?;
        let id = new_id();
        tx.execute(
            "INSERT INTO patient (id, external_ref, first_name, last_name, birth_date, chiffre, pronouns, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id, input.external_ref, input.first_name.trim(), input.last_name.trim(),
                input.birth_date, input.chiffre, input.pronouns, now()
            ],
        )?;
        audit::append(&tx, Entry { actor: ACTOR, action: "patient.create", entity_type: "patient", entity_id: Some(&id), meta: json!({}) })?;
        tx.commit()?;
        self.patient(&id)
    }

    pub fn update_patient(&self, id: &str, input: PatientInput) -> Result<Patient> {
        validate_patient(&input)?;
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute(
            "UPDATE patient SET external_ref=?2, first_name=?3, last_name=?4, birth_date=?5, chiffre=?6, pronouns=?7
             WHERE id=?1",
            params![
                id, input.external_ref, input.first_name.trim(), input.last_name.trim(),
                input.birth_date, input.chiffre, input.pronouns
            ],
        )?;
        if n == 0 {
            return Err(Error::NotFound(format!("patient {id}")));
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "patient.update", entity_type: "patient", entity_id: Some(id), meta: json!({}) })?;
        tx.commit()?;
        self.patient(id)
    }

    pub fn patient(&self, id: &str) -> Result<Patient> {
        self.conn
            .query_row(&format!("{PATIENT_SELECT} WHERE id = ?1"), [id], row_to_patient)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("patient {id}")))
    }

    pub fn list_patients(&self, query: Option<&str>) -> Result<Vec<Patient>> {
        let like = format!("%{}%", query.unwrap_or("").trim());
        let mut stmt = self.conn.prepare(&format!(
            "{PATIENT_SELECT} WHERE archived_at IS NULL
               AND (first_name || ' ' || last_name LIKE ?1 OR IFNULL(chiffre,'') LIKE ?1 OR IFNULL(external_ref,'') LIKE ?1)
             ORDER BY last_name, first_name"
        ))?;
        let rows = stmt.query_map([like], row_to_patient)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Art. 17 GDPR: hard delete with cascade. The audit entry keeps only the ID.
    pub fn erase_patient(&self, id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute("DELETE FROM patient WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(Error::NotFound(format!("patient {id}")));
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "patient.erase", entity_type: "patient", entity_id: Some(id), meta: json!({}) })?;
        tx.commit()?;
        Ok(())
    }

    // ---------- episodes & applications ----------

    pub fn create_episode(&self, input: EpisodeInput) -> Result<Episode> {
        self.patient(&input.patient_id)?;
        let tx = self.conn.unchecked_transaction()?;
        let id = new_id();
        tx.execute(
            "INSERT INTO episode (id, patient_id, method, age_group, setting, started_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, input.patient_id, input.method.as_str(), input.age_group.as_str(), input.setting.as_str(), now()],
        )?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "episode.create", entity_type: "episode", entity_id: Some(&id),
            meta: json!({ "method": input.method.as_str(), "age_group": input.age_group.as_str() }),
        })?;
        tx.commit()?;
        self.episode(&id)
    }

    pub fn episode(&self, id: &str) -> Result<Episode> {
        self.conn
            .query_row(&format!("{EPISODE_SELECT} WHERE id = ?1"), [id], row_to_episode)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("episode {id}")))
    }

    pub fn list_episodes(&self, patient_id: &str) -> Result<Vec<Episode>> {
        let mut stmt = self.conn.prepare(&format!("{EPISODE_SELECT} WHERE patient_id = ?1 ORDER BY started_at DESC"))?;
        let rows = stmt.query_map([patient_id], row_to_episode)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn list_applications(&self, episode_id: &str) -> Result<Vec<Application>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, episode_id, kind, seq, status, report_id, created_at FROM application
             WHERE episode_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map([episode_id], row_to_application)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Creates the Antrag and its Bericht draft, with one empty section per applicable template section.
    pub fn create_application(&self, episode_id: &str, kind: ApplicationKind) -> Result<Application> {
        let episode = self.episode(episode_id)?;
        let template = self.template(templates::template_key_for(kind))?;
        let tx = self.conn.unchecked_transaction()?;

        // Fortführungsberichte are numbered consecutively per episode (PTV 3).
        let seq: i64 = if kind == ApplicationKind::Fortfuehrung {
            tx.query_row(
                "SELECT COUNT(*) + 1 FROM application WHERE episode_id = ?1 AND kind = 'fortfuehrung'",
                [episode_id],
                |r| r.get(0),
            )?
        } else {
            1
        };

        let app_id = new_id();
        let report_id = new_id();
        let ts = now();
        tx.execute(
            "INSERT INTO application (id, episode_id, kind, seq, report_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![app_id, episode_id, kind.as_str(), seq, report_id, ts],
        )?;
        tx.execute(
            "INSERT INTO report (id, patient_id, episode_id, application_id, template_key, template_version, kind, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'bericht', ?7, ?7)",
            params![report_id, episode.patient_id, episode_id, app_id, template.key, template.version, ts],
        )?;
        let ctx = templates::Context { method: episode.method, age_group: episode.age_group, kind };
        for s in template.resolve(&ctx).sections {
            tx.execute(
                "INSERT INTO report_section (id, report_id, key) VALUES (?1, ?2, ?3)",
                params![new_id(), report_id, s.key],
            )?;
        }
        audit::append(&tx, Entry {
            actor: ACTOR, action: "application.create", entity_type: "application", entity_id: Some(&app_id),
            meta: json!({ "kind": kind.as_str(), "seq": seq, "report_id": report_id }),
        })?;
        tx.commit()?;
        self.conn
            .query_row(
                "SELECT id, episode_id, kind, seq, status, report_id, created_at FROM application WHERE id = ?1",
                [&app_id],
                row_to_application,
            )
            .map_err(Into::into)
    }

    // ---------- sessions ----------

    pub fn create_session(&self, input: SessionInput) -> Result<Session> {
        self.patient(&input.patient_id)?;
        let tx = self.conn.unchecked_transaction()?;
        let id = new_id();
        tx.execute(
            "INSERT INTO session (id, patient_id, episode_id, scheduled_at, modality) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, input.patient_id, input.episode_id, input.scheduled_at, input.modality],
        )?;
        audit::append(&tx, Entry { actor: ACTOR, action: "session.create", entity_type: "session", entity_id: Some(&id), meta: json!({}) })?;
        tx.commit()?;
        let mut stmt = self.conn.prepare(&format!("{SESSION_SELECT} WHERE id = ?1"))?;
        Ok(stmt.query_row([&id], row_to_session)?)
    }

    pub fn list_sessions(&self, patient_id: Option<&str>) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(&format!(
            "{SESSION_SELECT} WHERE (?1 IS NULL OR patient_id = ?1) ORDER BY COALESCE(scheduled_at, started_at) DESC"
        ))?;
        let rows = stmt.query_map([patient_id], row_to_session)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    // ---------- reports ----------

    pub fn report(&self, id: &str) -> Result<Report> {
        let mut report = self
            .conn
            .query_row(
                "SELECT id, patient_id, episode_id, application_id, template_key, template_version, kind, status,
                        created_at, updated_at, signed_at, signed_by
                 FROM report WHERE id = ?1",
                [id],
                |r| {
                    Ok(Report {
                        id: r.get(0)?,
                        patient_id: r.get(1)?,
                        episode_id: r.get(2)?,
                        application_id: r.get(3)?,
                        template_key: r.get(4)?,
                        template_version: r.get(5)?,
                        kind: r.get(6)?,
                        status: parse_col(r, 7)?,
                        created_at: r.get(8)?,
                        updated_at: r.get(9)?,
                        signed_at: r.get(10)?,
                        signed_by: r.get(11)?,
                        sections: vec![],
                    })
                },
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("report {id}")))?;
        let mut stmt = self.conn.prepare("SELECT key, body, origin, edited_at FROM report_section WHERE report_id = ?1")?;
        report.sections = stmt
            .query_map([id], |r| {
                Ok(ReportSection { key: r.get(0)?, body: r.get(1)?, origin: parse_col(r, 2)?, edited_at: r.get(3)? })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(report)
    }

    pub fn list_reports(&self, status: Option<ReportStatus>) -> Result<Vec<ReportSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.patient_id, p.first_name || ' ' || p.last_name, p.chiffre, e.method, a.kind, a.seq,
                    r.status, r.updated_at
             FROM report r
             JOIN patient p ON p.id = r.patient_id
             LEFT JOIN episode e ON e.id = r.episode_id
             LEFT JOIN application a ON a.id = r.application_id
             WHERE (?1 IS NULL OR r.status = ?1)
             ORDER BY r.updated_at DESC",
        )?;
        let rows = stmt.query_map([status.map(|s| s.as_str())], |r| {
            let opt = |i: usize| r.get::<_, Option<String>>(i);
            Ok(ReportSummary {
                id: r.get(0)?,
                patient_id: r.get(1)?,
                patient_name: r.get(2)?,
                chiffre: r.get(3)?,
                method: opt(4)?.and_then(|s| s.parse().ok()),
                kind: opt(5)?.and_then(|s| s.parse().ok()),
                seq: r.get(6)?,
                status: parse_col(r, 7)?,
                updated_at: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn save_section(&self, report_id: &str, key: &str, body: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let status: String = tx.query_row("SELECT status FROM report WHERE id = ?1", [report_id], |r| r.get(0))?;
        if status == ReportStatus::Signed.as_str() || status == ReportStatus::Exported.as_str() {
            return Err(Error::Invalid("signed reports are read-only".into()));
        }
        let ts = now();
        // AI-drafted text that the therapist edits becomes "mixed".
        let n = tx.execute(
            "UPDATE report_section SET body = ?3, edited_at = ?4,
                origin = CASE origin WHEN 'ai' THEN 'mixed' ELSE origin END
             WHERE report_id = ?1 AND key = ?2",
            params![report_id, key, body, ts],
        )?;
        if n == 0 {
            return Err(Error::NotFound(format!("section {key}")));
        }
        tx.execute("UPDATE report SET updated_at = ?2 WHERE id = ?1", params![report_id, ts])?;
        tx.commit()?;
        Ok(())
    }

    /// Sign-off. Content edits are blocked afterwards. The caller runs the checklist first.
    pub fn sign_report(&self, report_id: &str, signed_by: &str) -> Result<Report> {
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute(
            "UPDATE report SET status = 'signed', signed_at = ?2, signed_by = ?3, updated_at = ?2
             WHERE id = ?1 AND status IN ('draft', 'in_review')",
            params![report_id, now(), signed_by],
        )?;
        if n == 0 {
            return Err(Error::Invalid("report is not signable".into()));
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "report.sign", entity_type: "report", entity_id: Some(report_id), meta: json!({}) })?;
        tx.commit()?;
        self.report(report_id)
    }

    pub fn mark_exported(&self, report_id: &str, format: &str, sha256: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("UPDATE report SET status = 'exported' WHERE id = ?1 AND status = 'signed'", [report_id])?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "report.export", entity_type: "report", entity_id: Some(report_id),
            meta: json!({ "format": format, "sha256": sha256 }),
        })?;
        tx.commit()?;
        Ok(())
    }

    // ---------- settings ----------

    pub fn setting(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let v: Option<String> = self
            .conn
            .query_row("SELECT value_json FROM app_setting WHERE key = ?1", [key], |r| r.get(0))
            .optional()?;
        v.map(|s| serde_json::from_str(&s).map_err(Into::into)).transpose()
    }

    pub fn set_setting(&self, key: &str, value: &serde_json::Value) -> Result<()> {
        self.conn.execute(
            "INSERT INTO app_setting (key, value_json) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            params![key, value.to_string()],
        )?;
        Ok(())
    }
}

fn validate_patient(p: &PatientInput) -> Result<()> {
    if p.first_name.trim().is_empty() || p.last_name.trim().is_empty() {
        return Err(Error::Invalid("Vor- und Nachname sind erforderlich".into()));
    }
    if let Some(d) = &p.birth_date {
        let fmt = time::macros::format_description!("[year]-[month]-[day]");
        time::Date::parse(d, &fmt).map_err(|_| Error::Invalid("Geburtsdatum muss JJJJ-MM-TT sein".into()))?;
    }
    Ok(())
}

const PATIENT_SELECT: &str = "SELECT id, external_ref, first_name, last_name, birth_date, chiffre, pronouns,
    consent_recording, status, created_at, archived_at FROM patient";
const EPISODE_SELECT: &str =
    "SELECT id, patient_id, method, age_group, setting, stage, started_at, ended_at FROM episode";
const SESSION_SELECT: &str = "SELECT id, patient_id, episode_id, scheduled_at, started_at, ended_at, duration_s,
    modality, status FROM session";

fn row_to_patient(r: &rusqlite::Row<'_>) -> rusqlite::Result<Patient> {
    Ok(Patient {
        id: r.get(0)?,
        external_ref: r.get(1)?,
        first_name: r.get(2)?,
        last_name: r.get(3)?,
        birth_date: r.get(4)?,
        chiffre: r.get(5)?,
        pronouns: r.get(6)?,
        consent_recording: r.get(7)?,
        status: r.get(8)?,
        created_at: r.get(9)?,
        archived_at: r.get(10)?,
    })
}

fn row_to_episode(r: &rusqlite::Row<'_>) -> rusqlite::Result<Episode> {
    Ok(Episode {
        id: r.get(0)?,
        patient_id: r.get(1)?,
        method: parse_col(r, 2)?,
        age_group: parse_col(r, 3)?,
        setting: parse_col(r, 4)?,
        stage: parse_col(r, 5)?,
        started_at: r.get(6)?,
        ended_at: r.get(7)?,
    })
}

fn row_to_application(r: &rusqlite::Row<'_>) -> rusqlite::Result<Application> {
    Ok(Application {
        id: r.get(0)?,
        episode_id: r.get(1)?,
        kind: parse_col(r, 2)?,
        seq: r.get(3)?,
        status: r.get(4)?,
        report_id: r.get(5)?,
        created_at: r.get(6)?,
    })
}

fn row_to_session(r: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        patient_id: r.get(1)?,
        episode_id: r.get(2)?,
        scheduled_at: r.get(3)?,
        started_at: r.get(4)?,
        ended_at: r.get(5)?,
        duration_s: r.get(6)?,
        modality: r.get(7)?,
        status: r.get(8)?,
    })
}

fn parse_col<T: std::str::FromStr<Err = Error>>(r: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<T> {
    let s: String = r.get(idx)?;
    s.parse().map_err(|e: Error| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patient(store: &Store) -> Patient {
        store
            .create_patient(PatientInput {
                first_name: "Anna".into(),
                last_name: "Beispiel".into(),
                birth_date: Some("1985-03-14".into()),
                chiffre: Some("B140385".into()),
                ..Default::default()
            })
            .unwrap()
    }

    #[test]
    fn application_creates_filtered_report_sections_and_numbers_fortfuehrung() {
        let store = Store::open_in_memory().unwrap();
        let p = patient(&store);
        let ep = store
            .create_episode(EpisodeInput {
                patient_id: p.id.clone(),
                method: Method::Vt,
                age_group: AgeGroup::Adult,
                setting: Setting::Einzel,
            })
            .unwrap();

        let erst = store.create_application(&ep.id, ApplicationKind::Erstantrag).unwrap();
        let r = store.report(erst.report_id.as_deref().unwrap()).unwrap();
        assert_eq!(r.sections.len(), 6, "Erstantrag has sections 1-6, no §7");

        let umw = store.create_application(&ep.id, ApplicationKind::Umwandlung).unwrap();
        assert_eq!(store.report(umw.report_id.as_deref().unwrap()).unwrap().sections.len(), 7);

        let f1 = store.create_application(&ep.id, ApplicationKind::Fortfuehrung).unwrap();
        let f2 = store.create_application(&ep.id, ApplicationKind::Fortfuehrung).unwrap();
        assert_eq!((f1.seq, f2.seq), (1, 2));
        assert_eq!(store.report(f2.report_id.as_deref().unwrap()).unwrap().sections.len(), 3);
        let all = store.list_reports(Some(ReportStatus::Draft)).unwrap();
        assert_eq!(all.len(), 4);
        assert!(all.iter().any(|r| r.kind == Some(ApplicationKind::Fortfuehrung) && r.seq == Some(2)));
    }

    #[test]
    fn signed_report_is_read_only_and_erasure_keeps_audit_chain() {
        let store = Store::open_in_memory().unwrap();
        let p = patient(&store);
        let ep = store
            .create_episode(EpisodeInput { patient_id: p.id.clone(), method: Method::Tp, age_group: AgeGroup::Kjp, setting: Setting::Einzel })
            .unwrap();
        let app = store.create_application(&ep.id, ApplicationKind::Erstantrag).unwrap();
        let rid = app.report_id.unwrap();
        store.save_section(&rid, "soziodemographie", "<p>Schülerin, 9. Klasse</p>").unwrap();
        store.sign_report(&rid, "Dipl.-Psych. Test").unwrap();
        assert!(store.save_section(&rid, "soziodemographie", "x").is_err());

        store.erase_patient(&p.id).unwrap();
        assert!(store.report(&rid).is_err(), "cascade removes the report");
        assert_eq!(audit::verify(store.conn()).unwrap(), None);
    }

    #[test]
    fn migrations_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let key = MasterKey::generate().unwrap();
        Store::open(&path, &key).unwrap();
        let s = Store::open(&path, &key).unwrap();
        let v: i64 = s.conn().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());
    }

    #[cfg(feature = "sqlcipher")]
    #[test]
    fn database_is_unreadable_without_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let key = MasterKey::generate().unwrap();
        patient(&Store::open(&path, &key).unwrap());
        assert!(Store::open(&path, &MasterKey::generate().unwrap()).is_err());
        let raw = Connection::open(&path).unwrap();
        assert!(raw.query_row("SELECT count(*) FROM patient", [], |r| r.get::<_, i64>(0)).is_err());
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.windows(8).any(|w| w == b"Beispiel"), "plaintext must not appear on disk");
    }
}
