//! Autopilot jobs and their steps (slice 31). The run itself is `crate::autopilot::run`.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

use super::{row_to_job, Store, ACTOR, JOB_SELECT};
use crate::audit::{self, Entry};
use crate::autopilot::{self, SETTING};
use crate::domain::*;
use crate::extract::Preview;
use crate::llm::ProviderId;
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AutopilotStep {
    pub step: AutopilotStepKind,
    pub status: AutopilotStepStatus,
    /// Why it was skipped or failed (technical, never patient text).
    pub detail: Option<String>,
    /// What was sent can be looked at ([`Store::autopilot_sent`]).
    pub sent: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub updated_at: String,
}

/// The newest autopilot run of a session, for the session page and the fact card.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AutopilotRun {
    pub job: Job,
    pub steps: Vec<AutopilotStep>,
}

impl Store {
    pub fn autopilot_enabled(&self) -> Result<bool> {
        Ok(self.setting(SETTING)?.and_then(|v| v.as_bool()).unwrap_or(true))
    }

    pub fn set_autopilot(&self, on: bool) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO app_setting (key, value_json) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            params![SETTING, json!(on).to_string()],
        )?;
        audit::append(&tx, Entry { actor: ACTOR, action: "settings.autopilot", entity_type: "setting", entity_id: None, meta: json!({ "on": on }) })?;
        tx.commit()?;
        Ok(())
    }

    /// Queues the trigger's steps for a session when autopilot is on (`None` when off). A run
    /// still queued for the session takes the steps instead of a second job.
    pub fn enqueue_autopilot(&self, session_id: &str, trigger: AutopilotTrigger) -> Result<Option<Job>> {
        if !self.autopilot_enabled()? {
            return Ok(None);
        }
        if self.session(session_id)?.status == "cancelled" {
            return Ok(None);
        }
        let queued: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM job WHERE session_id = ?1 AND kind = 'autopilot' AND status = 'queued' ORDER BY created_at DESC LIMIT 1",
                [session_id],
                |r| r.get(0),
            )
            .optional()?;
        let ts = now();
        let tx = self.conn.unchecked_transaction()?;
        let id = match queued {
            Some(id) => id,
            None => {
                let id = new_id();
                tx.execute(
                    "INSERT INTO job (id, kind, session_id, created_at, updated_at) VALUES (?1, 'autopilot', ?2, ?3, ?3)",
                    params![id, session_id, ts],
                )?;
                id
            }
        };
        for step in autopilot::steps(trigger) {
            let seq = autopilot::steps(AutopilotTrigger::Confirm).iter().position(|s| s == step).unwrap_or(0) as i64;
            tx.execute(
                "INSERT INTO autopilot_step (job_id, seq, step, updated_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(job_id, step) DO NOTHING",
                params![id, seq, step.as_str(), ts],
            )?;
        }
        audit::append(&tx, Entry {
            actor: "system", action: "autopilot.enqueue", entity_type: "session", entity_id: Some(session_id),
            meta: json!({ "job_id": id, "trigger": trigger.as_str() }),
        })?;
        tx.commit()?;
        Ok(Some(self.job(&id)?))
    }

    /// "Erneut versuchen": failed and skipped steps run again (a provider set up meanwhile counts).
    pub fn retry_autopilot(&self, job_id: &str) -> Result<Job> {
        let job = self.job(job_id)?;
        if job.kind != "autopilot" {
            return Err(Error::Invalid("Kein Autopilot-Lauf".into()));
        }
        if matches!(job.status, JobStatus::Queued | JobStatus::Running) {
            return Err(Error::Invalid("Der Autopilot läuft bereits".into()));
        }
        let tx = self.conn.unchecked_transaction()?;
        let ts = now();
        tx.execute(
            "UPDATE autopilot_step SET status = 'pending', detail = NULL, updated_at = ?2
             WHERE job_id = ?1 AND status IN ('failed', 'skipped', 'pending', 'running')",
            params![job_id, ts],
        )?;
        tx.execute("UPDATE job SET status = 'queued', progress = 0, last_error = NULL, updated_at = ?2 WHERE id = ?1", params![job_id, ts])?;
        audit::append(&tx, Entry { actor: ACTOR, action: "autopilot.retry", entity_type: "job", entity_id: Some(job_id), meta: json!({}) })?;
        tx.commit()?;
        self.job(job_id)
    }

    pub fn autopilot_steps(&self, job_id: &str) -> Result<Vec<AutopilotStep>> {
        let mut stmt = self.conn.prepare(
            "SELECT step, status, detail, sent_json IS NOT NULL, provider, model, updated_at FROM autopilot_step
             WHERE job_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map([job_id], |r| {
            Ok(AutopilotStep {
                step: super::parse_col(r, 0)?,
                status: super::parse_col(r, 1)?,
                detail: r.get(2)?,
                sent: r.get(3)?,
                provider: r.get(4)?,
                model: r.get(5)?,
                updated_at: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn autopilot_run(&self, session_id: &str) -> Result<Option<AutopilotRun>> {
        let job = self
            .conn
            .query_row(
                &format!("{JOB_SELECT} WHERE session_id = ?1 AND kind = 'autopilot' ORDER BY created_at DESC, rowid DESC LIMIT 1"),
                [session_id],
                row_to_job,
            )
            .optional()?;
        let Some(job) = job else { return Ok(None) };
        let steps = self.autopilot_steps(&job.id)?;
        Ok(Some(AutopilotRun { job, steps }))
    }

    /// "Was wurde gesendet?": the pseudonymized requests of a step and the local token table.
    pub fn autopilot_sent(&self, job_id: &str, step: AutopilotStepKind) -> Result<Option<Preview>> {
        let sent: Option<Option<String>> = self
            .conn
            .query_row("SELECT sent_json FROM autopilot_step WHERE job_id = ?1 AND step = ?2", params![job_id, step.as_str()], |r| r.get(0))
            .optional()?;
        sent.flatten().map(|s| serde_json::from_str(&s).map_err(Into::into)).transpose()
    }

    pub fn set_autopilot_step(
        &self,
        job_id: &str,
        step: AutopilotStepKind,
        status: AutopilotStepStatus,
        detail: Option<&str>,
        sent: Option<&Preview>,
        model: Option<(ProviderId, &str)>,
    ) -> Result<()> {
        let sent = sent.map(serde_json::to_string).transpose()?;
        self.conn.execute(
            "UPDATE autopilot_step SET status = ?3, detail = ?4, sent_json = COALESCE(?5, sent_json),
                 provider = COALESCE(?6, provider), model = COALESCE(?7, model), updated_at = ?8
             WHERE job_id = ?1 AND step = ?2",
            params![
                job_id, step.as_str(), status.as_str(), detail, sent,
                model.map(|(p, _)| autopilot::provider_str(p)), model.map(|(_, m)| m), now()
            ],
        )?;
        Ok(())
    }

    /// Ends a run; the outcome per step goes into the audit log (statuses only).
    pub fn finish_autopilot(&self, job_id: &str) -> Result<()> {
        let steps = self.autopilot_steps(job_id)?;
        let outcome: serde_json::Map<String, serde_json::Value> =
            steps.iter().map(|s| (s.step.as_str().to_string(), json!(s.status.as_str()))).collect();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("UPDATE job SET status = 'done', progress = 100, updated_at = ?2 WHERE id = ?1", params![job_id, now()])?;
        audit::append(&tx, Entry { actor: "system", action: "autopilot.end", entity_type: "job", entity_id: Some(job_id), meta: json!({ "steps": outcome }) })?;
        tx.commit()?;
        Ok(())
    }

    /// After signing a note: does the session need a fresh summary ([`autopilot::resummarize`])?
    pub fn summary_refresh_due(&self, session_id: &str) -> Result<bool> {
        let input = self.summary_input(session_id)?;
        if input.segments.is_empty() && input.note.is_empty() {
            return Ok(false);
        }
        let row: Option<(i64, String)> = self
            .conn
            .query_row("SELECT note_chars, status FROM session_summary WHERE session_id = ?1", [session_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let current = autopilot::note_chars(&input);
        Ok(match row {
            None => autopilot::resummarize(None, false, current),
            Some((chars, status)) => autopilot::resummarize(Some(chars.max(0) as usize), status == "edited", current),
        })
    }
}
