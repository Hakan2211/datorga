//! Bericht-Lotse "Fragen" (slice 38): the therapist's answers per Bericht and the last AI check
//! of which questions the session facts already answer.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

use super::{Store, ACTOR};
use crate::audit::{self, Entry};
use crate::domain::*;
use crate::gaps::MAX_ANSWER_CHARS;
use crate::{Error, Result};

/// A question the check found answered, with the facts (ids) that answer it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CoveredAsk {
    pub section: String,
    pub ask: String,
    pub facts: Vec<String>,
}

/// The last coverage check of a Bericht.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GapCheck {
    pub provider: String,
    pub model: String,
    pub facts_digest: String,
    pub covered: Vec<CoveredAsk>,
    pub checked_at: String,
}

/// Provenance of one coverage check.
pub struct GapCheckMeta<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub prompt_hash: &'a str,
    pub facts_digest: &'a str,
    /// Questions sent.
    pub asked: usize,
    /// Entries of the answer that failed the checks.
    pub dropped: usize,
}

fn row_to_answer(r: &rusqlite::Row<'_>) -> rusqlite::Result<ReportAnswer> {
    Ok(ReportAnswer {
        id: r.get(0)?,
        section_key: r.get(1)?,
        ask_key: r.get(2)?,
        question: r.get(3)?,
        text: r.get(4)?,
        source: super::parse_col(r, 5)?,
        updated_at: r.get(6)?,
    })
}

const ANSWER_COLS: &str = "id, section_key, ask_key, question, text, source, updated_at";

impl Store {
    pub fn report_answers(&self, report_id: &str) -> Result<Vec<ReportAnswer>> {
        let mut stmt = self.conn.prepare(&format!("SELECT {ANSWER_COLS} FROM report_answer WHERE report_id = ?1 ORDER BY created_at, rowid"))?;
        let rows = stmt.query_map([report_id], row_to_answer)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Sets her answer to one question (the caller checks it is one of the template's); an empty
    /// text removes it. The audit entry has no text.
    pub fn set_report_answer(
        &self,
        report_id: &str,
        section_key: &str,
        ask_key: &str,
        question: &str,
        text: &str,
        source: AnswerSource,
    ) -> Result<Option<ReportAnswer>> {
        let text = text.trim();
        if text.chars().count() > MAX_ANSWER_CHARS {
            return Err(Error::Invalid(format!("Die Antwort ist zu lang (höchstens {MAX_ANSWER_CHARS} Zeichen)")));
        }
        let tx = self.conn.unchecked_transaction()?;
        let status: String = tx.query_row("SELECT status FROM report WHERE id = ?1", [report_id], |r| r.get(0))?;
        if status == ReportStatus::Signed.as_str() || status == ReportStatus::Exported.as_str() {
            return Err(Error::Invalid("signed reports are read-only".into()));
        }
        let ts = now();
        if text.is_empty() {
            tx.execute(
                "DELETE FROM report_answer WHERE report_id = ?1 AND section_key = ?2 AND ask_key = ?3",
                params![report_id, section_key, ask_key],
            )?;
        } else {
            tx.execute(
                "INSERT INTO report_answer (id, report_id, section_key, ask_key, question, text, source, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
                 ON CONFLICT (report_id, section_key, ask_key)
                 DO UPDATE SET question = excluded.question, text = excluded.text, source = excluded.source, updated_at = excluded.updated_at",
                params![new_id(), report_id, section_key, ask_key, question, text, source.as_str(), ts],
            )?;
        }
        audit::append(&tx, Entry {
            actor: ACTOR, action: "report.answer", entity_type: "report", entity_id: Some(report_id),
            meta: json!({ "section": section_key, "ask": ask_key, "source": source.as_str(), "cleared": text.is_empty() }),
        })?;
        let answer = tx
            .query_row(
                &format!("SELECT {ANSWER_COLS} FROM report_answer WHERE report_id = ?1 AND section_key = ?2 AND ask_key = ?3"),
                params![report_id, section_key, ask_key],
                row_to_answer,
            )
            .optional()?;
        tx.commit()?;
        Ok(answer)
    }

    pub fn gap_check(&self, report_id: &str) -> Result<Option<GapCheck>> {
        let row = self
            .conn
            .query_row(
                "SELECT provider, model, facts_digest, covered, checked_at FROM report_gap_check WHERE report_id = ?1",
                [report_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?)),
            )
            .optional()?;
        row.map(|(provider, model, facts_digest, covered, checked_at)| {
            Ok(GapCheck { provider, model, facts_digest, covered: serde_json::from_str(&covered)?, checked_at })
        })
        .transpose()
    }

    /// Replaces the Bericht's coverage check.
    pub fn save_gap_check(&self, report_id: &str, meta: &GapCheckMeta, covered: &[CoveredAsk]) -> Result<GapCheck> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO report_gap_check (report_id, provider, model, prompt_hash, facts_digest, covered, checked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (report_id) DO UPDATE SET provider = excluded.provider, model = excluded.model,
                prompt_hash = excluded.prompt_hash, facts_digest = excluded.facts_digest, covered = excluded.covered,
                checked_at = excluded.checked_at",
            params![report_id, meta.provider, meta.model, meta.prompt_hash, meta.facts_digest, serde_json::to_string(covered)?, now()],
        )?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "report.gaps", entity_type: "report", entity_id: Some(report_id),
            meta: json!({
                "provider": meta.provider, "model": meta.model,
                "prompt_version": crate::gaps::PROMPT_VERSION, "prompt_hash": meta.prompt_hash,
                "asked": meta.asked, "covered": covered.len(), "dropped": meta.dropped,
            }),
        })?;
        tx.commit()?;
        self.gap_check(report_id)?.ok_or_else(|| Error::NotFound("gap check".into()))
    }
}
