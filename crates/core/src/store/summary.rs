//! Session summaries (slice 30): one per session, replaced by each run, edited in place.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

use super::{Store, ACTOR};
use crate::audit::{self, Entry};
use crate::domain::*;
use crate::summary::{self, NoteSection, SummaryPoint, MAX_POINT_CHARS};
use crate::templates;
use crate::{Error, Result};

/// Most points the therapist can keep after editing (the model writes at most 6).
pub const MAX_EDITED_POINTS: usize = 12;

/// A key point as shown: its cited transcript lines resolved, note sections by key.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SummaryPointView {
    pub text: String,
    pub citations: Vec<FactCitation>,
    pub note: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionSummary {
    pub session_id: String,
    pub points: Vec<SummaryPointView>,
    /// `draft` as the model wrote it, `edited` once the therapist changed it.
    pub status: String,
    pub provider: String,
    pub model: String,
    pub created_at: String,
    pub updated_at: String,
}

/// What a summary run reads: the patient, the confirmed transcript and the note's sections.
pub struct SummaryInput {
    pub patient: Patient,
    pub segments: Vec<SourceSegment>,
    pub note: Vec<NoteSection>,
}

pub struct SummaryMeta {
    pub provider: String,
    pub model: String,
    pub prompt_hash: String,
    pub chunks: usize,
    pub dropped: usize,
}

impl Store {
    pub fn summary_input(&self, session_id: &str) -> Result<SummaryInput> {
        let session = self.session(session_id)?;
        if session.status == "cancelled" {
            return Err(Error::Invalid("Die Sitzung ist abgesagt".into()));
        }
        let patient = self.patient(&session.patient_id)?;
        let segments = self.source_segments(session_id)?;
        let note = match &session.note_id {
            Some(id) => {
                let report = self.report(id)?;
                let mut template = self.template(&report.template_key)?;
                if let Some(ep) = &session.episode_id {
                    let ep = self.episode(ep)?;
                    template = template.resolve(&templates::Context { method: ep.method, age_group: ep.age_group, kind: None });
                }
                summary::note_sections(&template, &report)
            }
            None => Vec::new(),
        };
        Ok(SummaryInput { patient, segments, note })
    }

    pub fn session_summary(&self, session_id: &str) -> Result<Option<SessionSummary>> {
        let row = self
            .conn
            .query_row(
                "SELECT points_json, status, provider, model, created_at, updated_at FROM session_summary WHERE session_id = ?1",
                [session_id],
                |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .optional()?;
        let Some((points, status, provider, model, created_at, updated_at)) = row else { return Ok(None) };
        let points: Vec<SummaryPoint> = serde_json::from_str(&points)?;
        let segments = self.source_segments(session_id)?;
        let points = points
            .into_iter()
            .map(|p| SummaryPointView {
                citations: p
                    .segments
                    .iter()
                    .filter_map(|id| segments.iter().find(|s| &s.segment_id == id))
                    .map(|s| FactCitation { segment_id: s.segment_id.clone(), source_id: s.source_id.clone(), start_ms: s.start_ms, text: s.text.clone() })
                    .collect(),
                text: p.text,
                note: p.note,
            })
            .collect();
        Ok(Some(SessionSummary { session_id: session_id.to_string(), points, status, provider, model, created_at, updated_at }))
    }

    /// Stores a run's points, replacing any earlier summary (edited or not). Audit without text.
    pub fn save_summary(&self, session_id: &str, meta: &SummaryMeta, points: &[SummaryPoint]) -> Result<SessionSummary> {
        let session = self.session(session_id)?;
        let tx = self.conn.unchecked_transaction()?;
        let ts = now();
        tx.execute(
            "INSERT INTO session_summary (session_id, points_json, status, provider, model, prompt_version, prompt_hash, created_at, updated_at)
             VALUES (?1, ?2, 'draft', ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT(session_id) DO UPDATE SET points_json = ?2, status = 'draft', provider = ?3, model = ?4,
               prompt_version = ?5, prompt_hash = ?6, created_at = ?7, updated_at = ?7",
            params![session_id, serde_json::to_string(points)?, meta.provider, meta.model, summary::PROMPT_VERSION, meta.prompt_hash, ts],
        )?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "summary.create", entity_type: "session", entity_id: Some(session_id),
            meta: json!({
                "patient_id": session.patient_id, "provider": meta.provider, "model": meta.model,
                "prompt_version": summary::PROMPT_VERSION, "prompt_hash": meta.prompt_hash,
                "chunks": meta.chunks, "points": points.len(), "dropped": meta.dropped,
            }),
        })?;
        tx.commit()?;
        self.session_summary(session_id)?.ok_or_else(|| Error::NotFound(format!("summary {session_id}")))
    }

    /// The therapist's version: empty points go, citations must belong to this session.
    pub fn edit_summary(&self, session_id: &str, points: Vec<SummaryPoint>) -> Result<SessionSummary> {
        if self.session_summary(session_id)?.is_none() {
            return Err(Error::NotFound(format!("summary {session_id}")));
        }
        let known: Vec<String> = self.source_segments(session_id)?.into_iter().map(|s| s.segment_id).collect();
        let mut clean = Vec::new();
        for p in points {
            let text = p.text.split_whitespace().collect::<Vec<_>>().join(" ");
            if text.is_empty() {
                continue;
            }
            if text.chars().count() > MAX_POINT_CHARS * 2 {
                return Err(Error::Invalid("Ein Kernpunkt ist zu lang".into()));
            }
            let segments = p.segments.into_iter().filter(|s| known.contains(s)).collect();
            clean.push(SummaryPoint { text, segments, note: p.note });
        }
        if clean.len() > MAX_EDITED_POINTS {
            return Err(Error::Invalid(format!("Höchstens {MAX_EDITED_POINTS} Kernpunkte")));
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE session_summary SET points_json = ?2, status = 'edited', updated_at = ?3 WHERE session_id = ?1",
            params![session_id, serde_json::to_string(&clean)?, now()],
        )?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "summary.edit", entity_type: "session", entity_id: Some(session_id),
            meta: json!({ "points": clean.len() }),
        })?;
        tx.commit()?;
        self.session_summary(session_id)?.ok_or_else(|| Error::NotFound(format!("summary {session_id}")))
    }

    /// The newest summaries of a patient's sessions (newest first), with the session time; only
    /// sessions before `before` (a session id) when given.
    pub fn recent_summaries(&self, patient_id: &str, before: Option<&str>, limit: usize) -> Result<Vec<(Option<String>, SessionSummary)>> {
        let cutoff = match before {
            Some(id) => {
                let s = self.session(id)?;
                Some(s.scheduled_at.or(s.started_at).unwrap_or_else(now))
            }
            None => None,
        };
        let rows: Vec<(String, Option<String>)> = self
            .conn
            .prepare(
                "SELECT s.id, COALESCE(s.scheduled_at, s.started_at) AS at FROM session_summary x JOIN session s ON s.id = x.session_id
                 WHERE s.patient_id = ?1 AND s.status <> 'cancelled' AND s.id IS NOT ?2 AND (?3 IS NULL OR at < ?3)
                 ORDER BY at DESC LIMIT ?4",
            )?
            .query_map(params![patient_id, before, cutoff, limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut out = Vec::new();
        for (id, at) in rows {
            if let Some(s) = self.session_summary(&id)? {
                out.push((at, s));
            }
        }
        Ok(out)
    }
}
