//! AI spend from the audit log (slice 33): totals, per task, per model, per day.

use serde::Serialize;
use serde_json::{json, Value};
use ts_rs::TS;

use super::{Store, ACTOR};
use crate::audit::{self, Entry};
use crate::Result;

/// Billed LLM cost over a period ([`Store::llm_spend`]).
#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct LlmSpend {
    pub requests: u32,
    pub usd: f64,
}

/// What an AI request was for, from its audit action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SpendTask {
    /// `llm.extract`: facts (manual or autopilot).
    Facts,
    /// `llm.draft`: Bericht sections.
    Draft,
    /// `llm.summary`
    Summary,
    /// `llm.gaps`: which Lotse questions the sessions answer.
    Gaps,
    /// `llm.review`: Gutachter-Check of a Bericht.
    Review,
    /// `llm.workshop`: Diktat-Werkstatt.
    Workshop,
    /// `llm.agent`: assistant chat.
    Agent,
    /// `llm.prep_sheet` and `prep.live`: voice session prep.
    Prep,
    /// `transcription.cloud` and `dictation.cloud`.
    Speech,
    /// `llm.tts`: read-aloud.
    ReadAloud,
    /// `llm.test` and anything newer under `llm.*`.
    Other,
}

fn task(action: &str) -> Option<SpendTask> {
    Some(match action {
        "llm.extract" => SpendTask::Facts,
        "llm.draft" => SpendTask::Draft,
        "llm.summary" => SpendTask::Summary,
        "llm.gaps" => SpendTask::Gaps,
        "llm.review" => SpendTask::Review,
        "llm.workshop" => SpendTask::Workshop,
        "llm.agent" => SpendTask::Agent,
        "llm.prep_sheet" | "prep.live" => SpendTask::Prep,
        "transcription.cloud" | "dictation.cloud" => SpendTask::Speech,
        "llm.tts" => SpendTask::ReadAloud,
        a if a.starts_with("llm.") => SpendTask::Other,
        _ => return None,
    })
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TaskSpend {
    pub task: SpendTask,
    pub requests: u32,
    pub usd: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ModelSpend {
    pub model: String,
    pub provider: Option<String>,
    pub requests: u32,
    pub usd: f64,
}

/// One UTC day (`YYYY-MM-DD`) with at least one request.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DaySpend {
    pub day: String,
    pub requests: u32,
    pub usd: f64,
}

/// Successful requests since a point in time, broken down ([`Store::llm_spend_report`]).
/// Requests without a reported cost (local, Requesty, custom) count as requests only.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct SpendReport {
    pub total: LlmSpend,
    /// Most expensive first.
    pub tasks: Vec<TaskSpend>,
    /// Most expensive first.
    pub models: Vec<ModelSpend>,
    /// Oldest first.
    pub days: Vec<DaySpend>,
}

impl Store {
    /// What the LLM calls since `since` (RFC 3339 prefix, e.g. "2026-10") cost, from the audit
    /// log, cloud transcription included.
    pub fn llm_spend(&self, since: &str) -> Result<LlmSpend> {
        Ok(self.llm_spend_report(since)?.total)
    }

    pub fn llm_spend_report(&self, since: &str) -> Result<SpendReport> {
        let mut stmt = self.conn.prepare(
            "SELECT action, at, meta_json FROM audit_event
             WHERE (action LIKE 'llm.%' OR action IN ('transcription.cloud', 'prep.live', 'dictation.cloud')) AND at >= ?1
             ORDER BY seq",
        )?;
        let mut report = SpendReport::default();
        let rows = stmt.query_map([since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
        for row in rows {
            let (action, at, meta) = row?;
            let Some(task) = task(&action) else { continue };
            let meta: Value = serde_json::from_str(&meta)?;
            if meta.get("ok").and_then(Value::as_bool) != Some(true) {
                continue;
            }
            let usd = meta.get("cost_usd").and_then(Value::as_f64).unwrap_or(0.0);
            let add = |requests: &mut u32, sum: &mut f64| {
                *requests += 1;
                *sum += usd;
            };
            add(&mut report.total.requests, &mut report.total.usd);

            match report.tasks.iter_mut().find(|t| t.task == task) {
                Some(t) => add(&mut t.requests, &mut t.usd),
                None => report.tasks.push(TaskSpend { task, requests: 1, usd }),
            }

            let model = meta.get("model").and_then(Value::as_str).unwrap_or("?").to_string();
            let provider = meta.get("provider").and_then(Value::as_str).map(str::to_string);
            match report.models.iter_mut().find(|m| m.model == model && m.provider == provider) {
                Some(m) => add(&mut m.requests, &mut m.usd),
                None => report.models.push(ModelSpend { model, provider, requests: 1, usd }),
            }

            let day: String = at.chars().take(10).collect();
            match report.days.iter_mut().find(|d| d.day == day) {
                Some(d) => add(&mut d.requests, &mut d.usd),
                None => report.days.push(DaySpend { day, requests: 1, usd }),
            }
        }
        let by_cost = |a: (f64, u32), b: (f64, u32)| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1));
        report.tasks.sort_by(|a, b| by_cost((a.usd, a.requests), (b.usd, b.requests)).then(a.task.cmp(&b.task)));
        report.models.sort_by(|a, b| by_cost((a.usd, a.requests), (b.usd, b.requests)).then(a.model.cmp(&b.model)));
        report.days.sort_by(|a, b| a.day.cmp(&b.day));
        Ok(report)
    }

    /// A balance lookup at the provider: whether it worked and whether the account balance was
    /// read (management key), never the amounts.
    pub fn log_balance_check(&self, provider: crate::llm::ProviderId, ok: bool, credits: bool) -> Result<()> {
        audit::append(&self.conn, Entry {
            actor: ACTOR,
            action: "billing.balance",
            entity_type: "llm",
            entity_id: None,
            meta: json!({ "provider": provider, "ok": ok, "credits": credits }),
        })
    }
}

/// The UTC day `days` before `now` (RFC 3339), as `YYYY-MM-DD`: the start of a "last n days" window.
pub fn day_before(now: &str, days: i64) -> String {
    time::OffsetDateTime::parse(now, &time::format_description::well_known::Rfc3339)
        .map(|t| (t - time::Duration::days(days)).date().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{Completion, ProviderId};
    use crate::store::DictationLog;

    fn call(model: &str, cost: Option<f64>) -> Completion {
        Completion {
            text: "OK".into(),
            model: model.into(),
            prompt_tokens: Some(10),
            completion_tokens: Some(5),
            reasoning_tokens: None,
            cost,
            latency_ms: 1,
            tool_calls: vec![],
        }
    }

    #[test]
    fn llm_spend_sums_billed_cost_of_successful_calls() {
        let store = Store::open_in_memory().unwrap();
        store.log_llm_call("llm.extract", ProviderId::Openrouter, "m", Ok(&call("m", Some(0.012)))).unwrap();
        store.log_llm_call("llm.draft", ProviderId::Openrouter, "m", Ok(&call("m", Some(0.003)))).unwrap();
        store.log_llm_call("llm.draft", ProviderId::Local, "m", Ok(&call("m", None))).unwrap();
        store.log_llm_call("llm.draft", ProviderId::Openrouter, "m", Err("x")).unwrap();
        let s = store.llm_spend("2000").unwrap();
        assert_eq!(s.requests, 3);
        assert!((s.usd - 0.015).abs() < 1e-12);
        assert_eq!(store.llm_spend("9999").unwrap().requests, 0);
    }

    #[test]
    fn report_breaks_spend_down_by_task_model_and_day() {
        let store = Store::open_in_memory().unwrap();
        store.log_llm_call("llm.extract", ProviderId::Openrouter, "a/cheap", Ok(&call("a/cheap", Some(0.01)))).unwrap();
        store.log_llm_call("llm.extract", ProviderId::Openrouter, "a/cheap", Ok(&call("a/cheap", Some(0.02)))).unwrap();
        store.log_llm_call("llm.draft", ProviderId::Openrouter, "b/best", Ok(&call("b/best", Some(0.5)))).unwrap();
        store.log_llm_call("llm.summary", ProviderId::Local, "qwen3-8b", Ok(&call("qwen3-8b", None))).unwrap();
        store.log_llm_call("llm.test", ProviderId::Openrouter, "a/cheap", Ok(&call("a/cheap", Some(0.0001)))).unwrap();
        store.log_llm_call("llm.agent", ProviderId::Openrouter, "b/best", Err("x")).unwrap();
        store
            .log_dictation(None, &DictationLog { provider: Some("openrouter".into()), model: "w".into(), seconds: 3.0, pieces: 1, requests: 1, cost: Some(0.004), error: None })
            .unwrap();
        // Not an AI request: ignored even though it starts like one.
        audit::append(store.conn(), Entry { actor: ACTOR, action: "transcription.end", entity_type: "x", entity_id: None, meta: serde_json::json!({ "ok": true }) }).unwrap();

        let r = store.llm_spend_report("2000").unwrap();
        assert_eq!(r.total.requests, 6);
        let tasks: Vec<_> = r.tasks.iter().map(|t| (t.task, t.requests)).collect();
        assert_eq!(tasks, [(SpendTask::Draft, 1), (SpendTask::Facts, 2), (SpendTask::Speech, 1), (SpendTask::Other, 1), (SpendTask::Summary, 1)]);
        assert!((r.tasks[1].usd - 0.03).abs() < 1e-12);
        let models: Vec<_> = r.models.iter().map(|m| (m.model.as_str(), m.provider.as_deref(), m.requests)).collect();
        assert_eq!(models, [("b/best", Some("openrouter"), 1), ("a/cheap", Some("openrouter"), 3), ("w", Some("openrouter"), 1), ("qwen3-8b", Some("local"), 1)]);
        assert_eq!(r.days.len(), 1);
        assert_eq!(r.days[0].requests, 6);
        assert_eq!(r.days[0].day.len(), 10);
        assert!((r.days[0].usd - r.total.usd).abs() < 1e-12);

        store.log_balance_check(ProviderId::Openrouter, true, false).unwrap();
        assert_eq!(store.llm_spend_report("2000").unwrap().total.requests, 6, "a balance lookup is no AI request");
    }

    #[test]
    fn day_before_crosses_months() {
        assert_eq!(day_before("2026-11-03T08:00:00Z", 29), "2026-10-05");
        assert_eq!(day_before("2026-10-29T23:59:00Z", 0), "2026-10-29");
        assert_eq!(day_before("kaputt", 3), "");
    }
}
