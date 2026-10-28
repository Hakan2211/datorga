//! The assistant's threads, messages and proposals (slice 25). A proposal is checked when the
//! model creates it and again when the therapist applies it; applying runs the normal store
//! command and is audited as `agent.apply`.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::calendar::{check_date, check_minutes, check_task};
use super::{escape_html, parse_col, Store, ACTOR, PATIENT_SELECT};
use crate::agent::{ToolOutcome, MAX_PROPOSAL_TEXT};
use crate::audit::{self, Entry};
use crate::domain::*;
use crate::pseudo::{Dictionary, TokenMap};
use crate::templates::FieldKind;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextMode {
    Append,
    Replace,
}

/// Text for a section of a Bericht or Sitzungsnotiz. A note proposal names the session; its note
/// is created on apply if there is none yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextProposal {
    pub report_id: Option<String>,
    pub session_id: Option<String>,
    pub section_key: String,
    /// Filled by the check, for the card.
    #[serde(default)]
    pub heading: String,
    pub text: String,
    pub mode: TextMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Reschedule {
    session_id: String,
    update: SessionUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DayPlan {
    date: String,
    text: String,
}

const THREAD_SELECT: &str = "SELECT t.id, t.scope, t.ref_id, t.title, t.created_at, t.updated_at,
    COALESCE(p.first_name || ' ' || p.last_name, sp.first_name || ' ' || sp.last_name)
    FROM chat_thread t
    LEFT JOIN patient p ON t.scope = 'patient' AND p.id = t.ref_id
    LEFT JOIN session s ON t.scope = 'session' AND s.id = t.ref_id
    LEFT JOIN patient sp ON sp.id = s.patient_id";

fn row_to_thread(r: &rusqlite::Row<'_>) -> rusqlite::Result<ChatThread> {
    Ok(ChatThread {
        id: r.get(0)?,
        scope: parse_col(r, 1)?,
        ref_id: r.get(2)?,
        title: r.get(3)?,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
        patient_name: r.get(6)?,
    })
}

const PROPOSAL_SELECT: &str = "SELECT id, thread_id, message_id, kind, patient_id, summary, payload, status, result_id,
    created_at, decided_at FROM agent_proposal";

fn row_to_proposal(r: &rusqlite::Row<'_>) -> rusqlite::Result<AgentProposal> {
    let payload: String = r.get(6)?;
    Ok(AgentProposal {
        id: r.get(0)?,
        thread_id: r.get(1)?,
        message_id: r.get(2)?,
        kind: parse_col(r, 3)?,
        patient_id: r.get(4)?,
        summary: r.get(5)?,
        payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
        status: parse_col(r, 7)?,
        result_id: r.get(8)?,
        created_at: r.get(9)?,
        decided_at: r.get(10)?,
    })
}

/// Plain text (one paragraph per line) as editor HTML.
fn text_html(text: &str) -> String {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(|l| format!("<p>{}</p>", escape_html(l))).collect()
}

fn from_payload<T: serde::de::DeserializeOwned>(payload: &Value) -> Result<T> {
    serde_json::from_value(payload.clone()).map_err(|e| Error::Invalid(format!("Vorschlag unlesbar: {e}")))
}

impl Store {
    // ---------- threads ----------

    pub fn create_chat_thread(&self, scope: ChatScope, ref_id: Option<&str>) -> Result<ChatThread> {
        let patient = match (scope, ref_id) {
            (ChatScope::Global, _) => None,
            (ChatScope::Patient, Some(id)) => Some(self.patient(id)?.id),
            (ChatScope::Session, Some(id)) => Some(self.session(id)?.patient_id),
            _ => return Err(Error::Invalid("Ein Gespräch zu einer Person oder Sitzung braucht deren ID".into())),
        };
        let id = new_id();
        let ts = now();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO chat_thread (id, scope, ref_id, token_map, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id, scope.as_str(), ref_id.filter(|_| scope != ChatScope::Global), serde_json::to_string(&TokenMap::default())?, ts],
        )?;
        if let Some(p) = &patient {
            tx.execute("INSERT OR IGNORE INTO chat_thread_patient (thread_id, patient_id) VALUES (?1, ?2)", params![id, p])?;
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "chat.create", entity_type: "chat_thread", entity_id: Some(&id), meta: json!({ "scope": scope }) })?;
        tx.commit()?;
        self.chat_thread(&id)
    }

    pub fn chat_thread(&self, id: &str) -> Result<ChatThread> {
        self.conn
            .query_row(&format!("{THREAD_SELECT} WHERE t.id = ?1"), [id], row_to_thread)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("chat {id}")))
    }

    /// Newest first. With `scope`, only that scope (and `ref_id`).
    pub fn chat_threads(&self, scope: Option<ChatScope>, ref_id: Option<&str>, limit: usize) -> Result<Vec<ChatThread>> {
        let mut stmt = self.conn.prepare(&format!(
            "{THREAD_SELECT} WHERE (?1 IS NULL OR t.scope = ?1) AND (?2 IS NULL OR t.ref_id = ?2)
             ORDER BY t.updated_at DESC, t.created_at DESC LIMIT ?3"
        ))?;
        let rows = stmt.query_map(params![scope.map(ChatScope::as_str), ref_id, limit as i64], row_to_thread)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn delete_chat_thread(&self, id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if tx.execute("DELETE FROM chat_thread WHERE id = ?1", [id])? == 0 {
            return Err(Error::NotFound(format!("chat {id}")));
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "chat.delete", entity_type: "chat_thread", entity_id: Some(id), meta: json!({}) })?;
        tx.commit()?;
        Ok(())
    }

    pub fn chat_thread_map(&self, id: &str) -> Result<TokenMap> {
        let raw: String = self.conn.query_row("SELECT token_map FROM chat_thread WHERE id = ?1", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Stores the thread's tokens and links every patient whose name is among them.
    pub fn set_chat_thread_map(&self, id: &str, map: &TokenMap) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("UPDATE chat_thread SET token_map = ?2 WHERE id = ?1", params![id, serde_json::to_string(map)?])?;
        let names: Vec<String> = map.entries().iter().map(|e| e.original.to_lowercase()).collect();
        if !names.is_empty() {
            let mut stmt = tx.prepare("SELECT id, first_name, last_name FROM patient")?;
            let patients: Vec<(String, String, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
            drop(stmt);
            for (pid, first, last) in patients {
                let full = format!("{first} {last}").to_lowercase();
                if names.iter().any(|n| *n == last.to_lowercase() || *n == full) {
                    tx.execute("INSERT OR IGNORE INTO chat_thread_patient (thread_id, patient_id) VALUES (?1, ?2)", params![id, pid])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every patient (archived too) and all their terms: the assistant may meet any of them.
    pub fn assistant_dictionary(&self) -> Result<Dictionary> {
        let mut stmt = self.conn.prepare(&format!("{PATIENT_SELECT} ORDER BY created_at"))?;
        let patients: Vec<Patient> = stmt.query_map([], super::row_to_patient)?.collect::<rusqlite::Result<_>>()?;
        let mut dict = Dictionary::new();
        for p in &patients {
            dict.extend(&Dictionary::from_patient(p));
        }
        let mut stmt = self.conn.prepare("SELECT kind, text FROM pseudo_term ORDER BY created_at")?;
        let terms = stmt.query_map([], |r| Ok((super::entity_kind(r, 0)?, r.get::<_, String>(1)?)))?;
        for t in terms {
            let (kind, text) = t?;
            dict.add(kind, &text);
        }
        Ok(dict)
    }

    // ---------- messages ----------

    pub fn chat_messages(&self, thread_id: &str) -> Result<Vec<ChatMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, seq, role, content, tool_calls, tool_call_id, tool_name, created_at FROM chat_message
             WHERE thread_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map([thread_id], |r| {
            let calls: Option<String> = r.get(4)?;
            Ok(ChatMessage {
                id: r.get(0)?,
                seq: r.get(1)?,
                role: parse_col(r, 2)?,
                content: r.get(3)?,
                tool_calls: calls.and_then(|c| serde_json::from_str(&c).ok()).unwrap_or_default(),
                tool_call_id: r.get(5)?,
                tool_name: r.get(6)?,
                created_at: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Appends a message. The first question becomes the thread's title.
    pub fn add_chat_message(&self, thread_id: &str, role: ChatRole, content: &str, calls: &[ChatToolCall], answers: Option<&ChatToolCall>) -> Result<ChatMessage> {
        let tx = self.conn.unchecked_transaction()?;
        let id = insert_message(&tx, thread_id, role, content, calls, answers)?;
        if role == ChatRole::User {
            let flat = content.split_whitespace().collect::<Vec<_>>().join(" ");
            let title = if flat.chars().count() > 60 { format!("{}…", flat.chars().take(59).collect::<String>().trim_end()) } else { flat };
            tx.execute("UPDATE chat_thread SET title = COALESCE(title, ?2) WHERE id = ?1", params![thread_id, title])?;
        }
        tx.commit()?;
        self.chat_messages(thread_id)?.into_iter().find(|m| m.id == id).ok_or_else(|| Error::NotFound(id))
    }

    /// A tool's result, with its proposal if it made one, in one transaction.
    pub fn add_tool_result(&self, thread_id: &str, call: &ChatToolCall, outcome: ToolOutcome) -> Result<Option<AgentProposal>> {
        let tx = self.conn.unchecked_transaction()?;
        let message_id = insert_message(&tx, thread_id, ChatRole::Tool, &outcome.text, &[], Some(call))?;
        for p in &outcome.patients {
            tx.execute("INSERT OR IGNORE INTO chat_thread_patient (thread_id, patient_id) VALUES (?1, ?2)", params![thread_id, p])?;
        }
        let mut proposal_id = None;
        if let Some(p) = outcome.proposal {
            let id = new_id();
            tx.execute(
                "INSERT INTO agent_proposal (id, thread_id, message_id, kind, patient_id, summary, payload, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, thread_id, message_id, p.kind.as_str(), p.patient_id, p.summary, p.payload.to_string(), now()],
            )?;
            audit::append(&tx, Entry {
                actor: ACTOR, action: "agent.propose", entity_type: "agent_proposal", entity_id: Some(&id),
                meta: json!({ "kind": p.kind }),
            })?;
            proposal_id = Some(id);
        }
        tx.commit()?;
        proposal_id.map(|id| self.proposal(&id)).transpose()
    }

    // ---------- proposals ----------

    pub fn proposal(&self, id: &str) -> Result<AgentProposal> {
        self.conn
            .query_row(&format!("{PROPOSAL_SELECT} WHERE id = ?1"), [id], row_to_proposal)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("proposal {id}")))
    }

    pub fn proposals(&self, thread_id: &str) -> Result<Vec<AgentProposal>> {
        let mut stmt = self.conn.prepare(&format!("{PROPOSAL_SELECT} WHERE thread_id = ?1 ORDER BY created_at, id"))?;
        let rows = stmt.query_map([thread_id], row_to_proposal)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The target section exists, takes prose (never diagnoses) and its document isn't signed;
    /// the text is sane. Fills in the heading.
    pub fn check_text_proposal(&self, mut p: TextProposal) -> Result<TextProposal> {
        let note = match &p.session_id {
            Some(s) => self.session(s)?.note_id.map(|id| self.report(&id)).transpose()?,
            None => None,
        };
        let report = match (&p.report_id, note) {
            (Some(id), _) => Some(self.report(id)?),
            (None, note) => note,
        };
        if p.report_id.is_none() && p.session_id.is_none() {
            return Err(Error::Invalid("Kein Ziel für den Text".into()));
        }
        if let Some(r) = &report {
            if matches!(r.status, ReportStatus::Signed | ReportStatus::Exported) {
                return Err(Error::Invalid("Das Dokument ist unterschrieben und schreibgeschützt; dafür gibt es nur Nachträge".into()));
            }
            if !r.sections.iter().any(|s| s.key == p.section_key) {
                return Err(Error::Invalid(format!("Abschnitt „{}“ gibt es in diesem Dokument nicht", p.section_key)));
            }
        }
        let template = self.template(report.as_ref().map_or("sitzungsnotiz", |r| r.template_key.as_str()))?;
        let def = template
            .sections
            .iter()
            .find(|d| d.key == p.section_key)
            .ok_or_else(|| Error::Invalid(format!("Abschnitt „{}“ gibt es nicht", p.section_key)))?;
        if def.field != FieldKind::Prose || !def.ai_allowed {
            return Err(Error::Invalid("Diagnosen und dieser Abschnitt sind der Therapeutin vorbehalten".into()));
        }
        let text = p.text.trim();
        if text.is_empty() || text.chars().count() > MAX_PROPOSAL_TEXT {
            return Err(Error::Invalid(format!("Der Text muss 1 bis {MAX_PROPOSAL_TEXT} Zeichen lang sein")));
        }
        if crate::extract::ICD.is_match(text) {
            return Err(Error::Invalid("Der Text enthält einen ICD-Code; Diagnosen trägt nur die Therapeutin ein".into()));
        }
        p.text = text.to_string();
        p.heading = def.heading.trim().to_string();
        Ok(p)
    }

    /// Checks a proposal against the current data. Returns the patient it concerns.
    pub fn check_proposal(&self, kind: ProposalKind, payload: &Value) -> Result<Option<String>> {
        match kind {
            ProposalKind::Session => {
                let input: SessionInput = from_payload(payload)?;
                let p = self.patient(&input.patient_id)?;
                if p.archived_at.is_some() {
                    return Err(Error::Invalid("Die Person ist archiviert".into()));
                }
                if let Some(e) = &input.episode_id {
                    if self.episode(e)?.patient_id != p.id {
                        return Err(Error::Invalid("Die Behandlung gehört zu einer anderen Person".into()));
                    }
                }
                super::normalize_ts(input.scheduled_at.as_deref().unwrap_or_default())?;
                check_minutes(input.planned_minutes)?;
                Ok(Some(p.id))
            }
            ProposalKind::Reschedule => {
                let r: Reschedule = from_payload(payload)?;
                let s = self.session(&r.session_id)?;
                if s.status == "cancelled" || !self.session_untouched(&s.id)? {
                    return Err(Error::Invalid("Nur geplante Termine ohne Aufnahme oder Notiz lassen sich verschieben".into()));
                }
                super::normalize_ts(&r.update.scheduled_at)?;
                check_minutes(r.update.planned_minutes)?;
                Ok(Some(s.patient_id))
            }
            ProposalKind::Task => {
                let input: TaskInput = from_payload(payload)?;
                check_task(&input)?;
                if let Some(s) = &input.session_id {
                    self.session(s)?;
                }
                match &input.patient_id {
                    Some(p) => Ok(Some(self.patient(p)?.id)),
                    None => Ok(None),
                }
            }
            ProposalKind::SectionText | ProposalKind::NoteText => {
                let p = self.check_text_proposal(from_payload(payload)?)?;
                let patient = match (&p.report_id, &p.session_id) {
                    (Some(r), _) => self.report(r)?.patient_id,
                    (None, Some(s)) => self.session(s)?.patient_id,
                    _ => unreachable!("checked"),
                };
                Ok(Some(patient))
            }
            ProposalKind::DayPlan => {
                let d: DayPlan = from_payload(payload)?;
                check_date(&d.date)?;
                if d.text.trim().is_empty() || d.text.chars().count() > MAX_PROPOSAL_TEXT {
                    return Err(Error::Invalid(format!("Der Text muss 1 bis {MAX_PROPOSAL_TEXT} Zeichen lang sein")));
                }
                Ok(None)
            }
        }
    }

    fn open_proposal(&self, id: &str) -> Result<AgentProposal> {
        let p = self.proposal(id)?;
        if p.status != ProposalStatus::Open {
            return Err(Error::Invalid("Über diesen Vorschlag ist schon entschieden".into()));
        }
        Ok(p)
    }

    /// "Übernehmen": checks the proposal again and runs the normal store command.
    pub fn apply_proposal(&self, id: &str) -> Result<AgentProposal> {
        let p = self.open_proposal(id)?;
        self.check_proposal(p.kind, &p.payload)?;
        let result_id = match p.kind {
            ProposalKind::Session => self.create_session(from_payload(&p.payload)?)?.id,
            ProposalKind::Reschedule => {
                let r: Reschedule = from_payload(&p.payload)?;
                self.update_session(&r.session_id, r.update)?.id
            }
            ProposalKind::Task => self.create_task(from_payload(&p.payload)?)?.id,
            ProposalKind::SectionText | ProposalKind::NoteText => {
                let t = self.check_text_proposal(from_payload(&p.payload)?)?;
                let report = match (&t.report_id, &t.session_id) {
                    (Some(r), _) => self.report(r)?,
                    (None, Some(s)) => match self.session(s)?.note_id {
                        Some(n) => self.report(&n)?,
                        None => self.create_session_note(s)?,
                    },
                    _ => unreachable!("checked"),
                };
                let section = report.sections.iter().find(|s| s.key == t.section_key).ok_or_else(|| Error::NotFound(t.section_key.clone()))?;
                let had_text = !crate::draft::plain_text(&section.body).trim().is_empty();
                let body = match t.mode {
                    TextMode::Replace => text_html(&t.text),
                    TextMode::Append => format!("{}{}", section.body, text_html(&t.text)),
                };
                self.save_section(&report.id, &t.section_key, &body)?;
                // Provenance like an applied draft: AI text alone is `ai`, next to her own `mixed`.
                let origin = if t.mode == TextMode::Replace || !had_text {
                    SectionOrigin::Ai
                } else if section.origin == SectionOrigin::Human {
                    SectionOrigin::Mixed
                } else {
                    section.origin
                };
                self.conn.execute(
                    "UPDATE report_section SET origin = ?3 WHERE report_id = ?1 AND key = ?2",
                    params![report.id, t.section_key, origin.as_str()],
                )?;
                report.id
            }
            ProposalKind::DayPlan => {
                let d: DayPlan = from_payload(&p.payload)?;
                let body = match self.day_note(&d.date)? {
                    Some(old) if !old.trim().is_empty() => format!("{}\n\n{}", old.trim_end(), d.text.trim()),
                    _ => d.text.trim().to_string(),
                };
                self.set_day_note(&d.date, &body)?;
                d.date
            }
        };
        self.decide(&p, ProposalStatus::Applied, Some(&result_id))
    }

    pub fn discard_proposal(&self, id: &str) -> Result<AgentProposal> {
        let p = self.open_proposal(id)?;
        self.decide(&p, ProposalStatus::Discarded, None)
    }

    fn decide(&self, p: &AgentProposal, status: ProposalStatus, result_id: Option<&str>) -> Result<AgentProposal> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE agent_proposal SET status = ?2, result_id = ?3, decided_at = ?4 WHERE id = ?1",
            params![p.id, status.as_str(), result_id, now()],
        )?;
        let action = if status == ProposalStatus::Applied { "agent.apply" } else { "agent.discard" };
        audit::append(&tx, Entry { actor: ACTOR, action, entity_type: "agent_proposal", entity_id: Some(&p.id), meta: json!({ "kind": p.kind }) })?;
        tx.commit()?;
        self.proposal(&p.id)
    }

    /// Threads that touched a patient go with the patient (Art. 17): their text may name them.
    pub(super) fn erase_chat_threads(tx: &rusqlite::Transaction<'_>, patient_id: &str) -> Result<usize> {
        Ok(tx.execute(
            "DELETE FROM chat_thread WHERE id IN (SELECT thread_id FROM chat_thread_patient WHERE patient_id = ?1)
                OR (scope = 'patient' AND ref_id = ?1)
                OR (scope = 'session' AND ref_id IN (SELECT id FROM session WHERE patient_id = ?1))",
            [patient_id],
        )?)
    }
}

fn insert_message(
    tx: &rusqlite::Transaction<'_>,
    thread_id: &str,
    role: ChatRole,
    content: &str,
    calls: &[ChatToolCall],
    answers: Option<&ChatToolCall>,
) -> Result<String> {
    let id = new_id();
    let ts = now();
    let seq: i64 = tx.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM chat_message WHERE thread_id = ?1", [thread_id], |r| r.get(0))?;
    tx.execute(
        "INSERT INTO chat_message (id, thread_id, seq, role, content, tool_calls, tool_call_id, tool_name, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            thread_id,
            seq,
            role.as_str(),
            content,
            if calls.is_empty() { None } else { Some(serde_json::to_string(calls)?) },
            answers.map(|c| c.id.as_str()),
            answers.map(|c| c.name.as_str()),
            ts
        ],
    )?;
    if tx.execute("UPDATE chat_thread SET updated_at = ?2 WHERE id = ?1", params![thread_id, ts])? == 0 {
        return Err(Error::NotFound(format!("chat {thread_id}")));
    }
    Ok(id)
}
