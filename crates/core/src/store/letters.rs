//! Briefe (slice 45): letters, the recipient address book, and the practice letterhead.

use std::collections::HashMap;

use rusqlite::{params, OptionalExtension};
use serde_json::json;

use super::{new_id, now, parse_col, row_to_consent, row_to_episode, Store, ACTOR, CONSENT_SELECT, EPISODE_SELECT};
use crate::audit::{self, Entry};
use crate::consent;
use crate::domain::*;
use crate::letter::{
    self, FillContext, Letter, LetterInput, LetterMeta, LetterRecipient, LetterState, LetterSummary, Practice, PracticeAsset,
    Material, Recipient, RecipientInput, RefLine, ReleaseInput, RequestQuestion, RequestSection,
};
use crate::templates::{self, FieldKind, Layout};
use crate::{Error, Result};

const LETTER_SELECT: &str = "SELECT report_id, patient_id, episode_id, recipient_id, recipient_json, salutation, subject,
    refs_json, place, letter_date, sent_at, sent_via, follow_up_on, follow_up_note, request_text, request_questions_json,
    material_json FROM letter";
const RECIPIENT_COLS: &str =
    "r.id, r.kind, r.name, r.department, r.lines_json, r.salutation, r.default_template, r.use_count, r.created_at, r.updated_at";

fn json_col<T: serde::de::DeserializeOwned + Default>(r: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<T> {
    let s: String = r.get(idx)?;
    Ok(serde_json::from_str(&s).unwrap_or_default())
}

fn row_to_letter(r: &rusqlite::Row<'_>) -> rusqlite::Result<Letter> {
    Ok(Letter {
        report_id: r.get(0)?,
        patient_id: r.get(1)?,
        episode_id: r.get(2)?,
        recipient_id: r.get(3)?,
        recipient: json_col(r, 4)?,
        salutation: r.get(5)?,
        subject: r.get(6)?,
        refs: json_col(r, 7)?,
        place: r.get(8)?,
        letter_date: r.get(9)?,
        sent_at: r.get(10)?,
        sent_via: r.get::<_, Option<String>>(11)?.and_then(|v| v.parse().ok()),
        follow_up_on: r.get(12)?,
        follow_up_note: r.get(13)?,
        request_text: r.get::<_, Option<String>>(14)?.unwrap_or_default(),
        questions: json_col(r, 15)?,
        material: json_col(r, 16)?,
    })
}

fn row_to_recipient(r: &rusqlite::Row<'_>) -> rusqlite::Result<Recipient> {
    Ok(Recipient {
        id: r.get(0)?,
        kind: parse_col(r, 1)?,
        name: r.get(2)?,
        department: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
        lines: json_col(r, 4)?,
        salutation: r.get(5)?,
        default_template: r.get(6)?,
        use_count: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
        role: None,
    })
}

/// `YYYY-MM-DD` or nothing.
fn check_date(label: &str, d: &Option<String>) -> Result<Option<String>> {
    let d = d.as_deref().map(str::trim).filter(|d| !d.is_empty());
    if let Some(d) = d {
        let fmt = time::macros::format_description!("[year]-[month]-[day]");
        time::Date::parse(d, &fmt).map_err(|_| Error::Invalid(format!("{label} muss JJJJ-MM-TT sein")))?;
    }
    Ok(d.map(str::to_string))
}

fn opt_text(v: &Option<String>) -> Option<String> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn clean_recipient_input(input: RecipientInput) -> Result<RecipientInput> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(Error::Invalid("Der Empfänger braucht einen Namen".into()));
    }
    Ok(RecipientInput {
        kind: input.kind,
        name,
        department: input.department.trim().to_string(),
        lines: input.lines.into_iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
        salutation: opt_text(&input.salutation),
        default_template: opt_text(&input.default_template),
    })
}

impl Store {
    // ---------- practice / letterhead ----------

    /// Settings → Praxis & Briefkopf. Defaults to an empty practice.
    pub fn practice(&self) -> Result<Practice> {
        Ok(self.setting(letter::PRACTICE_KEY)?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
    }

    pub fn set_practice(&self, p: &Practice) -> Result<Practice> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO app_setting (key, value_json) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            params![letter::PRACTICE_KEY, serde_json::to_string(p)?],
        )?;
        audit::append(&tx, Entry { actor: ACTOR, action: "practice.update", entity_type: "practice", entity_id: None, meta: json!({}) })?;
        tx.commit()?;
        self.practice()
    }

    /// Once: the free practice header of earlier versions becomes the practice fields.
    pub(super) fn migrate_practice_header(&self) -> Result<()> {
        if self.setting(letter::PRACTICE_KEY)?.is_some() {
            return Ok(());
        }
        let legacy = self.setting(letter::LEGACY_HEADER_KEY)?.and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        if legacy.trim().is_empty() {
            return Ok(());
        }
        self.set_setting(letter::PRACTICE_KEY, &serde_json::to_value(Practice::from_legacy_header(&legacy))?)
    }

    /// Stores the logo or signature image (any JPEG/PNG/WebP, normalized to a PNG).
    pub fn set_practice_asset(&self, kind: PracticeAsset, upload: &[u8]) -> Result<()> {
        let png = crate::photo::normalize_logo(upload)?;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO practice_asset (kind, png, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(kind) DO UPDATE SET png = excluded.png, updated_at = excluded.updated_at",
            params![kind.as_str(), png, now()],
        )?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "practice.asset_set", entity_type: "practice", entity_id: None, meta: json!({ "kind": kind.as_str() }),
        })?;
        tx.commit()?;
        Ok(())
    }

    pub fn practice_asset(&self, kind: PracticeAsset) -> Result<Option<Vec<u8>>> {
        Ok(self.conn.query_row("SELECT png FROM practice_asset WHERE kind = ?1", [kind.as_str()], |r| r.get(0)).optional()?)
    }

    pub fn remove_practice_asset(&self, kind: PracticeAsset) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if tx.execute("DELETE FROM practice_asset WHERE kind = ?1", [kind.as_str()])? > 0 {
            audit::append(&tx, Entry {
                actor: ACTOR, action: "practice.asset_remove", entity_type: "practice", entity_id: None, meta: json!({ "kind": kind.as_str() }),
            })?;
        }
        tx.commit()?;
        Ok(())
    }

    // ---------- address book ----------

    /// Most used first; `query` matches name, department and address.
    pub fn list_recipients(&self, query: Option<&str>) -> Result<Vec<Recipient>> {
        let like = format!("%{}%", query.unwrap_or("").trim());
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {RECIPIENT_COLS} FROM recipient r WHERE r.name || ' ' || IFNULL(r.department, '') || ' ' || r.lines_json LIKE ?1
             ORDER BY r.use_count DESC, r.name COLLATE NOCASE"
        ))?;
        let rows = stmt.query_map([like], row_to_recipient)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn recipient(&self, id: &str) -> Result<Recipient> {
        self.conn
            .query_row(&format!("SELECT {RECIPIENT_COLS} FROM recipient r WHERE r.id = ?1"), [id], row_to_recipient)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("recipient {id}")))
    }

    pub fn create_recipient(&self, input: RecipientInput) -> Result<Recipient> {
        let tx = self.conn.unchecked_transaction()?;
        let id = Self::insert_recipient(&tx, clean_recipient_input(input)?)?;
        tx.commit()?;
        self.recipient(&id)
    }

    fn insert_recipient(tx: &rusqlite::Connection, input: RecipientInput) -> Result<String> {
        let id = new_id();
        let ts = now();
        tx.execute(
            "INSERT INTO recipient (id, kind, name, department, lines_json, salutation, default_template, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                id, input.kind.as_str(), input.name, input.department, serde_json::to_string(&input.lines)?,
                input.salutation, input.default_template, ts
            ],
        )?;
        audit::append(tx, Entry { actor: ACTOR, action: "recipient.create", entity_type: "recipient", entity_id: Some(&id), meta: json!({}) })?;
        Ok(id)
    }

    pub fn update_recipient(&self, id: &str, input: RecipientInput) -> Result<Recipient> {
        let input = clean_recipient_input(input)?;
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute(
            "UPDATE recipient SET kind = ?2, name = ?3, department = ?4, lines_json = ?5, salutation = ?6, default_template = ?7,
                 updated_at = ?8 WHERE id = ?1",
            params![
                id, input.kind.as_str(), input.name, input.department, serde_json::to_string(&input.lines)?,
                input.salutation, input.default_template, now()
            ],
        )?;
        if n == 0 {
            return Err(Error::NotFound(format!("recipient {id}")));
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "recipient.update", entity_type: "recipient", entity_id: Some(id), meta: json!({}) })?;
        tx.commit()?;
        self.recipient(id)
    }

    /// Letters written to it keep their address (they hold a copy).
    pub fn delete_recipient(&self, id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if tx.execute("DELETE FROM recipient WHERE id = ?1", [id])? == 0 {
            return Err(Error::NotFound(format!("recipient {id}")));
        }
        tx.execute("UPDATE letter SET recipient_id = NULL WHERE recipient_id = ?1", [id])?;
        audit::append(&tx, Entry { actor: ACTOR, action: "recipient.delete", entity_type: "recipient", entity_id: Some(id), meta: json!({}) })?;
        tx.commit()?;
        Ok(())
    }

    /// Remembers a recipient for the patient ("Hausärztin"), for one-click picks.
    pub fn link_recipient(&self, patient_id: &str, recipient_id: &str, role: Option<&str>) -> Result<()> {
        self.patient(patient_id)?;
        self.recipient(recipient_id)?;
        let role = role.map(str::trim).filter(|r| !r.is_empty());
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO patient_recipient (patient_id, recipient_id, role, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(patient_id, recipient_id) DO UPDATE SET role = excluded.role",
            params![patient_id, recipient_id, role, now()],
        )?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "recipient.link", entity_type: "patient", entity_id: Some(patient_id), meta: json!({ "recipient_id": recipient_id }),
        })?;
        tx.commit()?;
        Ok(())
    }

    pub fn unlink_recipient(&self, patient_id: &str, recipient_id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if tx.execute("DELETE FROM patient_recipient WHERE patient_id = ?1 AND recipient_id = ?2", params![patient_id, recipient_id])? > 0 {
            audit::append(&tx, Entry {
                actor: ACTOR, action: "recipient.unlink", entity_type: "patient", entity_id: Some(patient_id), meta: json!({ "recipient_id": recipient_id }),
            })?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The patient's remembered recipients, with their role.
    pub fn patient_recipients(&self, patient_id: &str) -> Result<Vec<Recipient>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {RECIPIENT_COLS}, pr.role FROM recipient r JOIN patient_recipient pr ON pr.recipient_id = r.id
             WHERE pr.patient_id = ?1 ORDER BY pr.created_at"
        ))?;
        let rows = stmt.query_map([patient_id], |r| Ok(Recipient { role: r.get(10)?, ..row_to_recipient(r)? }))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    // ---------- letters ----------

    /// The patient's newest open Behandlungsfall, else the newest one.
    fn current_episode(&self, patient_id: &str) -> Result<Option<Episode>> {
        Ok(self
            .conn
            .query_row(
                &format!("{EPISODE_SELECT} WHERE patient_id = ?1 ORDER BY ended_at IS NULL DESC, started_at DESC LIMIT 1"),
                [patient_id],
                row_to_episode,
            )
            .optional()?)
    }

    /// A new draft letter: sections from the template (starter texts and the Diagnosen filled
    /// from the record), Betreff and references from the patient, address from the book entry.
    pub fn create_letter(&self, input: LetterInput) -> Result<Letter> {
        let patient = self.patient(&input.patient_id)?;
        let template = self.template(&input.template_key)?;
        if template.layout != Layout::Letter {
            return Err(Error::Invalid(format!("{} ist keine Briefvorlage", template.title)));
        }
        let episode = match &input.episode_id {
            Some(id) => {
                let e = self.episode(id)?;
                if e.patient_id != patient.id {
                    return Err(Error::Invalid("Der Behandlungsfall gehört zu einer anderen Person".into()));
                }
                Some(e)
            }
            None => self.current_episode(&patient.id)?,
        };
        let template = match &episode {
            Some(e) => template.resolve(&templates::Context { method: e.method, age_group: e.age_group, kind: None }),
            None => template,
        };
        let recipient = input.recipient_id.as_deref().map(|id| self.recipient(id)).transpose()?;
        let ctx = FillContext { patient: &patient, episode: episode.as_ref() };
        let subject = letter::fill(template.subject.as_deref().unwrap_or(&template.title), &ctx);
        let refs: Vec<RefLine> = template
            .refs
            .iter()
            .map(|r| RefLine { label: r.label.clone(), value: r.from.map(|f| letter::patient_ref(&patient, f)).unwrap_or_default() })
            .collect();
        let salutation = recipient
            .as_ref()
            .and_then(|r| r.salutation.clone())
            .unwrap_or_else(|| letter::DEFAULT_SALUTATION.to_string());
        let place = self.practice()?.place_or_city();

        let tx = self.conn.unchecked_transaction()?;
        let id = new_id();
        let ts = now();
        tx.execute(
            "INSERT INTO report (id, patient_id, episode_id, template_key, template_version, kind, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![id, patient.id, episode.as_ref().map(|e| &e.id), template.key, template.version, letter::KIND, ts],
        )?;
        for s in &template.sections {
            let body = match (s.field, &s.starter) {
                (FieldKind::Diagnoses, _) => match &episode {
                    Some(e) if !e.diagnoses.is_empty() => serde_json::to_string(&crate::pii::DiagnosesBody {
                        diagnoses: e.diagnoses.clone(),
                        notes: String::new(),
                    })?,
                    _ => String::new(),
                },
                (FieldKind::Prose, Some(starter)) => letter::starter_html(&letter::fill(starter, &ctx)),
                (FieldKind::Prose, None) => String::new(),
            };
            tx.execute("INSERT INTO report_section (id, report_id, key, body) VALUES (?1, ?2, ?3, ?4)", params![new_id(), id, s.key, body])?;
        }
        tx.execute(
            "INSERT INTO letter (report_id, patient_id, episode_id, recipient_id, recipient_json, salutation, subject, refs_json, place)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                patient.id,
                episode.as_ref().map(|e| &e.id),
                recipient.as_ref().map(|r| &r.id),
                serde_json::to_string(&recipient.as_ref().map(Recipient::as_letter).unwrap_or_default())?,
                salutation,
                subject,
                serde_json::to_string(&refs)?,
                place,
            ],
        )?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "letter.create", entity_type: "report", entity_id: Some(&id),
            meta: json!({ "template": template.key, "recipient": recipient.is_some() }),
        })?;
        tx.commit()?;
        self.letter(&id)
    }

    pub fn letter(&self, report_id: &str) -> Result<Letter> {
        self.conn
            .query_row(&format!("{LETTER_SELECT} WHERE report_id = ?1"), [report_id], row_to_letter)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("letter {report_id}")))
    }

    /// Newest change first; `patient_id` narrows to one patient (timeline, patient page).
    pub fn list_letters(&self, patient_id: Option<&str>) -> Result<Vec<LetterSummary>> {
        let titles: HashMap<String, String> = self.list_templates()?.into_iter().map(|t| (t.key, t.title)).collect();
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.patient_id, p.first_name || ' ' || p.last_name, l.episode_id, r.template_key, l.recipient_json,
                    l.subject, r.status, r.created_at, r.updated_at, r.signed_at, r.exported_at, l.sent_at, l.sent_via, l.follow_up_on
             FROM report r JOIN letter l ON l.report_id = r.id JOIN patient p ON p.id = r.patient_id
             WHERE r.kind = 'brief' AND (?1 IS NULL OR r.patient_id = ?1)
             ORDER BY r.updated_at DESC",
        )?;
        let rows = stmt.query_map([patient_id], |r| {
            let recipient: LetterRecipient = json_col(r, 5)?;
            let status: ReportStatus = parse_col(r, 7)?;
            let template_key: String = r.get(4)?;
            let sent_at: Option<String> = r.get(12)?;
            Ok(LetterSummary {
                report_id: r.get(0)?,
                patient_id: r.get(1)?,
                patient_name: r.get(2)?,
                episode_id: r.get(3)?,
                template_title: titles.get(&template_key).cloned().unwrap_or_else(|| template_key.clone()),
                template_key,
                recipient_name: recipient.name.clone(),
                recipient_kind: recipient.kind,
                subject: r.get(6)?,
                state: LetterState::of(status, sent_at.as_deref()),
                status,
                created_at: r.get(8)?,
                updated_at: r.get(9)?,
                signed_at: r.get(10)?,
                exported_at: r.get(11)?,
                sent_at,
                sent_via: r.get::<_, Option<String>>(13)?.and_then(|v| v.parse().ok()),
                follow_up_on: r.get(14)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    fn letter_status(conn: &rusqlite::Connection, report_id: &str) -> Result<ReportStatus> {
        let status: Option<String> = conn
            .query_row("SELECT r.status FROM report r JOIN letter l ON l.report_id = r.id WHERE r.id = ?1", [report_id], |r| r.get(0))
            .optional()?;
        status.ok_or_else(|| Error::NotFound(format!("letter {report_id}")))?.parse()
    }

    /// Address, Betreff, references, date and Wiedervorlage of a draft. The Wiedervorlage can
    /// still change after signing; everything printed can't.
    pub fn save_letter_meta(&self, report_id: &str, meta: LetterMeta) -> Result<Letter> {
        let letter_date = check_date("Briefdatum", &meta.letter_date)?;
        let follow_up_on = check_date("Wiedervorlage", &meta.follow_up_on)?;
        let tx = self.conn.unchecked_transaction()?;
        let status = Self::letter_status(&tx, report_id)?;
        if let Some(episode_id) = &meta.episode_id {
            let owner: Option<String> = tx
                .query_row(
                    "SELECT e.id FROM episode e JOIN letter l ON l.patient_id = e.patient_id WHERE e.id = ?1 AND l.report_id = ?2",
                    params![episode_id, report_id],
                    |r| r.get(0),
                )
                .optional()?;
            if owner.is_none() {
                return Err(Error::Invalid("Der Behandlungsfall gehört zu einer anderen Person".into()));
            }
        }
        let ts = now();
        if matches!(status, ReportStatus::Signed | ReportStatus::Exported) {
            tx.execute(
                "UPDATE letter SET follow_up_on = ?2, follow_up_note = ?3, episode_id = ?4 WHERE report_id = ?1",
                params![report_id, follow_up_on, opt_text(&meta.follow_up_note), meta.episode_id],
            )?;
        } else {
            let refs: Vec<RefLine> = meta
                .refs
                .into_iter()
                .map(|r| RefLine { label: r.label.trim().to_string(), value: r.value.trim().to_string() })
                .filter(|r| !r.label.is_empty() || !r.value.is_empty())
                .collect();
            tx.execute(
                "UPDATE letter SET episode_id = ?2, recipient_id = ?3, recipient_json = ?4, salutation = ?5, subject = ?6,
                     refs_json = ?7, place = ?8, letter_date = ?9, follow_up_on = ?10, follow_up_note = ?11
                 WHERE report_id = ?1",
                params![
                    report_id,
                    meta.episode_id,
                    meta.recipient_id,
                    serde_json::to_string(&meta.recipient.cleaned())?,
                    meta.salutation.trim(),
                    meta.subject.trim(),
                    serde_json::to_string(&refs)?,
                    opt_text(&meta.place),
                    letter_date,
                    follow_up_on,
                    opt_text(&meta.follow_up_note),
                ],
            )?;
            tx.execute("UPDATE report SET updated_at = ?2, episode_id = ?3 WHERE id = ?1", params![report_id, ts, meta.episode_id])?;
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "letter.meta_save", entity_type: "report", entity_id: Some(report_id), meta: json!({}) })?;
        tx.commit()?;
        self.letter(report_id)
    }

    /// A draft letter, or an error naming why it can't change.
    fn draft_letter(tx: &rusqlite::Connection, report_id: &str) -> Result<()> {
        if matches!(Self::letter_status(tx, report_id)?, ReportStatus::Signed | ReportStatus::Exported) {
            return Err(Error::Invalid("Der Brief ist unterschrieben und schreibgeschützt".into()));
        }
        Ok(())
    }

    /// The pasted request ("Anschreiben"). Its text stays in the encrypted database; only its
    /// length goes into the audit log.
    pub fn save_letter_request(&self, report_id: &str, text: &str) -> Result<Letter> {
        let text = text.trim();
        if text.chars().count() > crate::compose::MAX_REQUEST_CHARS * 4 {
            return Err(Error::Invalid("Das Anschreiben ist zu lang".into()));
        }
        let tx = self.conn.unchecked_transaction()?;
        Self::draft_letter(&tx, report_id)?;
        tx.execute("UPDATE letter SET request_text = ?2 WHERE report_id = ?1", params![report_id, (!text.is_empty()).then_some(text)])?;
        tx.execute("UPDATE report SET updated_at = ?2 WHERE id = ?1", params![report_id, now()])?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "letter.request", entity_type: "report", entity_id: Some(report_id),
            meta: json!({ "chars": text.chars().count() }),
        })?;
        tx.commit()?;
        self.letter(report_id)
    }

    /// The request's questions as found (or as she edited them). Questions already taken over as
    /// sections keep their section when the text is the same.
    pub fn set_letter_questions(&self, report_id: &str, questions: &[String]) -> Result<Letter> {
        let old = self.letter(report_id)?.questions;
        let mut out: Vec<RequestQuestion> = Vec::new();
        for q in questions {
            let text = q.split_whitespace().collect::<Vec<_>>().join(" ");
            if text.is_empty() || out.iter().any(|x| x.text == text) {
                continue;
            }
            if text.chars().count() > crate::compose::MAX_QUESTION_CHARS {
                return Err(Error::Invalid(format!("Eine Frage ist zu lang (höchstens {} Zeichen)", crate::compose::MAX_QUESTION_CHARS)));
            }
            let section_key = old.iter().find(|o| o.text == text).and_then(|o| o.section_key.clone());
            out.push(RequestQuestion { text, section_key });
        }
        if out.len() > crate::compose::MAX_QUESTIONS {
            return Err(Error::Invalid(format!("Höchstens {} Fragen", crate::compose::MAX_QUESTIONS)));
        }
        let tx = self.conn.unchecked_transaction()?;
        Self::draft_letter(&tx, report_id)?;
        tx.execute("UPDATE letter SET request_questions_json = ?2 WHERE report_id = ?1", params![report_id, serde_json::to_string(&out)?])?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "letter.questions", entity_type: "report", entity_id: Some(report_id),
            meta: json!({ "questions": out.len() }),
        })?;
        tx.commit()?;
        self.letter(report_id)
    }

    /// The letter's request sections, in her order: existing ones keep their text, new ones start
    /// empty, the rest are removed. `keep_template`: the template's sections stay (missing ones
    /// come back); otherwise its empty prose sections give way, except a headingless opening.
    pub fn set_request_sections(&self, report_id: &str, sections: &[RequestSection], keep_template: bool) -> Result<Letter> {
        let mut letter = self.letter(report_id)?;
        let report = self.report(report_id)?;
        let template = self.template(&report.template_key)?;
        let existing: Vec<&crate::domain::ReportSection> = report.sections.iter().filter(|s| s.heading.is_some()).collect();
        for s in sections {
            if s.heading.trim().is_empty() {
                return Err(Error::Invalid("Jeder Abschnitt braucht eine Überschrift".into()));
            }
            if s.heading.chars().count() > crate::compose::MAX_QUESTION_CHARS {
                return Err(Error::Invalid(format!("Eine Überschrift ist zu lang (höchstens {} Zeichen)", crate::compose::MAX_QUESTION_CHARS)));
            }
            if let Some(k) = &s.key {
                if !existing.iter().any(|e| &e.key == k) {
                    return Err(Error::NotFound(format!("section {k}")));
                }
            }
            if s.question.is_some_and(|i| i >= letter.questions.len()) {
                return Err(Error::Invalid("Unbekannte Frage".into()));
            }
        }
        let tx = self.conn.unchecked_transaction()?;
        Self::draft_letter(&tx, report_id)?;
        let mut next = existing
            .iter()
            .filter_map(|e| e.key.strip_prefix(letter::REQUEST_SECTION_PREFIX).and_then(|n| n.parse::<usize>().ok()))
            .max()
            .unwrap_or(0);
        for q in &mut letter.questions {
            q.section_key = None;
        }
        let mut kept = Vec::new();
        for (ord, s) in sections.iter().enumerate() {
            let key = match &s.key {
                Some(k) => {
                    tx.execute(
                        "UPDATE report_section SET heading = ?3, ord = ?4 WHERE report_id = ?1 AND key = ?2",
                        params![report_id, k, s.heading.trim(), ord as i64],
                    )?;
                    k.clone()
                }
                None => {
                    next += 1;
                    let k = format!("{}{next}", letter::REQUEST_SECTION_PREFIX);
                    tx.execute(
                        "INSERT INTO report_section (id, report_id, key, body, heading, ord) VALUES (?1, ?2, ?3, '', ?4, ?5)",
                        params![new_id(), report_id, k, s.heading.trim(), ord as i64],
                    )?;
                    k
                }
            };
            if let Some(q) = s.question.and_then(|i| letter.questions.get_mut(i)) {
                q.section_key = Some(key.clone());
            }
            kept.push(key);
        }
        for e in existing.iter().filter(|e| !kept.contains(&e.key)) {
            tx.execute("DELETE FROM report_section WHERE report_id = ?1 AND key = ?2", params![report_id, e.key])?;
            tx.execute("DELETE FROM section_draft WHERE report_id = ?1 AND section_key = ?2", params![report_id, e.key])?;
        }
        let opening = template.sections.first().filter(|d| d.heading.trim().is_empty()).map(|d| d.key.clone());
        for def in &template.sections {
            let row = report.sections.iter().find(|s| s.key == def.key);
            if keep_template || sections.is_empty() {
                if row.is_none() {
                    tx.execute("INSERT INTO report_section (id, report_id, key, body) VALUES (?1, ?2, ?3, '')", params![new_id(), report_id, def.key])?;
                }
            } else if let Some(row) = row {
                let empty = crate::pii::section_text(def.field, &row.body).trim().is_empty();
                if def.field == FieldKind::Prose && empty && Some(&def.key) != opening.as_ref() {
                    tx.execute("DELETE FROM report_section WHERE report_id = ?1 AND key = ?2", params![report_id, def.key])?;
                    tx.execute("DELETE FROM section_draft WHERE report_id = ?1 AND section_key = ?2", params![report_id, def.key])?;
                }
            }
        }
        tx.execute("UPDATE letter SET request_questions_json = ?2 WHERE report_id = ?1", params![report_id, serde_json::to_string(&letter.questions)?])?;
        tx.execute("UPDATE report SET updated_at = ?2 WHERE id = ?1", params![report_id, now()])?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "letter.sections", entity_type: "report", entity_id: Some(report_id),
            meta: json!({ "sections": sections.len(), "keep_template": keep_template }),
        })?;
        super::focus::sync_review(&tx, report_id)?;
        tx.commit()?;
        self.letter(report_id)
    }

    /// The Vorbericht of a letter: the newest signed Bericht or letter of its Behandlungsfall.
    pub fn letter_prior(&self, report_id: &str) -> Result<Option<Report>> {
        let report = self.report(report_id)?;
        let Some(episode_id) = &report.episode_id else { return Ok(None) };
        let id: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM report WHERE episode_id = ?1 AND kind IN ('bericht', 'brief') AND status IN ('signed', 'exported')
                   AND id != ?2 ORDER BY signed_at DESC, rowid DESC LIMIT 1",
                params![episode_id, report_id],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|id| self.report(&id)).transpose()
    }

    /// What "Mit KI schreiben" may use.
    pub fn set_letter_material(&self, report_id: &str, material: Material) -> Result<Letter> {
        let tx = self.conn.unchecked_transaction()?;
        Self::draft_letter(&tx, report_id)?;
        tx.execute("UPDATE letter SET material_json = ?2 WHERE report_id = ?1", params![report_id, serde_json::to_string(&material)?])?;
        tx.commit()?;
        self.letter(report_id)
    }

    /// After signing: the recipient's use counts; a typed-in recipient goes into the address
    /// book (unless `save_recipient` is off) and the letter points to it.
    pub fn letter_signed(&self, report_id: &str, save_recipient: bool) -> Result<Letter> {
        let letter = self.letter(report_id)?;
        let tx = self.conn.unchecked_transaction()?;
        if !matches!(Self::letter_status(&tx, report_id)?, ReportStatus::Signed | ReportStatus::Exported) {
            return Err(Error::Invalid("Der Brief ist noch nicht unterschrieben".into()));
        }
        let book_id = match &letter.recipient_id {
            Some(id) => tx.query_row("SELECT id FROM recipient WHERE id = ?1", [id], |r| r.get::<_, String>(0)).optional()?,
            None if save_recipient && !letter.recipient.name.trim().is_empty() => {
                let input = RecipientInput {
                    kind: letter.recipient.kind.unwrap_or(RecipientKind::Sonstige),
                    name: letter.recipient.name.clone(),
                    department: letter.recipient.department.clone(),
                    lines: letter.recipient.lines.clone(),
                    salutation: (letter.salutation != letter::DEFAULT_SALUTATION && !letter.salutation.trim().is_empty())
                        .then(|| letter.salutation.clone()),
                    default_template: None,
                };
                let id = Self::insert_recipient(&tx, input)?;
                tx.execute("UPDATE letter SET recipient_id = ?2 WHERE report_id = ?1", params![report_id, id])?;
                Some(id)
            }
            None => None,
        };
        if let Some(id) = book_id {
            tx.execute("UPDATE recipient SET use_count = use_count + 1 WHERE id = ?1", [id])?;
        }
        tx.commit()?;
        self.letter(report_id)
    }

    /// "Als versendet markieren" (`sent_at` YYYY-MM-DD), or undo it with `None`.
    pub fn mark_letter_sent(&self, report_id: &str, sent_at: Option<&str>, via: Option<SentVia>) -> Result<Letter> {
        let sent_at = check_date("Versanddatum", &sent_at.map(str::to_string))?;
        let tx = self.conn.unchecked_transaction()?;
        if !matches!(Self::letter_status(&tx, report_id)?, ReportStatus::Signed | ReportStatus::Exported) {
            return Err(Error::Invalid("Erst unterschreiben, dann versenden".into()));
        }
        let via = sent_at.as_ref().and(via);
        tx.execute(
            "UPDATE letter SET sent_at = ?2, sent_via = ?3 WHERE report_id = ?1",
            params![report_id, sent_at, via.map(SentVia::as_str)],
        )?;
        tx.execute("UPDATE report SET updated_at = ?2 WHERE id = ?1", params![report_id, now()])?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "letter.send", entity_type: "report", entity_id: Some(report_id),
            meta: json!({ "sent": sent_at.is_some(), "via": via.map(SentVia::as_str) }),
        })?;
        tx.commit()?;
        self.letter(report_id)
    }

    // ---------- Schweigepflichtentbindung (slice 46) ----------

    /// Records a Schweigepflichtentbindung she has on paper: for one address book entry or for a
    /// kind of recipient, dated the day it was signed (default today). One active per target.
    pub fn grant_release(&self, patient_id: &str, input: ReleaseInput) -> Result<Consent> {
        self.patient(patient_id)?;
        let recipient_id = opt_text(&input.recipient_id);
        if let Some(id) = &recipient_id {
            self.recipient(id)?;
        }
        let scope = if recipient_id.is_some() { None } else { input.scope };
        if recipient_id.is_none() && scope.is_none() {
            return Err(Error::Invalid("Für wen gilt die Schweigepflichtentbindung?".into()));
        }
        if self.release_for(patient_id, recipient_id.as_deref(), scope)?.is_some_and(|c| c.recipient_id == recipient_id) {
            return Err(Error::Invalid("Diese Schweigepflichtentbindung ist bereits erfasst".into()));
        }
        let granted_at = match check_date("Datum", &input.granted_on)? {
            // Noon UTC: the same calendar day in German time.
            Some(d) => format!("{d}T12:00:00Z"),
            None => now(),
        };
        let text = consent::current(ConsentKind::ReleaseOfInformation);
        let tx = self.conn.unchecked_transaction()?;
        let id = new_id();
        tx.execute(
            "INSERT INTO consent (id, patient_id, kind, scope, recipient_id, text_version, text_hash, granted_at, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                patient_id,
                ConsentKind::ReleaseOfInformation.as_str(),
                scope.map(RecipientKind::as_str),
                recipient_id,
                text.version,
                text.hash,
                granted_at,
                opt_text(&input.note),
            ],
        )?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "consent.grant", entity_type: "consent", entity_id: Some(&id),
            meta: json!({
                "kind": ConsentKind::ReleaseOfInformation.as_str(),
                "scope": scope.map(RecipientKind::as_str),
                "recipient": recipient_id.is_some(),
                "text_version": text.version,
            }),
        })?;
        tx.commit()?;
        self.consent(&id)
    }

    /// The active Schweigepflichtentbindung covering a recipient: for this book entry, else for
    /// its kind.
    pub fn release_for(&self, patient_id: &str, recipient_id: Option<&str>, kind: Option<RecipientKind>) -> Result<Option<Consent>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "{CONSENT_SELECT} WHERE patient_id = ?1 AND kind = 'release_of_information' AND revoked_at IS NULL
                       AND ((?2 IS NOT NULL AND recipient_id = ?2) OR (?3 IS NOT NULL AND recipient_id IS NULL AND scope = ?3))
                     ORDER BY recipient_id IS NULL, granted_at DESC LIMIT 1"
                ),
                params![patient_id, recipient_id, kind.map(RecipientKind::as_str)],
                row_to_consent,
            )
            .optional()?)
    }

    /// The Schweigepflichtentbindung a letter's recipient has, if any.
    pub fn letter_release(&self, letter: &Letter) -> Result<Option<Consent>> {
        self.release_for(&letter.patient_id, letter.recipient_id.as_deref(), letter.recipient.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgeGroup, Method, PatientContact, PatientInput, Setting};

    fn setup() -> (Store, Patient, Episode) {
        let store = Store::open_in_memory().unwrap();
        let p = store
            .create_patient(PatientInput {
                first_name: "Anna".into(),
                last_name: "Beispiel".into(),
                birth_date: Some("1985-04-03".into()),
                gender: Some(Gender::W),
                contact: PatientContact { street: Some(" Hauptstr. 3 ".into()), pension_no: Some("".into()), ..Default::default() },
                ..Default::default()
            })
            .unwrap();
        let ep = store
            .create_episode(EpisodeInput { patient_id: p.id.clone(), method: Method::Vt, age_group: AgeGroup::Adult, setting: Setting::Einzel })
            .unwrap();
        (store, p, ep)
    }

    #[test]
    fn patient_contact_is_stored_cleaned() {
        let (_store, p, _) = setup();
        assert_eq!(p.contact.street.as_deref(), Some("Hauptstr. 3"));
        assert_eq!(p.contact.pension_no, None);
    }

    #[test]
    fn letter_lifecycle_create_sign_send_timeline_trash_erase() {
        let (store, p, ep) = setup();
        let r = store
            .create_recipient(RecipientInput {
                kind: RecipientKind::Arzt,
                name: "Dr. med. Erika Poppe".into(),
                department: String::new(),
                lines: vec!["Lindenweg 2".into(), "".into(), "12345 Musterstadt".into()],
                salutation: Some("Sehr geehrte Frau Dr. Poppe,".into()),
                default_template: None,
            })
            .unwrap();
        assert_eq!(r.lines.len(), 2);
        let l = store
            .create_letter(LetterInput { patient_id: p.id.clone(), template_key: "bescheinigung".into(), episode_id: None, recipient_id: Some(r.id.clone()) })
            .unwrap();
        assert_eq!(l.episode_id.as_deref(), Some(ep.id.as_str()), "newest open case by default");
        assert_eq!(l.salutation, "Sehr geehrte Frau Dr. Poppe,");
        assert_eq!(l.subject, "Behandlungsbestätigung für Frau Anna Beispiel, geb. 03.04.1985");
        assert_eq!(l.recipient.address_lines(), ["Dr. med. Erika Poppe", "Lindenweg 2", "12345 Musterstadt"]);
        let report = store.report(&l.report_id).unwrap();
        assert_eq!(report.kind, "brief");
        let body = &report.sections[0].body;
        assert!(body.starts_with("<p>hiermit bestätige ich, dass sich Frau Anna Beispiel, geb. 03.04.1985, seit dem "), "{body}");
        assert!(!body.contains('{'));
        // Not a Bericht: the PTV 3 board doesn't list it.
        assert!(store.list_reports(None, &now()).unwrap().is_empty());

        let mut meta = LetterMeta {
            episode_id: l.episode_id.clone(),
            recipient_id: l.recipient_id.clone(),
            recipient: l.recipient.clone(),
            salutation: l.salutation.clone(),
            subject: "Bestätigung".into(),
            refs: vec![RefLine { label: "Aktenzeichen".into(), value: " 4711 ".into() }, RefLine::default()],
            follow_up_on: Some("2027-01-31".into()),
            ..Default::default()
        };
        let saved = store.save_letter_meta(&l.report_id, meta.clone()).unwrap();
        assert_eq!(saved.refs, vec![RefLine { label: "Aktenzeichen".into(), value: "4711".into() }]);
        meta.letter_date = Some("31.01.2027".into());
        assert!(store.save_letter_meta(&l.report_id, meta.clone()).is_err(), "dates are ISO");
        assert!(store.mark_letter_sent(&l.report_id, Some("2026-11-07"), Some(SentVia::Post)).is_err(), "sign first");

        store.sign_report(&l.report_id, "M. Muster").unwrap();
        store.letter_signed(&l.report_id, true).unwrap();
        assert_eq!(store.recipient(&r.id).unwrap().use_count, 1);
        meta.letter_date = None;
        meta.subject = "geändert".into();
        meta.follow_up_on = Some("2027-02-01".into());
        let after = store.save_letter_meta(&l.report_id, meta).unwrap();
        assert_eq!((after.subject.as_str(), after.follow_up_on.as_deref()), ("Bestätigung", Some("2027-02-01")), "signed: only the Wiedervorlage");
        let sent = store.mark_letter_sent(&l.report_id, Some("2026-11-07"), Some(SentVia::Fax)).unwrap();
        assert_eq!((sent.sent_at.as_deref(), sent.sent_via), (Some("2026-11-07"), Some(SentVia::Fax)));
        let list = store.list_letters(None).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].state, list[0].template_title.as_str(), list[0].recipient_name.as_str()), (LetterState::Sent, "Behandlungsbestätigung", "Dr. med. Erika Poppe"));

        let timeline = store.patient_timeline(&p.id, &now()).unwrap();
        assert!(timeline[0].items.iter().any(|i| matches!(i, crate::store::TimelineItem::Letter { .. })));

        // Signed → Papierkorb, and back with its letter data.
        assert!(store.delete_report(&l.report_id).unwrap().trashed);
        assert!(store.letter(&l.report_id).is_err());
        let trash = store.list_trash().unwrap();
        assert_eq!((trash[0].kind.as_str(), trash[0].detail.as_deref()), ("brief", Some("Behandlungsbestätigung")));
        store.restore_trash(&trash[0].id).unwrap();
        assert_eq!(store.letter(&l.report_id).unwrap().sent_via, Some(SentVia::Fax));

        store.link_recipient(&p.id, &r.id, Some("Hausärztin")).unwrap();
        assert_eq!(store.patient_recipients(&p.id).unwrap()[0].role.as_deref(), Some("Hausärztin"));
        store.erase_patient(&p.id).unwrap();
        let left: i64 = store.conn().query_row("SELECT (SELECT count(*) FROM letter) + (SELECT count(*) FROM patient_recipient)", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
        assert_eq!(store.list_recipients(None).unwrap().len(), 1, "the address book stays");
        let audit = crate::audit::list(store.conn(), 500).unwrap();
        assert!(audit.iter().any(|e| e.action == "letter.send"));
        assert!(audit.iter().all(|e| !e.meta_json.contains("Poppe") && !e.meta_json.contains("Beispiel")), "no names in the audit log");
        assert_eq!(crate::audit::verify(store.conn()).unwrap(), None);
    }

    #[test]
    fn typed_recipient_goes_to_the_book_on_signing() {
        let (store, p, _) = setup();
        let l = store
            .create_letter(LetterInput { patient_id: p.id.clone(), template_key: "freier-brief".into(), episode_id: None, recipient_id: None })
            .unwrap();
        assert_eq!(l.salutation, letter::DEFAULT_SALUTATION);
        let draft = store.delete_report(&l.report_id).unwrap();
        assert!(!draft.trashed, "a draft is deleted for good");

        let l = store
            .create_letter(LetterInput { patient_id: p.id.clone(), template_key: "freier-brief".into(), episode_id: None, recipient_id: None })
            .unwrap();
        let recipient = LetterRecipient { kind: Some(RecipientKind::Versorgungsamt), name: "Landratsamt".into(), department: "Versorgungsamt".into(), lines: vec!["Postfach 1".into()] };
        store.save_letter_meta(&l.report_id, LetterMeta { recipient, salutation: l.salutation.clone(), ..Default::default() }).unwrap();
        store.sign_report(&l.report_id, "M. Muster").unwrap();
        let signed = store.letter_signed(&l.report_id, true).unwrap();
        let book = store.list_recipients(Some("versorgungs")).unwrap();
        assert_eq!(book.len(), 1);
        assert_eq!((book[0].kind, book[0].use_count, book[0].salutation.clone()), (RecipientKind::Versorgungsamt, 1, None));
        assert_eq!(signed.recipient_id.as_deref(), Some(book[0].id.as_str()));
        store.delete_recipient(&book[0].id).unwrap();
        let kept = store.letter(&l.report_id).unwrap();
        assert_eq!((kept.recipient_id, kept.recipient.name.as_str()), (None, "Landratsamt"), "the letter keeps its copy");
    }

    #[test]
    fn practice_header_is_migrated_once() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting(letter::LEGACY_HEADER_KEY, &json!("Praxis Muster\nMusterstr. 1\n12345 Musterstadt")).unwrap();
        store.migrate_practice_header().unwrap();
        let p = store.practice().unwrap();
        assert_eq!((p.name.as_str(), p.city.as_str()), ("Praxis Muster", "Musterstadt"));
        store.set_practice(&Practice { name: "Neu".into(), ..p }).unwrap();
        store.migrate_practice_header().unwrap();
        assert_eq!(store.practice().unwrap().name, "Neu");
    }

    #[test]
    fn rejects_non_letter_templates() {
        let (store, p, _) = setup();
        let err = store.create_letter(LetterInput { patient_id: p.id, template_key: "ptv3-fortfuehrung".into(), episode_id: None, recipient_id: None });
        assert!(err.is_err());
    }

    #[test]
    fn letter_berichte_fill_from_the_record_and_join_the_board() {
        let (store, p, ep) = setup();
        let dx = vec![Diagnosis { code: "F33.1".into(), label: "Rezidivierende depressive Störung".into(), certainty: "G".into() }];
        store.set_episode_diagnoses(&ep.id, dx).unwrap();
        store.conn.execute("UPDATE patient SET pension_no = '12 030485 B 123' WHERE id = ?1", [&p.id]).unwrap();
        for key in ["drv-befund", "pflege-stellungnahme", "versorgungsamt-befund", "arztbrief"] {
            let l = store.create_letter(LetterInput { patient_id: p.id.clone(), template_key: key.into(), episode_id: None, recipient_id: None }).unwrap();
            assert!(l.subject.contains("Frau Anna Beispiel, geb. 03.04.1985"), "{key}: {}", l.subject);
            let report = store.report(&l.report_id).unwrap();
            let first = &report.sections.iter().find(|s| s.key == "anlass" || s.key == "bezug").unwrap().body;
            assert!(first.contains("in meiner ambulanten psychotherapeutischen Behandlung"), "{key}: {first}");
            let diagnoses = report.sections.iter().find(|s| s.key == "diagnosen").unwrap();
            assert!(diagnoses.body.contains("F33.1"), "{key}: diagnoses of the case");
        }
        let letters = store.list_letters(None).unwrap();
        let drv = letters.iter().find(|l| l.template_key == "drv-befund").unwrap();
        let letter = store.letter(&drv.report_id).unwrap();
        assert_eq!(letter.refs, vec![RefLine { label: "Versicherungsnummer".into(), value: "12 030485 B 123".into() }]);
        let amt = letters.iter().find(|l| l.template_key == "versorgungsamt-befund").unwrap();
        let body = &store.report(&amt.report_id).unwrap().sections.iter().find(|s| s.key == "bezug").unwrap().body.clone();
        assert!(body.contains("{Datum des Anschreibens}") && body.contains(", die sich seit dem"), "unknown placeholder stays: {body}");

        // The board lists the four Berichte with their titles; a Bescheinigung stays in Briefe.
        store.create_letter(LetterInput { patient_id: p.id.clone(), template_key: "bescheinigung".into(), episode_id: None, recipient_id: None }).unwrap();
        let board = store.list_reports(None, &now()).unwrap();
        assert_eq!(board.len(), 4);
        assert!(board.iter().all(|r| r.title.is_some() && r.kind.is_none()));
        assert!(board.iter().any(|r| r.title.as_deref() == Some("Befundbericht Rentenversicherung")));
    }

    #[test]
    fn release_of_information_per_recipient_or_kind() {
        let (store, p, _) = setup();
        let drv = store
            .create_recipient(RecipientInput {
                kind: RecipientKind::Rentenversicherung,
                name: "Deutsche Rentenversicherung Bund".into(),
                department: String::new(),
                lines: vec!["10704 Berlin".into()],
                salutation: None,
                default_template: None,
            })
            .unwrap();
        let l = store
            .create_letter(LetterInput { patient_id: p.id.clone(), template_key: "drv-befund".into(), episode_id: None, recipient_id: Some(drv.id.clone()) })
            .unwrap();
        let template = store.template("drv-befund").unwrap();
        let check = |store: &Store| {
            let l = store.letter(&l.report_id).unwrap();
            letter::release_check(&l, &template, store.letter_release(&l).unwrap().as_ref()).unwrap()
        };
        let missing = check(&store);
        assert!(!missing.ok && missing.severity == "warn");
        assert_eq!(missing.message, "Keine Schweigepflichtentbindung für Deutsche Rentenversicherung Bund erfasst");

        assert!(store.grant_release(&p.id, ReleaseInput::default()).is_err(), "for whom?");
        assert!(store.grant_consent(&p.id, ConsentKind::ReleaseOfInformation, None).is_err());
        // For every Pflegekasse: doesn't cover the DRV.
        store.grant_release(&p.id, ReleaseInput { scope: Some(RecipientKind::Pflegekasse), ..Default::default() }).unwrap();
        assert!(!check(&store).ok);
        let by_kind = store
            .grant_release(&p.id, ReleaseInput { scope: Some(RecipientKind::Rentenversicherung), granted_on: Some("2026-10-02".into()), ..Default::default() })
            .unwrap();
        assert_eq!(by_kind.granted_at, "2026-10-02T12:00:00Z");
        let found = check(&store);
        assert!(found.ok, "{}", found.message);
        assert_eq!(found.message, "Schweigepflichtentbindung für Deutsche Rentenversicherung Bund liegt vor (vom 02.10.2026)");
        assert!(store.grant_release(&p.id, ReleaseInput { scope: Some(RecipientKind::Rentenversicherung), ..Default::default() }).is_err(), "twice");
        // One for the entry itself wins over the kind; the same one twice is refused.
        let own = store.grant_release(&p.id, ReleaseInput { recipient_id: Some(drv.id.clone()), note: Some(" Scan in Akte ".into()), ..Default::default() }).unwrap();
        assert_eq!((own.scope.as_deref(), own.note.as_deref()), (None, Some("Scan in Akte")));
        assert!(store.grant_release(&p.id, ReleaseInput { recipient_id: Some(drv.id.clone()), ..Default::default() }).is_err());
        assert_eq!(store.letter_release(&store.letter(&l.report_id).unwrap()).unwrap().unwrap().id, own.id);
        store.revoke_consent(&own.id).unwrap();
        store.revoke_consent(&by_kind.id).unwrap();
        assert!(!check(&store).ok, "revoked ones don't count");
        assert!(!store.patient(&p.id).unwrap().consent_recording, "no recording consent from a release");
        // Short letters to the patient need none.
        let b = store.create_letter(LetterInput { patient_id: p.id.clone(), template_key: "bescheinigung".into(), episode_id: None, recipient_id: Some(drv.id) }).unwrap();
        let bt = store.template("bescheinigung").unwrap();
        assert!(letter::release_check(&store.letter(&b.report_id).unwrap(), &bt, None).is_none());
    }

    #[test]
    fn request_questions_become_sections_and_template_sections_give_way() {
        let (store, p, _) = setup();
        let l = store.create_letter(LetterInput { patient_id: p.id.clone(), template_key: "versorgungsamt-befund".into(), episode_id: None, recipient_id: None }).unwrap();
        let id = l.report_id.clone();
        let l = store.save_letter_request(&id, "  In der Angelegenheit von Frau Anna Beispiel bitten wir bis zum 15.12.2026 um ...  ").unwrap();
        assert!(l.request_text.starts_with("In der Angelegenheit"));
        assert_eq!(crate::compose::deadline(&l.request_text).as_deref(), Some("2026-12-15"));
        let l = store
            .set_letter_questions(&id, &["Datum der letzten Behandlung".into(), " Depressionen  (Art, Dauer) ".into(), "Datum der letzten Behandlung".into(), "".into()])
            .unwrap();
        assert_eq!(l.questions.iter().map(|q| q.text.as_str()).collect::<Vec<_>>(), ["Datum der letzten Behandlung", "Depressionen (Art, Dauer)"]);

        let sections = vec![
            RequestSection { key: None, heading: "Datum der letzten Behandlung".into(), question: Some(0) },
            RequestSection { key: None, heading: "Depressionen (Art, Dauer)".into(), question: Some(1) },
        ];
        let l = store.set_request_sections(&id, &sections, false).unwrap();
        assert_eq!(l.questions[0].section_key.as_deref(), Some("frage-1"));
        let report = store.report(&id).unwrap();
        let t = letter::effective_template(store.template("versorgungsamt-befund").unwrap(), &report.sections);
        let keys: Vec<&str> = t.sections.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(keys, ["bezug", "diagnosen", "frage-1", "frage-2"], "opening and Diagnosen stay, empty template sections give way");
        assert_eq!(t.sections[2].heading, "Datum der letzten Behandlung");
        assert!(crate::draft::target_sections(&t).iter().any(|d| d.key == "frage-2"), "the AI may write request sections");
        let checks = letter::checklist(&l, &report, &t);
        assert!(checks.iter().any(|c| c.key == "letter_questions" && !c.ok && c.message.contains("2 Fragen")));

        // Answer one, rename it, drop the other; then bring the template back.
        store.save_section(&id, "frage-1", "<p>Zuletzt am 03.11.2026.</p>").unwrap();
        let l = store.set_letter_questions(&id, &["Datum der letzten Behandlung".into(), "Depressionen (Art, Dauer)".into()]).unwrap();
        assert_eq!(l.questions[0].section_key.as_deref(), Some("frage-1"), "same text keeps its section");
        let renamed = vec![RequestSection { key: Some("frage-1".into()), heading: "Letzte Behandlung".into(), question: Some(0) }];
        let l = store.set_request_sections(&id, &renamed, true).unwrap();
        let report = store.report(&id).unwrap();
        assert_eq!(report.sections.iter().find(|s| s.key == "frage-1").unwrap().body, "<p>Zuletzt am 03.11.2026.</p>");
        assert!(!report.sections.iter().any(|s| s.key == "frage-2"));
        let t = letter::effective_template(store.template("versorgungsamt-befund").unwrap(), &report.sections);
        assert_eq!(t.sections.len(), 9, "all eight template sections are back, plus the request section");
        assert_eq!(t.sections[3].heading, "Letzte Behandlung", "after the Diagnosen");
        assert!(letter::checklist(&l, &report, &t).iter().any(|c| c.key == "letter_questions" && !c.ok && c.message.contains("1 Frage")));
        assert!(store.set_request_sections(&id, &[RequestSection { key: Some("frage-9".into()), heading: "x".into(), question: None }], true).is_err());
        assert!(store.set_request_sections(&id, &[RequestSection { key: None, heading: " ".into(), question: None }], true).is_err());
        // No sections from the request: the letter is the template again.
        store.set_request_sections(&id, &[], false).unwrap();
        let report = store.report(&id).unwrap();
        assert_eq!(letter::effective_template(store.template("versorgungsamt-befund").unwrap(), &report.sections).sections.len(), 8);

        let l = store.set_letter_material(&id, Material { facts: false, ..Default::default() }).unwrap();
        assert!(!l.material.facts && l.material.answers && l.material.prior);
        store.sign_report(&id, "T").unwrap();
        assert!(store.save_letter_request(&id, "neu").is_err(), "signed letters are read-only");
        assert!(store.set_letter_questions(&id, &[]).is_err());
        assert!(store.set_letter_material(&id, Material::default()).is_err());
        let audit = crate::audit::list(store.conn(), 500).unwrap();
        assert!(audit.iter().any(|e| e.action == "letter.questions"));
        assert!(audit.iter().all(|e| !e.meta_json.contains("Behandlung") && !e.meta_json.contains("Beispiel")), "no request text in the audit log");
    }

    #[test]
    fn letter_drafts_keep_unbacked_sentences_marked_until_she_decides() {
        use crate::draft::{DraftSentence, Parsed, SectionResult};
        let (store, p, _) = setup();
        let l = store.create_letter(LetterInput { patient_id: p.id.clone(), template_key: "arztbrief".into(), episode_id: None, recipient_id: None }).unwrap();
        let id = l.report_id;
        let key = store.report(&id).unwrap().sections.iter().find(|s| s.body.is_empty()).unwrap().key.clone();
        let meta = DraftMeta { provider: "local".into(), model: "m".into(), prompt_hash: "h".into(), instructed: true, session_ids: None, source: DraftSource::Letter };
        let run = vec![SectionResult {
            key: key.clone(),
            parsed: Parsed {
                sentences: vec![
                    DraftSentence { text: "Die Behandlung findet wöchentlich statt.".into(), facts: vec![] },
                    DraftSentence { text: "Die Prognose ist günstig.".into(), facts: vec![] },
                ],
                dropped: 0,
                unsupported: vec![1],
            },
            with_existing: false,
        }];
        let drafts = store.save_drafts(&id, &meta, &run, &[], &[], &[]).unwrap();
        assert_eq!(drafts[0].source, DraftSource::Letter);
        assert_eq!(drafts[0].sentences.iter().map(|s| s.unsupported).collect::<Vec<_>>(), [false, true]);
        let report = store.apply_draft(&id, &drafts[0].id, ApplyMode::Replace).unwrap();
        let body = &report.sections.iter().find(|s| s.key == key).unwrap().body;
        assert_eq!(body, "<p>Die Behandlung findet wöchentlich statt. <mark data-ohne-beleg=\"\">Die Prognose ist günstig.</mark></p>");
        let t = store.template("arztbrief").unwrap();
        let check = |r: &Report| letter::checklist(&store.letter(&id).unwrap(), r, &t).into_iter().find(|c| c.key == "letter_unsupported");
        let item = check(&report).unwrap();
        assert_eq!((item.ok, item.severity.as_str(), item.section_key.as_deref()), (false, "block", Some(key.as_str())));
        // Kept: the mark goes, the sentence stays.
        store.save_section(&id, &key, "<p>Die Behandlung findet wöchentlich statt. Die Prognose ist günstig.</p>").unwrap();
        assert!(check(&store.report(&id).unwrap()).is_none());
        // Werkstatt drafts still work on a letter.
        assert!(store.save_drafts(&id, &DraftMeta { source: DraftSource::Dictation, ..meta.clone() }, &run, &[], &[], &[]).is_ok());
    }
}
