//! Bericht-Lotse "Abschnitte" (slice 39): proposals taken over sentence by sentence, the Bericht
//! sections a fact belongs to changed after it was applied, and `in_review` once every section is
//! done.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

use super::{escape_html, section_filled, Store, ACTOR};
use crate::audit::{self, Entry};
use crate::domain::*;
use crate::{Error, Result};

/// Longest sentence she may take over in her own wording.
const MAX_SENTENCE_CHARS: usize = 2000;

/// Her decision on one sentence; `text` is her wording when she edited it before taking it over.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SentenceChoice {
    pub sentence_id: String,
    pub accept: bool,
    #[ts(optional)]
    pub text: Option<String>,
}

impl Store {
    /// Takes over or discards sentences of a proposed draft. A taken-over sentence goes straight
    /// to the end of the section: into the paragraph of the draft's previous one, or a new
    /// paragraph after her own text. Once every sentence is decided the draft counts as applied
    /// (appended), or as rejected when none was taken over.
    pub fn decide_sentences(&self, report_id: &str, draft_id: &str, choices: &[SentenceChoice]) -> Result<Report> {
        self.editable_for_drafts(report_id, None)?;
        if choices.is_empty() {
            return Err(Error::Invalid("Kein Satz gewählt".into()));
        }
        let draft = self
            .section_drafts(report_id)?
            .into_iter()
            .find(|d| d.id == draft_id && d.status == DraftStatus::Proposed)
            .ok_or_else(|| Error::Invalid("Der Entwurf ist nicht mehr offen".into()))?;
        let (mut body, mut origin): (String, SectionOrigin) = self.conn.query_row(
            "SELECT body, origin FROM report_section WHERE report_id = ?1 AND key = ?2",
            params![report_id, draft.section_key],
            |r| Ok((r.get(0)?, super::parse_col(r, 1)?)),
        )?;
        let mut decided: Vec<Option<SentenceDecision>> = draft.sentences.iter().map(|s| s.decision).collect();
        // A sentence taken over earlier from this draft: the next one continues its paragraph.
        let mut continues = decided.contains(&Some(SentenceDecision::Accepted));
        let ts = now();
        let tx = self.conn.unchecked_transaction()?;
        let (mut accepted, mut rejected, mut edited) = (0, 0, 0);
        for c in choices {
            let i = draft
                .sentences
                .iter()
                .position(|s| s.id == c.sentence_id)
                .ok_or_else(|| Error::NotFound(format!("Satz {}", c.sentence_id)))?;
            if decided[i].is_some() {
                return Err(Error::Invalid("Über den Satz ist schon entschieden".into()));
            }
            if !c.accept {
                decided[i] = Some(SentenceDecision::Rejected);
                rejected += 1;
                tx.execute("UPDATE draft_sentence SET decision = 'rejected' WHERE id = ?1", [&c.sentence_id])?;
                continue;
            }
            let original = &draft.sentences[i].text;
            let text = match &c.text {
                Some(t) => t.split_whitespace().collect::<Vec<_>>().join(" "),
                None => original.clone(),
            };
            if text.is_empty() {
                return Err(Error::Invalid("Der Satz ist leer".into()));
            }
            if text.chars().count() > MAX_SENTENCE_CHARS {
                return Err(Error::Invalid("Der Satz ist zu lang".into()));
            }
            let own = &text != original;
            let was_empty = crate::pii::plain_text(&body).trim().is_empty();
            body = append_sentence(&body, &text, continues);
            origin = if was_empty && !own {
                SectionOrigin::Ai
            } else if origin == SectionOrigin::Ai && !own {
                SectionOrigin::Ai
            } else {
                SectionOrigin::Mixed
            };
            continues = true;
            decided[i] = Some(SentenceDecision::Accepted);
            accepted += 1;
            edited += own as usize;
            tx.execute("UPDATE draft_sentence SET decision = 'accepted', edited = ?2 WHERE id = ?1", params![c.sentence_id, own])?;
        }
        if accepted > 0 {
            tx.execute(
                "UPDATE report_section SET body = ?3, origin = ?4, edited_at = ?5 WHERE report_id = ?1 AND key = ?2",
                params![report_id, draft.section_key, body, origin.as_str(), ts],
            )?;
            let (prompt_version, prompt_hash): (String, String) =
                tx.query_row("SELECT prompt_version, prompt_hash FROM section_draft WHERE id = ?1", [draft_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            tx.execute(
                "UPDATE report SET provider = ?2, model = ?3, prompt_version = ?4, prompt_hash = ?5 WHERE id = ?1",
                params![report_id, draft.provider, draft.model, prompt_version, prompt_hash],
            )?;
        }
        let finished = decided.iter().all(Option::is_some);
        if finished {
            let any = decided.contains(&Some(SentenceDecision::Accepted));
            tx.execute(
                "UPDATE section_draft SET status = ?2, mode = ?3, applied_at = ?4 WHERE id = ?1",
                params![
                    draft_id,
                    if any { DraftStatus::Applied } else { DraftStatus::Rejected }.as_str(),
                    any.then_some(ApplyMode::Append.as_str()),
                    any.then_some(&ts),
                ],
            )?;
        }
        tx.execute("UPDATE report SET updated_at = ?2 WHERE id = ?1", params![report_id, ts])?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "report.draft_decide", entity_type: "report", entity_id: Some(report_id),
            meta: json!({
                "draft_id": draft_id, "section": draft.section_key,
                "accepted": accepted, "rejected": rejected, "edited": edited, "finished": finished,
            }),
        })?;
        sync_review(&tx, report_id)?;
        tx.commit()?;
        self.report(report_id)
    }

    /// The Bericht sections an applied fact belongs to, set by the therapist (also after the
    /// fact was applied, and on a signed note: the tags aren't part of its text). Tags she keeps
    /// keep their source; new ones are hers.
    pub fn set_fact_sections(&self, fact_id: &str, keys: &[String]) -> Result<()> {
        let mut keys: Vec<String> = keys.iter().map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect();
        keys.sort();
        keys.dedup();
        if keys.len() > 10 || keys.iter().any(|k| !k.chars().all(|c| c.is_ascii_lowercase() || c == '_')) {
            return Err(Error::Invalid("Ungültige Bericht-Abschnitte".into()));
        }
        let (report_id, status): (String, String) = self
            .conn
            .query_row("SELECT report_id, status FROM note_fact WHERE id = ?1", [fact_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("Fakt {fact_id}")))?;
        if status != FactStatus::Applied.as_str() {
            return Err(Error::Invalid("Nur übernommene Fakten lassen sich zuordnen".into()));
        }
        let before: Vec<(String, String)> = self
            .conn
            .prepare("SELECT section_key, source FROM note_fact_section WHERE fact_id = ?1")?
            .query_map([fact_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM note_fact_section WHERE fact_id = ?1", [fact_id])?;
        for key in &keys {
            let source = before.iter().find(|(k, _)| k == key).map_or("therapist", |(_, s)| s.as_str());
            tx.execute(
                "INSERT INTO note_fact_section (fact_id, section_key, source) VALUES (?1, ?2, ?3)",
                params![fact_id, key, source],
            )?;
        }
        tx.execute("UPDATE note_fact SET bericht_tagged = 1 WHERE id = ?1", [fact_id])?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "note.fact_tags", entity_type: "report", entity_id: Some(&report_id),
            meta: json!({
                "fact_id": fact_id,
                "added": keys.iter().filter(|k| !before.iter().any(|(b, _)| b == *k)).count(),
                "removed": before.iter().filter(|(b, _)| !keys.contains(b)).count(),
            }),
        })?;
        tx.commit()?;
        Ok(())
    }
}

impl Store {
    /// Brings every open Bericht's status in line (at open: databases from before `in_review`
    /// was set, or a template change).
    pub(super) fn sync_review_statuses(&self) -> Result<()> {
        let ids: Vec<String> = self
            .conn
            .prepare("SELECT id FROM report WHERE kind = 'bericht' AND status IN ('draft', 'in_review')")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for id in ids {
            sync_review(&self.conn, &id)?;
        }
        Ok(())
    }
}

/// A sentence at the end of a section: continuing its last paragraph, or as a new one.
fn append_sentence(body: &str, text: &str, continues: bool) -> String {
    let text = escape_html(text);
    let body = body.trim_end();
    if crate::pii::plain_text(body).trim().is_empty() {
        format!("<p>{text}</p>")
    } else if let Some(head) = body.strip_suffix("</p>").filter(|_| continues) {
        format!("{head} {text}</p>")
    } else {
        format!("{body}<p>{text}</p>")
    }
}

/// A Bericht is `in_review` while every section has text or is "nicht relevant" and no proposal
/// waits; otherwise (again) a `draft`. Sitzungsnotizen and signed reports stay as they are.
pub(super) fn sync_review(conn: &Connection, report_id: &str) -> Result<()> {
    let (kind, status): (String, ReportStatus) =
        conn.query_row("SELECT kind, status FROM report WHERE id = ?1", [report_id], |r| Ok((r.get(0)?, super::parse_col(r, 1)?)))?;
    if kind != "bericht" || !matches!(status, ReportStatus::Draft | ReportStatus::InReview) {
        return Ok(());
    }
    let sections: Vec<(String, bool)> = conn
        .prepare("SELECT body, not_relevant FROM report_section WHERE report_id = ?1")?
        .query_map([report_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    // A proposal for a section marked "nicht relevant" doesn't wait for anything.
    let open: i64 = conn.query_row(
        "SELECT COUNT(*) FROM section_draft d JOIN report_section s ON s.report_id = d.report_id AND s.key = d.section_key
         WHERE d.report_id = ?1 AND d.status = 'proposed' AND s.not_relevant = 0",
        [report_id],
        |r| r.get(0),
    )?;
    let done = !sections.is_empty() && open == 0 && sections.iter().all(|(b, nr)| *nr || section_filled(b));
    let next = if done { ReportStatus::InReview } else { ReportStatus::Draft };
    if next != status {
        conn.execute("UPDATE report SET status = ?2 WHERE id = ?1", params![report_id, next.as_str()])?;
        audit::append(conn, Entry {
            actor: ACTOR, action: "report.status", entity_type: "report", entity_id: Some(report_id),
            meta: json!({ "from": status.as_str(), "to": next.as_str() }),
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences_continue_the_drafts_paragraph() {
        assert_eq!(append_sentence("", "A <b>.", false), "<p>A &lt;b&gt;.</p>");
        assert_eq!(append_sentence("<p></p>", "A.", true), "<p>A.</p>");
        assert_eq!(append_sentence("<p>Eigen.</p>", "A.", false), "<p>Eigen.</p><p>A.</p>");
        assert_eq!(append_sentence("<p>A.</p>", "B.", true), "<p>A. B.</p>");
        assert_eq!(append_sentence("<ul><li>x</li></ul>", "B.", true), "<ul><li>x</li></ul><p>B.</p>");
    }
}
