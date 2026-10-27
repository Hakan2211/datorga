//! SQLite (SQLCipher in production) persistence. Every mutation writes an audit event in the
//! same transaction.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use crate::audio::{self, AudioKey, ChunkWriter};
use crate::audit::{self, Entry};
use crate::consent;
use crate::crypto::MasterKey;
use crate::domain::*;
use crate::templates::{self, Template};
use crate::{Error, Result};

const ACTOR: &str = "therapist";

/// Ordered migrations; index + 1 = schema version (stored in `PRAGMA user_version`).
const MIGRATIONS: &[&str] = &[include_str!("migrations/V1__spine.sql"), include_str!("migrations/V2__capture.sql")];

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
    /// Audio directories are removed after the commit. If that fails, the leftover chunks are
    /// unreadable anyway: their keys went with the database rows.
    pub fn erase_patient(&self, id: &str) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT a.path FROM audio_asset a JOIN session_source src ON src.id = a.source_id
             JOIN session s ON s.id = src.session_id WHERE s.patient_id = ?1",
        )?;
        let audio_dirs: Vec<String> = stmt.query_map([id], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute("DELETE FROM patient WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(Error::NotFound(format!("patient {id}")));
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "patient.erase", entity_type: "patient", entity_id: Some(id), meta: json!({}) })?;
        tx.commit()?;
        for dir in audio_dirs {
            let _ = std::fs::remove_dir_all(dir);
        }
        Ok(())
    }

    // ---------- consent ----------

    /// Records consent against the current text version. Only one active consent per kind.
    pub fn grant_consent(&self, patient_id: &str, kind: ConsentKind, note: Option<&str>) -> Result<Consent> {
        self.patient(patient_id)?;
        if self.active_consent(patient_id, kind)?.is_some() {
            return Err(Error::Invalid("Einwilligung ist bereits erteilt".into()));
        }
        let text = consent::current(kind);
        let note = note.map(str::trim).filter(|n| !n.is_empty());
        let tx = self.conn.unchecked_transaction()?;
        let id = new_id();
        tx.execute(
            "INSERT INTO consent (id, patient_id, kind, text_version, text_hash, granted_at, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, patient_id, kind.as_str(), text.version, text.hash, now(), note],
        )?;
        sync_consent_flag(&tx, patient_id)?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "consent.grant", entity_type: "consent", entity_id: Some(&id),
            meta: json!({ "kind": kind.as_str(), "text_version": text.version, "text_hash": text.hash }),
        })?;
        tx.commit()?;
        self.consent(&id)
    }

    /// Revocation applies from now on; the record stays as evidence of the earlier grant.
    pub fn revoke_consent(&self, id: &str) -> Result<Consent> {
        let c = self.consent(id)?;
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute("UPDATE consent SET revoked_at = ?2 WHERE id = ?1 AND revoked_at IS NULL", params![id, now()])?;
        if n == 0 {
            return Err(Error::Invalid("Einwilligung ist bereits widerrufen".into()));
        }
        sync_consent_flag(&tx, &c.patient_id)?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "consent.revoke", entity_type: "consent", entity_id: Some(id),
            meta: json!({ "kind": c.kind.as_str() }),
        })?;
        tx.commit()?;
        self.consent(id)
    }

    pub fn consent(&self, id: &str) -> Result<Consent> {
        self.conn
            .query_row(&format!("{CONSENT_SELECT} WHERE id = ?1"), [id], row_to_consent)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("consent {id}")))
    }

    /// Newest first, revoked ones included (consent history).
    pub fn list_consents(&self, patient_id: &str) -> Result<Vec<Consent>> {
        let mut stmt = self.conn.prepare(&format!("{CONSENT_SELECT} WHERE patient_id = ?1 ORDER BY granted_at DESC"))?;
        let rows = stmt.query_map([patient_id], row_to_consent)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The consent a recording gets linked to; `None` means recording is not allowed.
    pub fn active_consent(&self, patient_id: &str, kind: ConsentKind) -> Result<Option<Consent>> {
        Ok(self
            .conn
            .query_row(
                &format!("{CONSENT_SELECT} WHERE patient_id = ?1 AND kind = ?2 AND revoked_at IS NULL"),
                params![patient_id, kind.as_str()],
                row_to_consent,
            )
            .optional()?)
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

    pub fn session(&self, id: &str) -> Result<Session> {
        self.conn
            .query_row(&format!("{SESSION_SELECT} WHERE id = ?1"), [id], row_to_session)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("session {id}")))
    }

    // ---------- capture ----------

    /// Opens a new source for `session_id` and returns the encrypted chunk writer for it.
    /// Session recordings are refused without an active recording consent (fail closed).
    pub fn start_source(&self, session_id: &str, kind: SourceKind, audio_root: &Path) -> Result<(SessionSource, ChunkWriter)> {
        let session = self.session(session_id)?;
        let consent_id = match kind {
            SourceKind::SessionAudio => Some(
                self.active_consent(&session.patient_id, ConsentKind::Recording)?
                    .ok_or_else(|| Error::Invalid("Keine Einwilligung zur Tonaufnahme".into()))?
                    .id,
            ),
            SourceKind::Dictation => None,
        };
        let busy: bool = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM session_source WHERE status = 'recording')",
            [],
            |r| r.get(0),
        )?;
        if busy {
            return Err(Error::Invalid("Es läuft bereits eine Aufnahme".into()));
        }

        let id = new_id();
        let dir: PathBuf = audio_root.join(&id);
        let key = AudioKey::generate()?;
        let writer = ChunkWriter::create(&dir, &key)?;
        let ts = now();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO session_source (id, session_id, kind, consent_id, started_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, session_id, kind.as_str(), consent_id, ts],
        )?;
        tx.execute(
            "INSERT INTO audio_asset (id, source_id, path, codec, dek) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![new_id(), id, dir.to_string_lossy(), audio::CODEC, key.to_hex()],
        )?;
        tx.execute(
            "UPDATE session SET started_at = COALESCE(started_at, ?2), status = 'in_progress',
                consent_id = COALESCE(?3, consent_id)
             WHERE id = ?1",
            params![session_id, ts, consent_id],
        )?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "capture.start", entity_type: "session_source", entity_id: Some(&id),
            meta: json!({ "kind": kind.as_str(), "session_id": session_id, "consent_id": consent_id }),
        })?;
        tx.commit()?;
        Ok((self.source(&id)?, writer))
    }

    /// Closes a source. `reason`: stopped | consent_revoked | device_error.
    pub fn finish_source(&self, source_id: &str, reason: &str, writer: &ChunkWriter) -> Result<SessionSource> {
        let duration_ms = (writer.samples() * 1000 / audio::SAMPLE_RATE as u64) as i64;
        self.close_source(source_id, SourceStatus::Recorded, reason, duration_ms, Some(writer.sha256()))
    }

    /// After a crash: sources still marked `recording` keep the chunks that made it to disk.
    pub fn recover_interrupted_sources(&self) -> Result<usize> {
        let mut stmt = self.conn.prepare(
            "SELECT src.id, a.path FROM session_source src JOIN audio_asset a ON a.source_id = src.id
             WHERE src.status = 'recording'",
        )?;
        let stale: Vec<(String, String)> =
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?;
        for (id, path) in &stale {
            let samples = audio::samples_on_disk(Path::new(path))?;
            self.close_source(id, SourceStatus::Interrupted, "crash", (samples * 1000 / audio::SAMPLE_RATE as u64) as i64, None)?;
        }
        Ok(stale.len())
    }

    fn close_source(&self, id: &str, status: SourceStatus, reason: &str, duration_ms: i64, sha256: Option<String>) -> Result<SessionSource> {
        let ts = now();
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute(
            "UPDATE session_source SET status = ?2, ended_at = ?3, end_reason = ?4 WHERE id = ?1 AND status = 'recording'",
            params![id, status.as_str(), ts, reason],
        )?;
        if n == 0 {
            return Err(Error::Invalid("Aufnahme läuft nicht".into()));
        }
        tx.execute("UPDATE audio_asset SET duration_ms = ?2, sha256 = ?3 WHERE source_id = ?1", params![id, duration_ms, sha256])?;
        tx.execute(
            "UPDATE session SET ended_at = ?2, status = 'recorded',
                duration_s = CAST(round((julianday(?2) - julianday(started_at)) * 86400) AS INTEGER)
             WHERE id = (SELECT session_id FROM session_source WHERE id = ?1)",
            params![id, ts],
        )?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "capture.stop", entity_type: "session_source", entity_id: Some(id),
            meta: json!({ "status": status.as_str(), "reason": reason, "duration_ms": duration_ms }),
        })?;
        tx.commit()?;
        self.source(id)
    }

    pub fn source(&self, id: &str) -> Result<SessionSource> {
        self.conn
            .query_row(&format!("{SOURCE_SELECT} WHERE src.id = ?1"), [id], row_to_source)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("source {id}")))
    }

    pub fn list_sources(&self, session_id: &str) -> Result<Vec<SessionSource>> {
        let mut stmt = self.conn.prepare(&format!("{SOURCE_SELECT} WHERE src.session_id = ?1 ORDER BY src.started_at"))?;
        let rows = stmt.query_map([session_id], row_to_source)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Directory and key of a source's audio, for decryption (transcription).
    pub fn audio_location(&self, source_id: &str) -> Result<(PathBuf, AudioKey)> {
        let (path, dek): (String, String) = self
            .conn
            .query_row(
                "SELECT path, dek FROM audio_asset WHERE source_id = ?1 AND deleted_at IS NULL",
                [source_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("audio for source {source_id}")))?;
        Ok((PathBuf::from(path), AudioKey::from_hex(&dek)?))
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
const CONSENT_SELECT: &str = "SELECT id, patient_id, kind, scope, text_version, text_hash, granted_at, revoked_at,
    note FROM consent";
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

const SOURCE_SELECT: &str = "SELECT src.id, src.session_id, src.kind, src.consent_id, src.status, src.started_at,
    src.ended_at, src.end_reason, a.duration_ms, a.deleted_at
    FROM session_source src LEFT JOIN audio_asset a ON a.source_id = src.id";

fn row_to_source(r: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSource> {
    Ok(SessionSource {
        id: r.get(0)?,
        session_id: r.get(1)?,
        kind: parse_col(r, 2)?,
        consent_id: r.get(3)?,
        status: parse_col(r, 4)?,
        started_at: r.get(5)?,
        ended_at: r.get(6)?,
        end_reason: r.get(7)?,
        duration_ms: r.get(8)?,
        audio_deleted_at: r.get(9)?,
    })
}

/// `patient.consent_recording` is derived: true while an unrevoked recording consent exists.
fn sync_consent_flag(conn: &Connection, patient_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE patient SET consent_recording = EXISTS (
             SELECT 1 FROM consent WHERE patient_id = ?1 AND kind = 'recording' AND revoked_at IS NULL)
         WHERE id = ?1",
        [patient_id],
    )?;
    Ok(())
}

fn row_to_consent(r: &rusqlite::Row<'_>) -> rusqlite::Result<Consent> {
    Ok(Consent {
        id: r.get(0)?,
        patient_id: r.get(1)?,
        kind: parse_col(r, 2)?,
        scope: r.get(3)?,
        text_version: r.get(4)?,
        text_hash: r.get(5)?,
        granted_at: r.get(6)?,
        revoked_at: r.get(7)?,
        note: r.get(8)?,
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
    fn consent_grant_and_revoke_drive_recording_flag() {
        let store = Store::open_in_memory().unwrap();
        let p = patient(&store);
        assert!(!p.consent_recording);

        let c = store.grant_consent(&p.id, ConsentKind::Recording, Some("  Papierformular unterschrieben ")).unwrap();
        assert_eq!(c.text_hash, consent::current(ConsentKind::Recording).hash);
        assert_eq!(c.note.as_deref(), Some("Papierformular unterschrieben"));
        assert!(store.patient(&p.id).unwrap().consent_recording);
        assert!(store.grant_consent(&p.id, ConsentKind::Recording, None).is_err(), "one active consent per kind");

        store.revoke_consent(&c.id).unwrap();
        assert!(!store.patient(&p.id).unwrap().consent_recording);
        assert!(store.active_consent(&p.id, ConsentKind::Recording).unwrap().is_none());
        assert!(store.revoke_consent(&c.id).is_err(), "double revoke");

        // Re-granting after a revocation is a new record; history keeps both.
        store.grant_consent(&p.id, ConsentKind::Recording, None).unwrap();
        assert_eq!(store.list_consents(&p.id).unwrap().len(), 2);
        assert!(store.patient(&p.id).unwrap().consent_recording);

        let events = audit::list(store.conn(), 50).unwrap();
        assert_eq!(events.iter().filter(|e| e.action.starts_with("consent.")).count(), 3);
        assert!(events.iter().all(|e| !e.meta_json.contains("Papierformular")), "free-text note stays out of the audit log");
        assert_eq!(audit::verify(store.conn()).unwrap(), None);
    }

    fn session(store: &Store, p: &Patient) -> Session {
        store
            .create_session(SessionInput { patient_id: p.id.clone(), episode_id: None, scheduled_at: None, modality: None })
            .unwrap()
    }

    #[test]
    fn session_recording_requires_consent_dictation_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let p = patient(&store);
        let s = session(&store, &p);

        let err = store.start_source(&s.id, SourceKind::SessionAudio, dir.path()).err().unwrap();
        assert!(err.to_string().contains("Einwilligung"), "{err}");

        let (src, mut w) = store.start_source(&s.id, SourceKind::Dictation, dir.path()).unwrap();
        assert!(src.consent_id.is_none());
        assert!(store.start_source(&s.id, SourceKind::Dictation, dir.path()).is_err(), "one recording at a time");
        w.write(&vec![100i16; audio::CHUNK_SAMPLES]).unwrap();
        w.write(&vec![-100i16; 8_000]).unwrap();
        let done = store.finish_source(&src.id, "stopped", &w).unwrap();
        assert_eq!((done.status, done.duration_ms), (SourceStatus::Recorded, Some(5_500)));
        assert_eq!(store.session(&s.id).unwrap().status, "recorded");

        let c = store.grant_consent(&p.id, ConsentKind::Recording, None).unwrap();
        let (rec, w) = store.start_source(&s.id, SourceKind::SessionAudio, dir.path()).unwrap();
        assert_eq!(rec.consent_id.as_deref(), Some(c.id.as_str()));
        store.finish_source(&rec.id, "consent_revoked", &w).unwrap();
        assert_eq!(store.list_sources(&s.id).unwrap().len(), 2);

        let (path, key) = store.audio_location(&src.id).unwrap();
        assert_eq!(audio::read_pcm(&path, &key).unwrap().len(), audio::CHUNK_SAMPLES + 8_000);
        assert_eq!(audit::verify(store.conn()).unwrap(), None);
    }

    #[test]
    fn crash_leaves_recoverable_interrupted_source_and_erasure_removes_audio() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let p = patient(&store);
        let s = session(&store, &p);
        let (src, mut w) = store.start_source(&s.id, SourceKind::Dictation, dir.path()).unwrap();
        w.write(&vec![1i16; audio::CHUNK_SAMPLES]).unwrap();
        drop(w); // app dies mid-recording

        assert_eq!(store.recover_interrupted_sources().unwrap(), 1);
        let src = store.source(&src.id).unwrap();
        assert_eq!((src.status, src.end_reason.as_deref(), src.duration_ms), (SourceStatus::Interrupted, Some("crash"), Some(5_000)));
        assert_eq!(store.recover_interrupted_sources().unwrap(), 0);

        let (path, _) = store.audio_location(&src.id).unwrap();
        assert!(path.exists());
        store.erase_patient(&p.id).unwrap();
        assert!(!path.exists(), "erasure removes the audio directory");
        assert!(store.source(&src.id).is_err());
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
