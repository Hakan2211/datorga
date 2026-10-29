//! Calendar and day planning (slice 22): sessions in a time range, rescheduling, cancelling,
//! series, to-dos, the day's planning note and the derived "Fällig" items.

use rusqlite::{params, OptionalExtension};
use serde_json::json;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::{row_to_session, Store, ACTOR, SESSION_SELECT};
use crate::audit::{self, Entry};
use crate::domain::*;
use crate::{Error, Result};

/// Longest series "Serie anlegen" creates (a year of weekly sessions).
pub const MAX_SERIES: u32 = 52;

/// RFC 3339 in UTC without fractions, the one format stored in `session.scheduled_at`, so range
/// queries can compare strings (and use the index).
pub fn normalize_ts(s: &str) -> Result<String> {
    let t = OffsetDateTime::parse(s, &Rfc3339).map_err(|_| Error::Invalid(format!("Zeitpunkt ungültig: {s}")))?;
    format_utc(t)
}

fn format_utc(t: OffsetDateTime) -> Result<String> {
    let t = t.to_offset(time::UtcOffset::UTC).replace_nanosecond(0).expect("0 is a valid nanosecond");
    t.format(&Rfc3339).map_err(|e| Error::Invalid(e.to_string()))
}

pub(super) fn check_minutes(m: Option<i64>) -> Result<()> {
    match m {
        Some(m) if !(5..=480).contains(&m) => Err(Error::Invalid("Dauer muss zwischen 5 und 480 Minuten liegen".into())),
        _ => Ok(()),
    }
}

pub(super) fn check_date(d: &str) -> Result<()> {
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    time::Date::parse(d, &fmt).map(|_| ()).map_err(|_| Error::Invalid(format!("Datum muss JJJJ-MM-TT sein: {d}")))
}

pub(super) fn check_task(input: &TaskInput) -> Result<()> {
    check_date(&input.date)?;
    if let Some(t) = &input.time {
        let fmt = time::macros::format_description!("[hour]:[minute]");
        time::Time::parse(t, &fmt).map_err(|_| Error::Invalid(format!("Uhrzeit muss HH:MM sein: {t}")))?;
    }
    if input.title.trim().is_empty() {
        return Err(Error::Invalid("Aufgabe ohne Titel".into()));
    }
    Ok(())
}

const CALENDAR_SELECT: &str = "SELECT p.first_name || ' ' || p.last_name, p.chiffre,
    (SELECT count(*) FROM session o WHERE o.patient_id = s.patient_id AND o.status != 'cancelled'
        AND (COALESCE(o.scheduled_at, o.started_at), o.id) <= (COALESCE(s.scheduled_at, s.started_at), s.id)),
    EXISTS (SELECT 1 FROM session_prep sp WHERE sp.session_id = s.id),
    (SELECT count(*) FROM session_source src WHERE src.session_id = s.id AND src.status != 'recording'),
    (SELECT count(*) FROM session_source src LEFT JOIN audio_asset a ON a.source_id = src.id
        WHERE src.session_id = s.id AND src.status != 'recording' AND a.deleted_at IS NULL
          AND NOT EXISTS (SELECT 1 FROM transcript t WHERE t.source_id = src.id AND t.status = 'confirmed')),
    EXISTS (SELECT 1 FROM session_source src WHERE src.session_id = s.id AND src.status = 'recording')";

const TASK_SELECT: &str = "SELECT id, date, time, title, done, patient_id, session_id, created_at FROM task";

fn row_to_task(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        date: r.get(1)?,
        time: r.get(2)?,
        title: r.get(3)?,
        done: r.get(4)?,
        patient_id: r.get(5)?,
        session_id: r.get(6)?,
        created_at: r.get(7)?,
    })
}

impl Store {
    // ---------- sessions in the calendar ----------

    /// Sessions whose time (planned, else started) lies in `[from, to)`, oldest first.
    pub fn sessions_between(&self, from: &str, to: &str) -> Result<Vec<CalendarSession>> {
        let (from, to) = (normalize_ts(from)?, normalize_ts(to)?);
        // SESSION_SELECT ends in "FROM session s LEFT JOIN report n …": the calendar columns go
        // after its select list, so splice them in before its FROM.
        let (cols, from_clause) = SESSION_SELECT.split_once("FROM session s").expect("SESSION_SELECT has a FROM");
        let extra = CALENDAR_SELECT.trim_start_matches("SELECT ");
        let sql = format!(
            "{cols}, {extra} FROM session s {from_clause}
             JOIN patient p ON p.id = s.patient_id
             WHERE COALESCE(s.scheduled_at, s.started_at) >= ?1 AND COALESCE(s.scheduled_at, s.started_at) < ?2
             ORDER BY COALESCE(s.scheduled_at, s.started_at), s.id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![from, to], |r| {
            Ok(CalendarSession {
                session: row_to_session(r)?,
                patient_name: r.get(14)?,
                chiffre: r.get(15)?,
                number: r.get(16)?,
                has_prep: r.get(17)?,
                sources_finished: r.get(18)?,
                sources_unreviewed: r.get(19)?,
                recording: r.get(20)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Recordings or a note mean the session took place: it can't be cancelled or deleted then
    /// (and the assistant doesn't move it).
    pub(super) fn session_untouched(&self, id: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT NOT EXISTS (SELECT 1 FROM session_source WHERE session_id = ?1)
                AND NOT EXISTS (SELECT 1 FROM report WHERE session_id = ?1)",
            [id],
            |r| r.get(0),
        )?)
    }

    fn require_untouched(&self, id: &str) -> Result<Session> {
        let session = self.session(id)?;
        if !self.session_untouched(id)? {
            return Err(Error::Invalid("Die Sitzung hat schon Aufnahmen oder eine Notiz und bleibt, wie sie ist".into()));
        }
        Ok(session)
    }

    /// A session can still be moved until its note is signed (the note's date comes from the
    /// session, and a signed note must not change), and not while it is being recorded. A
    /// recording or an unsigned note doesn't hold it: she may have recorded into the wrong day.
    pub(super) fn session_movable(&self, id: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT NOT EXISTS (SELECT 1 FROM session_source WHERE session_id = ?1 AND status = 'recording')
                AND NOT EXISTS (SELECT 1 FROM report WHERE session_id = ?1 AND status IN ('signed', 'exported'))",
            [id],
            |r| r.get(0),
        )?)
    }

    /// Reschedule, change the length or the modality of a session whose note isn't signed yet.
    pub fn update_session(&self, id: &str, update: SessionUpdate) -> Result<Session> {
        let before = self.session(id)?;
        if !self.session_movable(id)? {
            return Err(Error::Invalid("Die Sitzungsnotiz ist unterschrieben (oder die Aufnahme läuft): die Sitzung bleibt, wie sie ist".into()));
        }
        let documented = !self.session_untouched(id)?;
        let scheduled_at = normalize_ts(&update.scheduled_at)?;
        check_minutes(update.planned_minutes)?;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE session SET scheduled_at = ?2, planned_minutes = ?3, modality = ?4 WHERE id = ?1",
            params![id, scheduled_at, update.planned_minutes, update.modality],
        )?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "session.update", entity_type: "session", entity_id: Some(id),
            meta: json!({
                "moved": before.scheduled_at.as_deref() != Some(scheduled_at.as_str()),
                "documented": documented,
                "planned_minutes": update.planned_minutes,
                "modality": update.modality,
            }),
        })?;
        tx.commit()?;
        self.session(id)
    }

    /// Marks a session that hasn't started as cancelled ("Ausfall", "abgesagt"). The reason is
    /// free text: it stays in the session row and out of the audit log.
    pub fn cancel_session(&self, id: &str, reason: Option<&str>) -> Result<Session> {
        self.require_untouched(id)?;
        let reason = reason.map(str::trim).filter(|r| !r.is_empty());
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("UPDATE session SET status = 'cancelled', cancelled_reason = ?2 WHERE id = ?1", params![id, reason])?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "session.cancel", entity_type: "session", entity_id: Some(id),
            meta: json!({ "reason": reason.is_some() }),
        })?;
        tx.commit()?;
        self.session(id)
    }

    pub fn reopen_session(&self, id: &str) -> Result<Session> {
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute(
            "UPDATE session SET status = 'planned', cancelled_reason = NULL WHERE id = ?1 AND status = 'cancelled'",
            [id],
        )?;
        if n == 0 {
            return Err(Error::Invalid("Nur abgesagte Sitzungen können wieder geplant werden".into()));
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "session.reopen", entity_type: "session", entity_id: Some(id), meta: json!({}) })?;
        tx.commit()?;
        self.session(id)
    }

    /// Removes a session created by mistake. Only one with nothing attached (no recording, note
    /// or prep); everything else is cancelled instead, so the record stays.
    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.require_untouched(id)?;
        let has_prep: bool =
            self.conn.query_row("SELECT EXISTS (SELECT 1 FROM session_prep WHERE session_id = ?1)", [id], |r| r.get(0))?;
        if has_prep {
            return Err(Error::Invalid("Die Sitzung hat eine Vorbereitung: absagen statt löschen".into()));
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM session WHERE id = ?1", [id])?;
        audit::append(&tx, Entry { actor: ACTOR, action: "session.delete", entity_type: "session", entity_id: Some(id), meta: json!({}) })?;
        tx.commit()?;
        Ok(())
    }

    /// Several sessions in one go ("Serie anlegen"). The UI computes the dates in local time, so a
    /// series keeps its wall-clock time across a DST change.
    pub fn create_sessions(&self, inputs: &[SessionInput]) -> Result<Vec<Session>> {
        if inputs.is_empty() || inputs.len() > MAX_SERIES as usize {
            return Err(Error::Invalid(format!("Eine Serie hat 1 bis {MAX_SERIES} Termine")));
        }
        let mut rows = Vec::with_capacity(inputs.len());
        for input in inputs {
            self.patient(&input.patient_id)?;
            let at = input.scheduled_at.as_deref().ok_or_else(|| Error::Invalid("Termin ohne Zeitpunkt".into()))?;
            check_minutes(input.planned_minutes)?;
            rows.push((new_id(), normalize_ts(at)?, input));
        }
        let tx = self.conn.unchecked_transaction()?;
        for (id, at, input) in &rows {
            tx.execute(
                "INSERT INTO session (id, patient_id, episode_id, scheduled_at, modality, planned_minutes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, input.patient_id, input.episode_id, at, input.modality, input.planned_minutes],
            )?;
            audit::append(&tx, Entry {
                actor: ACTOR, action: "session.create", entity_type: "session", entity_id: Some(id),
                meta: json!({ "series": rows.len() }),
            })?;
        }
        tx.commit()?;
        rows.iter().map(|(id, ..)| self.session(id)).collect()
    }

    /// Audit for an .ics export: how many sessions, nothing about whom.
    pub fn log_calendar_export(&self, sessions: usize) -> Result<()> {
        audit::append(&self.conn, Entry {
            actor: ACTOR, action: "calendar.export", entity_type: "calendar", entity_id: None,
            meta: json!({ "format": "ics", "sessions": sessions }),
        })
    }

    // ---------- tasks ----------

    /// Tasks dated in `[from, to]` (local dates); with `open_before` also undone ones from earlier
    /// days, so nothing gets lost overnight.
    pub fn list_tasks(&self, from: &str, to: &str, open_before: bool) -> Result<Vec<Task>> {
        check_date(from)?;
        check_date(to)?;
        let mut stmt = self.conn.prepare(&format!(
            "{TASK_SELECT} WHERE (date >= ?1 AND date <= ?2) OR (?3 AND date < ?1 AND done = 0)
             ORDER BY date, time IS NULL, time, created_at"
        ))?;
        let rows = stmt.query_map(params![from, to, open_before], row_to_task)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    fn task(&self, id: &str) -> Result<Task> {
        self.conn
            .query_row(&format!("{TASK_SELECT} WHERE id = ?1"), [id], row_to_task)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("task {id}")))
    }

    pub fn create_task(&self, input: TaskInput) -> Result<Task> {
        check_task(&input)?;
        let id = new_id();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO task (id, date, time, title, patient_id, session_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, input.date, input.time, input.title.trim(), input.patient_id, input.session_id, now()],
        )?;
        audit::append(&tx, Entry { actor: ACTOR, action: "task.create", entity_type: "task", entity_id: Some(&id), meta: json!({}) })?;
        tx.commit()?;
        self.task(&id)
    }

    pub fn update_task(&self, id: &str, input: TaskInput) -> Result<Task> {
        check_task(&input)?;
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute(
            "UPDATE task SET date = ?2, time = ?3, title = ?4, patient_id = ?5, session_id = ?6 WHERE id = ?1",
            params![id, input.date, input.time, input.title.trim(), input.patient_id, input.session_id],
        )?;
        if n == 0 {
            return Err(Error::NotFound(format!("task {id}")));
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "task.update", entity_type: "task", entity_id: Some(id), meta: json!({}) })?;
        tx.commit()?;
        self.task(id)
    }

    pub fn set_task_done(&self, id: &str, done: bool) -> Result<Task> {
        let tx = self.conn.unchecked_transaction()?;
        if tx.execute("UPDATE task SET done = ?2 WHERE id = ?1", params![id, done])? == 0 {
            return Err(Error::NotFound(format!("task {id}")));
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "task.done", entity_type: "task", entity_id: Some(id), meta: json!({ "done": done }) })?;
        tx.commit()?;
        self.task(id)
    }

    pub fn delete_task(&self, id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if tx.execute("DELETE FROM task WHERE id = ?1", [id])? == 0 {
            return Err(Error::NotFound(format!("task {id}")));
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "task.delete", entity_type: "task", entity_id: Some(id), meta: json!({}) })?;
        tx.commit()?;
        Ok(())
    }

    // ---------- day note ----------

    pub fn day_note(&self, date: &str) -> Result<Option<String>> {
        check_date(date)?;
        Ok(self.conn.query_row("SELECT body FROM day_note WHERE date = ?1", [date], |r| r.get(0)).optional()?)
    }

    /// Saves the day's planning text; an empty text removes it. Audited without the text.
    pub fn set_day_note(&self, date: &str, body: &str) -> Result<()> {
        check_date(date)?;
        let tx = self.conn.unchecked_transaction()?;
        if body.trim().is_empty() {
            tx.execute("DELETE FROM day_note WHERE date = ?1", [date])?;
        } else {
            tx.execute(
                "INSERT INTO day_note (date, body, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(date) DO UPDATE SET body = excluded.body, updated_at = excluded.updated_at",
                params![date, body, now()],
            )?;
        }
        audit::append(&tx, Entry {
            actor: ACTOR, action: "day_note.set", entity_type: "day_note", entity_id: Some(date),
            meta: json!({ "empty": body.trim().is_empty() }),
        })?;
        tx.commit()?;
        Ok(())
    }

    // ---------- due items ----------

    /// "Fällig" on the Heute page, derived (nothing is stored):
    /// - sessions in `[from, day_start)` that weren't cancelled and have no signed note,
    /// - planned sessions in `[day_start, until)` with nothing done yet (no prep, recording or note),
    /// - Berichte for an application still in draft or review.
    pub fn due_items(&self, from: &str, day_start: &str, until: &str) -> Result<Vec<DueItem>> {
        let (from, day_start, until) = (normalize_ts(from)?, normalize_ts(day_start)?, normalize_ts(until)?);
        let mut items = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.first_name || ' ' || p.last_name, p.chiffre, s.id, n.id, COALESCE(s.scheduled_at, s.started_at),
                 CASE WHEN COALESCE(s.scheduled_at, s.started_at) < ?2 THEN 'note_open' ELSE 'prep_missing' END
             FROM session s JOIN patient p ON p.id = s.patient_id
             LEFT JOIN report n ON n.session_id = s.id AND n.kind = 'sitzungsnotiz'
             WHERE s.status != 'cancelled' AND (
                 (COALESCE(s.scheduled_at, s.started_at) >= ?1 AND COALESCE(s.scheduled_at, s.started_at) < ?2
                    AND (n.id IS NULL OR n.status NOT IN ('signed', 'exported')))
              OR (COALESCE(s.scheduled_at, s.started_at) >= ?2 AND COALESCE(s.scheduled_at, s.started_at) < ?3
                    AND s.status = 'planned' AND n.id IS NULL
                    AND NOT EXISTS (SELECT 1 FROM session_prep sp WHERE sp.session_id = s.id)
                    AND NOT EXISTS (SELECT 1 FROM session_source src WHERE src.session_id = s.id)))
             ORDER BY COALESCE(s.scheduled_at, s.started_at), s.id",
        )?;
        let rows = stmt.query_map(params![from, day_start, until], |r| {
            Ok(DueItem {
                patient_id: r.get(0)?,
                patient_name: r.get(1)?,
                chiffre: r.get(2)?,
                session_id: r.get(3)?,
                report_id: r.get(4)?,
                at: r.get(5)?,
                kind: super::parse_col(r, 6)?,
                application_kind: None,
                seq: None,
            })
        })?;
        for row in rows {
            items.push(row?);
        }
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.first_name || ' ' || p.last_name, p.chiffre, r.id, r.updated_at, a.kind, a.seq
             FROM report r JOIN application a ON a.id = r.application_id JOIN patient p ON p.id = r.patient_id
             WHERE r.kind = 'bericht' AND r.status IN ('draft', 'in_review')
             ORDER BY r.updated_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(DueItem {
                kind: DueKind::ReportDraft,
                patient_id: r.get(0)?,
                patient_name: r.get(1)?,
                chiffre: r.get(2)?,
                session_id: None,
                report_id: r.get(3)?,
                at: r.get(4)?,
                application_kind: Some(super::parse_col(r, 5)?),
                seq: r.get(6)?,
            })
        })?;
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit;

    fn patient(store: &Store, last: &str, chiffre: &str) -> Patient {
        store
            .create_patient(PatientInput {
                first_name: "Anna".into(),
                last_name: last.into(),
                chiffre: Some(chiffre.into()),
                ..Default::default()
            })
            .unwrap()
    }

    fn at(store: &Store, p: &Patient, when: &str) -> Session {
        store
            .create_session(SessionInput {
                patient_id: p.id.clone(),
                episode_id: None,
                scheduled_at: Some(when.into()),
                modality: None,
                planned_minutes: None,
            })
            .unwrap()
    }

    #[test]
    fn times_are_normalized_to_utc_without_fractions() {
        assert_eq!(normalize_ts("2026-10-28T09:00:00.000Z").unwrap(), "2026-10-28T09:00:00Z");
        assert_eq!(normalize_ts("2026-10-28T10:30:15.5+01:00").unwrap(), "2026-10-28T09:30:15Z");
        assert!(normalize_ts("28.10.2026 09:00").is_err());
        let store = Store::open_in_memory().unwrap();
        let p = patient(&store, "Beispiel", "B1");
        assert_eq!(at(&store, &p, "2026-10-28T09:00:00.000Z").scheduled_at.as_deref(), Some("2026-10-28T09:00:00Z"));
    }

    #[test]
    fn range_query_numbers_sessions_and_counts_review_state() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let p = patient(&store, "Beispiel", "B1");
        let q = patient(&store, "Muster", "M2");
        let s1 = at(&store, &p, "2026-10-26T08:00:00Z");
        let s2 = at(&store, &p, "2026-10-28T08:00:00Z");
        let other = at(&store, &q, "2026-10-28T07:00:00Z");
        at(&store, &p, "2026-11-04T08:00:00Z");
        store.cancel_session(&s1.id, Some("krank")).unwrap();

        let (src, w) = store.start_source(&s2.id, SourceKind::Dictation, dir.path()).unwrap();
        let rows = store.sessions_between("2026-10-28T00:00:00Z", "2026-10-29T00:00:00Z").unwrap();
        assert_eq!(rows.iter().map(|r| r.session.id.as_str()).collect::<Vec<_>>(), [other.id.as_str(), s2.id.as_str()]);
        assert!(rows[1].recording && rows[1].sources_finished == 0);
        store.finish_source(&src.id, "stopped", &w).unwrap();

        let rows = store.sessions_between("2026-10-26T00:00:00.000Z", "2026-11-01T00:00:00+00:00").unwrap();
        assert_eq!(rows.len(), 3);
        let r2 = rows.iter().find(|r| r.session.id == s2.id).unwrap();
        assert_eq!((r2.number, r2.chiffre.as_deref(), r2.patient_name.as_str()), (1, Some("B1"), "Anna Beispiel"));
        assert_eq!((r2.sources_finished, r2.sources_unreviewed, r2.recording), (1, 1, false));
        let r1 = rows.iter().find(|r| r.session.id == s1.id).unwrap();
        assert_eq!((r1.session.status.as_str(), r1.session.cancelled_reason.as_deref()), ("cancelled", Some("krank")));
    }

    #[test]
    fn documented_sessions_move_until_signed_but_never_cancel_or_go_away() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let p = patient(&store, "Beispiel", "B1");
        let s = at(&store, &p, "2026-10-28T08:00:00Z");
        let update = |when: &str, minutes: Option<i64>| SessionUpdate {
            scheduled_at: when.into(),
            planned_minutes: minutes,
            modality: Some("video".into()),
        };
        let moved = store.update_session(&s.id, update("2026-10-29T10:15:00+01:00", Some(25))).unwrap();
        assert_eq!((moved.scheduled_at.as_deref(), moved.planned_minutes), (Some("2026-10-29T09:15:00Z"), Some(25)));
        assert!(store.update_session(&s.id, update("2026-10-29T09:00:00Z", Some(0))).is_err(), "length is checked");

        store.cancel_session(&s.id, Some("krank")).unwrap();
        assert!(store.create_session_note(&s.id).is_err(), "no note on a cancelled session");
        assert!(store.start_source(&s.id, SourceKind::Dictation, dir.path()).is_err(), "no recording either");
        let back = store.reopen_session(&s.id).unwrap();
        assert_eq!((back.status.as_str(), back.cancelled_reason), ("planned", None));
        assert!(store.reopen_session(&s.id).is_err());

        let held = at(&store, &p, "2026-10-30T08:00:00Z");
        let note = store.create_session_note(&held.id).unwrap();
        assert!(store.cancel_session(&held.id, None).is_err());
        assert!(store.delete_session(&held.id).is_err());
        // Documented in the wrong session: it moves, and the unsigned note's date with it.
        let fixed = store.update_session(&held.id, update("2026-10-29T08:00:00Z", None)).unwrap();
        assert_eq!(fixed.scheduled_at.as_deref(), Some("2026-10-29T08:00:00Z"));
        store.sign_report(&note.id, "Dr. Test").unwrap();
        assert!(store.update_session(&held.id, update("2026-10-31T08:00:00Z", None)).is_err(), "a signed note keeps its date");

        let rec = at(&store, &p, "2026-11-01T08:00:00Z");
        let (src, w) = store.start_source(&rec.id, SourceKind::Dictation, dir.path()).unwrap();
        assert!(store.update_session(&rec.id, update("2026-11-02T08:00:00Z", None)).is_err(), "not while recording");
        store.finish_source(&src.id, "stopped", &w).unwrap();
        store.update_session(&rec.id, update("2026-11-02T08:00:00Z", None)).unwrap();

        store.delete_session(&s.id).unwrap();
        assert!(store.session(&s.id).is_err());
        let events = audit::list(store.conn(), 50).unwrap();
        for action in ["session.update", "session.cancel", "session.reopen", "session.delete"] {
            assert!(events.iter().any(|e| e.action == action), "{action} audited");
        }
        assert!(events.iter().all(|e| !e.meta_json.contains("krank")), "the reason stays out of the audit log");
        assert_eq!(audit::verify(store.conn()).unwrap(), None);
    }

    #[test]
    fn series_is_all_or_nothing() {
        let store = Store::open_in_memory().unwrap();
        let p = patient(&store, "Beispiel", "B1");
        let input = |when: Option<&str>| SessionInput {
            patient_id: p.id.clone(),
            episode_id: None,
            scheduled_at: when.map(Into::into),
            modality: Some("praesenz".into()),
            planned_minutes: Some(50),
        };
        let made = store
            .create_sessions(&[input(Some("2026-10-21T08:00:00Z")), input(Some("2026-10-28T08:00:00Z")), input(Some("2026-11-04T09:00:00Z"))])
            .unwrap();
        assert_eq!(made.len(), 3);
        assert!(store.create_sessions(&[input(Some("2026-12-01T08:00:00Z")), input(None)]).is_err());
        assert!(store.create_sessions(&[]).is_err());
        assert_eq!(store.list_sessions(Some(&p.id)).unwrap().len(), 3, "a failed series leaves nothing behind");
    }

    #[test]
    fn tasks_and_day_note_round_trip_and_open_tasks_carry_over() {
        let store = Store::open_in_memory().unwrap();
        let task = |date: &str, time: Option<&str>, title: &str| TaskInput {
            date: date.into(),
            time: time.map(Into::into),
            title: title.into(),
            ..Default::default()
        };
        let old = store.create_task(task("2026-10-27", None, "Postfach prüfen")).unwrap();
        let done_old = store.create_task(task("2026-10-26", None, "Erledigt")).unwrap();
        store.set_task_done(&done_old.id, true).unwrap();
        let today = store.create_task(task("2026-10-28", Some("09:00"), " Leitlinie lesen ")).unwrap();
        assert_eq!(today.title, "Leitlinie lesen");
        assert!(store.create_task(task("28.10.", None, "x")).is_err());
        assert!(store.create_task(task("2026-10-28", Some("9 Uhr"), "x")).is_err());
        assert!(store.create_task(task("2026-10-28", None, "  ")).is_err());

        let ids = |v: Vec<Task>| v.into_iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids(store.list_tasks("2026-10-28", "2026-10-28", true).unwrap()), [old.id.clone(), today.id.clone()]);
        assert_eq!(ids(store.list_tasks("2026-10-28", "2026-10-28", false).unwrap()), [today.id.clone()]);
        let moved = store.update_task(&old.id, task("2026-10-29", None, "Postfach prüfen")).unwrap();
        assert_eq!(moved.date, "2026-10-29");
        store.delete_task(&today.id).unwrap();
        assert!(store.delete_task(&today.id).is_err());

        assert_eq!(store.day_note("2026-10-28").unwrap(), None);
        store.set_day_note("2026-10-28", "Fokus: Fortführungsbericht").unwrap();
        assert_eq!(store.day_note("2026-10-28").unwrap().as_deref(), Some("Fokus: Fortführungsbericht"));
        store.set_day_note("2026-10-28", " ").unwrap();
        assert_eq!(store.day_note("2026-10-28").unwrap(), None);

        let events = audit::list(store.conn(), 50).unwrap();
        assert!(events.iter().all(|e| !e.meta_json.contains("Postfach") && !e.meta_json.contains("Fokus")), "no free text in the audit log");
        assert_eq!(audit::verify(store.conn()).unwrap(), None);
    }

    #[test]
    fn due_items_cover_open_notes_missing_prep_and_draft_berichte() {
        let store = Store::open_in_memory().unwrap();
        let p = patient(&store, "Beispiel", "B1");
        let unsigned = at(&store, &p, "2026-10-26T08:00:00Z");
        let note = store.create_session_note(&unsigned.id).unwrap();
        let undocumented = at(&store, &p, "2026-10-27T08:00:00Z");
        let signed = at(&store, &p, "2026-10-27T10:00:00Z");
        let signed_note = store.create_session_note(&signed.id).unwrap();
        store.sign_report(&signed_note.id, "Dr. Test").unwrap();
        let cancelled = at(&store, &p, "2026-10-27T12:00:00Z");
        store.cancel_session(&cancelled.id, None).unwrap();
        let tomorrow = at(&store, &p, "2026-10-29T08:00:00Z");
        let noted = at(&store, &p, "2026-10-29T10:00:00Z");
        store.create_session_note(&noted.id).unwrap(); // already past preparing
        at(&store, &p, "2026-11-10T08:00:00Z"); // after `until`
        at(&store, &p, "2026-09-01T08:00:00Z"); // before `from`

        let ep = store
            .create_episode(EpisodeInput { patient_id: p.id.clone(), method: Method::Vt, age_group: AgeGroup::Adult, setting: Setting::Einzel })
            .unwrap();
        store.create_application(&ep.id, ApplicationKind::Erstantrag).unwrap();

        let items = store.due_items("2026-10-01T00:00:00Z", "2026-10-28T00:00:00Z", "2026-10-30T00:00:00Z").unwrap();
        let got: Vec<_> = items.iter().map(|i| (i.kind, i.session_id.clone())).collect();
        assert_eq!(
            got,
            [
                (DueKind::NoteOpen, Some(unsigned.id.clone())),
                (DueKind::NoteOpen, Some(undocumented.id.clone())),
                (DueKind::PrepMissing, Some(tomorrow.id.clone())),
                (DueKind::ReportDraft, None),
            ]
        );
        assert_eq!(items[0].report_id.as_deref(), Some(note.id.as_str()));
        assert_eq!((items[3].application_kind, items[3].chiffre.as_deref()), (Some(ApplicationKind::Erstantrag), Some("B1")));
    }
}
