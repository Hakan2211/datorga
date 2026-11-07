//! Briefe (slice 45): letters, the recipient address book, and the practice letterhead.

use std::collections::HashMap;

use rusqlite::{params, OptionalExtension};
use serde_json::json;

use super::{new_id, now, parse_col, row_to_episode, Store, ACTOR, EPISODE_SELECT};
use crate::audit::{self, Entry};
use crate::domain::*;
use crate::letter::{
    self, FillContext, Letter, LetterInput, LetterMeta, LetterRecipient, LetterState, LetterSummary, Practice, PracticeAsset,
    Recipient, RecipientInput, RefLine,
};
use crate::templates::{self, FieldKind, Layout};
use crate::{Error, Result};

const LETTER_SELECT: &str = "SELECT report_id, patient_id, episode_id, recipient_id, recipient_json, salutation, subject,
    refs_json, place, letter_date, sent_at, sent_via, follow_up_on, follow_up_note FROM letter";
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
}
