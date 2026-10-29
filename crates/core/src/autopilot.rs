//! Autopilot after the session (Phase 4, slice 31).
//!
//! Once the therapist confirms a transcript (a human step: it deletes the audio), the job worker
//! runs these steps without clicks: create the Sitzungsnotiz, propose facts into it, write the
//! session summary. Nothing is applied or signed. Cloud requests go through the same pseudonymizer
//! as the manual buttons; what was sent is stored per step ("Was wurde gesendet?"). A failed step
//! is shown with "Erneut versuchen" and never blocks the note.
//!
//! The extraction and summary loops here are also what the manual buttons run, so both paths
//! send and check exactly the same.

use crate::domain::*;
use crate::extract::{self, Preview};
use crate::llm::{ChatOptions, Completion, ProviderId, Role};
use crate::pseudo::Pseudonymized;
use crate::store::{Store, SummaryInput, SummaryMeta};
use crate::summary;
use crate::templates::{Context, Template};
use crate::{Error, Result};

/// Setting key (bool). Absent = on: the user decided autopilot is on by default.
pub const SETTING: &str = "autopilot";

/// A summary is written again after signing when the note's text changed by at least this many
/// characters and this share of what the summary read.
pub const RESUMMARIZE_MIN_CHARS: usize = 200;
pub const RESUMMARIZE_SHARE: f64 = 0.3;

/// The steps a trigger runs, in order.
pub fn steps(trigger: AutopilotTrigger) -> &'static [AutopilotStepKind] {
    match trigger {
        AutopilotTrigger::Confirm => &[AutopilotStepKind::Note, AutopilotStepKind::Extract, AutopilotStepKind::Summary],
        AutopilotTrigger::Signed => &[AutopilotStepKind::Summary],
    }
}

/// What the app gives a run: store access (locked only briefly) and the model.
pub trait Host {
    fn store<T>(&mut self, f: impl FnOnce(&Store) -> Result<T>) -> Result<T>;
    /// The active provider and model, the endpoint made ready (a local model is started).
    /// `Ok(None)`: no AI provider is set up, so the AI steps are skipped.
    fn ready(&mut self) -> Result<Option<(ProviderId, String)>>;
    /// One model request, logged under `action` (`llm.extract`, `llm.summary`). `Ok(None)`: cancelled.
    fn complete(&mut self, action: &str, messages: &[(Role, &Pseudonymized<String>)], opts: &ChatOptions) -> Result<Option<Completion>>;
    /// `done` of `total` requests answered (`done == 0` before the first).
    fn progress(&mut self, _done: usize, _total: usize) {}
}

// ---------- what goes out ----------

/// The Bericht template facts are tagged with, resolved for the episode (none without one).
pub fn tag_template(s: &Store, episode: Option<&Episode>) -> Result<Option<Template>> {
    let Some(ep) = episode else { return Ok(None) };
    let t = s.template(extract::TAG_TEMPLATE_KEY)?;
    let ctx = Context { method: ep.method, age_group: ep.age_group, kind: Some(ApplicationKind::Erstantrag) };
    Ok(Some(t.resolve(&ctx)))
}

/// What an extraction needs besides the request: the segments it cites into and the allowed
/// note sections and Bericht tags.
pub struct ExtractionScope {
    pub segments: Vec<SourceSegment>,
    pub sections: Vec<String>,
    pub tags: Vec<String>,
}

/// Exactly what an extraction for this note would send, built from the current transcript and terms.
pub fn prepare_extraction(s: &Store, report_id: &str) -> Result<(extract::Prepared, ExtractionScope)> {
    let report = s.report(report_id)?;
    let Some(session_id) = report.session_id.as_deref().filter(|_| report.kind == "sitzungsnotiz") else {
        return Err(Error::Invalid("Fakten gibt es nur für Sitzungsnotizen".into()));
    };
    let episode = report.episode_id.as_deref().map(|e| s.episode(e)).transpose()?;
    let mut template = s.template(&report.template_key)?;
    if let Some(ep) = &episode {
        template = template.resolve(&Context { method: ep.method, age_group: ep.age_group, kind: None });
    }
    let segments = s.source_segments(session_id)?;
    let dict = s.pseudo_dictionary(&report.patient_id)?;
    let bericht = tag_template(s, episode.as_ref())?;
    let tag_defs = bericht.as_ref().map(extract::tag_sections).unwrap_or_default();
    let prepared = extract::prepare(&template, &tag_defs, &segments, dict, extract::CHUNK_CHARS)?;
    let sections = extract::target_sections(&template).iter().map(|d| d.key.clone()).collect();
    let tags = tag_defs.iter().map(|d| d.key.clone()).collect();
    Ok((prepared, ExtractionScope { segments, sections, tags }))
}

/// Exactly what summarizing this session would send, from its confirmed transcript and note.
pub fn prepare_summary(s: &Store, session_id: &str) -> Result<(extract::Prepared, SummaryInput)> {
    let input = s.summary_input(session_id)?;
    let dict = s.pseudo_dictionary(&input.patient.id)?;
    let prepared = summary::prepare(&input.patient, &input.segments, &input.note, dict, extract::CHUNK_CHARS)?;
    Ok((prepared, input))
}

/// Characters of note text a summary reads (see [`RESUMMARIZE_MIN_CHARS`]).
pub fn note_chars(input: &SummaryInput) -> usize {
    input.note.iter().map(|n| n.text.chars().count()).sum()
}

// ---------- the loops ----------

pub struct ExtractOutcome {
    pub facts: Vec<NoteFact>,
    pub dropped: usize,
    /// Chunks answered; fewer than `total` when cancelled.
    pub chunks: usize,
    pub total: usize,
}

/// One request per chunk, then the checked facts are stored as proposals. Cancelled midway:
/// facts from finished chunks are kept. `Ok(None)`: cancelled before any chunk finished.
pub fn extract<H: Host>(
    host: &mut H,
    (provider, model): (ProviderId, &str),
    report_id: &str,
    prepared: &extract::Prepared,
    scope: &ExtractionScope,
) -> Result<Option<ExtractOutcome>> {
    let total = prepared.chunks.len();
    let opts = extract::chat_options();
    let keys: Vec<&str> = scope.sections.iter().map(String::as_str).collect();
    let tags: Vec<&str> = scope.tags.iter().map(String::as_str).collect();
    let (mut facts, mut dropped, mut done, mut answered_by) = (Vec::new(), 0, 0, model.to_string());
    for chunk in &prepared.chunks {
        host.progress(done, total);
        let messages = [(Role::System, &prepared.system), (Role::User, &chunk.user)];
        let Some(completion) = host.complete("llm.extract", &messages, &opts)? else { break };
        let parsed = extract::parse(&completion.text, &chunk.range, &keys, &tags, &prepared.map)?;
        facts.extend(parsed.facts);
        dropped += parsed.dropped;
        answered_by = completion.model;
        done += 1;
    }
    if done == 0 {
        return Ok(None);
    }
    host.progress(done, total);
    let (facts, merged) = extract::merge(facts);
    let meta = ExtractionMeta {
        provider: provider_str(provider),
        model: answered_by,
        prompt_hash: prepared.prompt_hash.clone(),
        chunks: done,
        dropped,
        merged,
        tagged: !scope.tags.is_empty(),
    };
    let facts = host.store(|s| s.save_extraction(report_id, &meta, &facts, &scope.segments))?;
    Ok(Some(ExtractOutcome { facts, dropped, chunks: done, total }))
}

pub struct SummaryOutcome {
    pub summary: crate::store::SessionSummary,
    pub dropped: usize,
}

/// One request per chunk, then the session's summary is replaced by the checked points.
/// `Ok(None)`: cancelled, the earlier summary stays.
pub fn summarize<H: Host>(
    host: &mut H,
    (provider, model): (ProviderId, &str),
    session_id: &str,
    prepared: &extract::Prepared,
    input: &SummaryInput,
) -> Result<Option<SummaryOutcome>> {
    let total = prepared.chunks.len();
    let opts = summary::chat_options();
    let quota = summary::quota(total);
    let (mut points, mut dropped, mut answered_by) = (Vec::new(), 0, model.to_string());
    for (i, chunk) in prepared.chunks.iter().enumerate() {
        host.progress(i, total);
        let messages = [(Role::System, &prepared.system), (Role::User, &chunk.user)];
        let Some(completion) = host.complete("llm.summary", &messages, &opts)? else { return Ok(None) };
        let scope = summary::Scope {
            range: &chunk.range,
            notes: if i == 0 { input.note.len() } else { 0 },
            segments: &input.segments,
            note: &input.note,
        };
        let parsed = summary::parse(&completion.text, &scope, quota, &prepared.map, &input.patient)?;
        points.extend(parsed.points);
        dropped += parsed.dropped;
        answered_by = completion.model;
    }
    host.progress(total, total);
    let meta = SummaryMeta {
        provider: provider_str(provider),
        model: answered_by,
        prompt_hash: prepared.prompt_hash.clone(),
        chunks: total,
        dropped,
        note_chars: note_chars(input),
    };
    let stored = summary::to_stored(&points, &input.segments, &input.note);
    let summary = host.store(|s| s.save_summary(session_id, &meta, &stored))?;
    Ok(Some(SummaryOutcome { summary, dropped }))
}

pub fn provider_str(id: ProviderId) -> String {
    serde_json::to_value(id).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

// ---------- the chain ----------

/// How a step ended.
enum StepEnd {
    Done { sent: Option<Preview>, model: Option<(ProviderId, String)> },
    Skipped(String),
}

/// Runs the job's pending steps in order (a retry or a restart continues where it stopped).
/// `Err(Cancelled)` leaves the unfinished steps pending; any other step error is stored on the
/// step and the chain goes on.
pub fn run<H: Host>(host: &mut H, job: &Job) -> Result<()> {
    let session_id = job.session_id.clone().ok_or_else(|| Error::Invalid("Autopilot ohne Sitzung".into()))?;
    let steps = host.store(|s| s.autopilot_steps(&job.id))?;
    let total = steps.len();
    // Resolved once, on the first AI step.
    let mut llm: Option<std::result::Result<Option<(ProviderId, String)>, String>> = None;
    for (i, step) in steps.iter().enumerate() {
        if step.status != AutopilotStepStatus::Pending {
            continue;
        }
        let kind = step.step;
        host.store(|s| s.set_autopilot_step(&job.id, kind, AutopilotStepStatus::Running, None, None, None))?;
        let result = match kind {
            AutopilotStepKind::Note => note_step(host, &session_id),
            AutopilotStepKind::Extract | AutopilotStepKind::Summary => {
                let ready = llm.get_or_insert_with(|| host.ready().map_err(|e| e.to_string())).clone();
                match ready {
                    Err(e) => Err(Error::Llm(e)),
                    Ok(None) => Ok(StepEnd::Skipped("Kein KI-Anbieter eingerichtet (Einstellungen → KI-Entwürfe)".into())),
                    Ok(Some(m)) if kind == AutopilotStepKind::Extract => extract_step(host, &session_id, m),
                    Ok(Some(m)) => summary_step(host, &session_id, m),
                }
            }
        };
        let (status, detail, sent, model) = match result {
            Err(Error::Cancelled) => {
                host.store(|s| s.set_autopilot_step(&job.id, kind, AutopilotStepStatus::Pending, None, None, None))?;
                return Err(Error::Cancelled);
            }
            Err(e) => (AutopilotStepStatus::Failed, Some(e.to_string()), None, None),
            Ok(StepEnd::Skipped(why)) => (AutopilotStepStatus::Skipped, Some(why), None, None),
            Ok(StepEnd::Done { sent, model }) => (AutopilotStepStatus::Done, None, sent, model),
        };
        host.store(|s| {
            s.set_autopilot_step(&job.id, kind, status, detail.as_deref(), sent.as_ref(), model.as_ref().map(|(p, m)| (*p, m.as_str())))?;
            s.set_job_progress(&job.id, ((i + 1) * 100 / total.max(1)) as u8)
        })?;
    }
    host.store(|s| s.finish_autopilot(&job.id))
}

fn note_step<H: Host>(host: &mut H, session_id: &str) -> Result<StepEnd> {
    let session = host.store(|s| s.session(session_id))?;
    if session.note_id.is_some() {
        return Ok(StepEnd::Skipped("Notiz war schon angelegt".into()));
    }
    host.store(|s| s.create_session_note(session_id))?;
    Ok(StepEnd::Done { sent: None, model: None })
}

fn extract_step<H: Host>(host: &mut H, session_id: &str, (provider, model): (ProviderId, String)) -> Result<StepEnd> {
    let session = host.store(|s| s.session(session_id))?;
    let Some(note_id) = session.note_id else { return Ok(StepEnd::Skipped("Keine Sitzungsnotiz".into())) };
    if matches!(session.note_status, Some(ReportStatus::Signed | ReportStatus::Exported)) {
        return Ok(StepEnd::Skipped("Die Notiz ist schon signiert".into()));
    }
    if host.store(|s| s.source_segments(session_id))?.is_empty() {
        return Ok(StepEnd::Skipped("Keine bestätigte Abschrift".into()));
    }
    let (prepared, scope) = host.store(|s| prepare_extraction(s, &note_id))?;
    let sent = prepared.preview();
    // Cancelled before any chunk: the worker treats it like any cancel.
    extract(host, (provider, &model), &note_id, &prepared, &scope)?.ok_or(Error::Cancelled)?;
    Ok(StepEnd::Done { sent: Some(sent), model: Some((provider, model)) })
}

fn summary_step<H: Host>(host: &mut H, session_id: &str, (provider, model): (ProviderId, String)) -> Result<StepEnd> {
    if host.store(|s| s.session_summary(session_id))?.is_some_and(|s| s.status == "edited") {
        return Ok(StepEnd::Skipped("Von Ihnen bearbeitet, bleibt unverändert".into()));
    }
    let input = host.store(|s| s.summary_input(session_id))?;
    if input.segments.is_empty() && input.note.is_empty() {
        return Ok(StepEnd::Skipped("Nichts zum Zusammenfassen".into()));
    }
    let (prepared, input) = host.store(|s| prepare_summary(s, session_id))?;
    let sent = prepared.preview();
    summarize(host, (provider, &model), session_id, &prepared, &input)?.ok_or(Error::Cancelled)?;
    Ok(StepEnd::Done { sent: Some(sent), model: Some((provider, model)) })
}

/// Whether a signed note should get a fresh summary: none yet, or not edited by her and the note
/// text changed by at least [`RESUMMARIZE_MIN_CHARS`] and [`RESUMMARIZE_SHARE`] of what the
/// summary read.
pub fn resummarize(summarized_chars: Option<usize>, edited: bool, current_chars: usize) -> bool {
    match summarized_chars {
        None => true,
        Some(_) if edited => false,
        Some(before) => {
            let diff = before.abs_diff(current_chars);
            diff >= RESUMMARIZE_MIN_CHARS && diff as f64 >= before as f64 * RESUMMARIZE_SHARE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt;
    use std::path::Path;

    /// Answers like a model would, records what was sent.
    struct Fake<'a> {
        store: &'a Store,
        model: Option<(ProviderId, String)>,
        sent: Vec<(String, String)>,
        fail: Option<&'static str>,
    }

    impl Host for Fake<'_> {
        fn store<T>(&mut self, f: impl FnOnce(&Store) -> Result<T>) -> Result<T> {
            f(self.store)
        }
        fn ready(&mut self) -> Result<Option<(ProviderId, String)>> {
            Ok(self.model.clone())
        }
        fn complete(&mut self, action: &str, messages: &[(Role, &Pseudonymized<String>)], _: &ChatOptions) -> Result<Option<Completion>> {
            self.sent.push((action.to_string(), messages[1].1.get().clone()));
            if self.fail == Some(action) {
                return Err(Error::Llm("HTTP 500".into()));
            }
            let text = match action {
                "llm.extract" => r#"{"facts": [{"section": "verlauf", "text": "Pat. schläft schlecht", "segments": [1]}]}"#,
                _ => r#"{"points": [{"text": "Pat. schläft seit Wochen schlecht.", "sources": ["S1"]}]}"#,
            };
            Ok(Some(Completion {
                text: text.into(),
                model: "fake-model".into(),
                prompt_tokens: None,
                completion_tokens: None,
                reasoning_tokens: None,
                cost: None,
                latency_ms: 1,
                tool_calls: vec![],
            }))
        }
    }

    fn fake(store: &Store) -> Fake<'_> {
        Fake { store, model: Some((ProviderId::Custom, "m".into())), sent: vec![], fail: None }
    }

    fn session(store: &Store) -> Session {
        let p = store
            .create_patient(PatientInput { first_name: "Karl".into(), last_name: "Beispiel".into(), chiffre: Some("B010190".into()), ..Default::default() })
            .unwrap();
        store
            .create_session(SessionInput { patient_id: p.id, episode_id: None, scheduled_at: Some("2026-10-01T09:00:00Z".into()), modality: None, planned_minutes: None })
            .unwrap()
    }

    /// A dictation transcribed to a draft (not confirmed yet).
    fn draft_transcript(store: &Store, s: &Session, dir: &Path) -> Transcript {
        let (src, mut w) = store.start_source(&s.id, SourceKind::Dictation, dir).unwrap();
        w.write(&vec![5i16; 8_000]).unwrap();
        let src = store.finish_source(&src.id, "stopped", &w).unwrap();
        let job = store.enqueue_transcription(&src.id, None).unwrap();
        store.claim_next_job().unwrap();
        let segs = vec![stt::Segment { start_ms: 0, end_ms: 900, text: "Herr Beispiel schläft seit Wochen schlecht.".into() }];
        store.finish_transcription(&job.id, "e", "m", &segs).unwrap()
    }

    fn claim(store: &Store) -> Job {
        let job = store.claim_next_job().unwrap().expect("a queued job");
        assert_eq!(job.kind, "autopilot");
        job
    }

    fn statuses(run: &crate::store::AutopilotRun) -> Vec<(AutopilotStepKind, AutopilotStepStatus)> {
        run.steps.iter().map(|s| (s.step, s.status)).collect()
    }

    #[test]
    fn confirm_runs_note_facts_and_summary_with_pseudonymized_text_only() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let s = session(&store);
        let t = draft_transcript(&store, &s, dir.path());
        // Nothing runs before the transcript is confirmed.
        assert!(store.claim_next_job().unwrap().is_none());
        assert!(store.autopilot_run(&s.id).unwrap().is_none());

        store.confirm_transcript(&t.id).unwrap();
        store.enqueue_autopilot(&s.id, AutopilotTrigger::Confirm).unwrap().unwrap();
        let job = claim(&store);
        let mut host = fake(&store);
        run(&mut host, &job).unwrap();

        let session = store.session(&s.id).unwrap();
        let note = session.note_id.expect("note created");
        let facts = store.note_facts(&note).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].status, FactStatus::Proposed, "nothing is applied");
        assert_eq!(store.session_summary(&s.id).unwrap().unwrap().points.len(), 1);
        assert_eq!(session.note_status, Some(ReportStatus::Draft), "nothing is signed");

        let actions: Vec<&str> = host.sent.iter().map(|(a, _)| a.as_str()).collect();
        assert_eq!(actions, ["llm.extract", "llm.summary"]);
        assert!(host.sent.iter().all(|(_, text)| !text.contains("Beispiel")), "the name never leaves");

        let run_ = store.autopilot_run(&s.id).unwrap().unwrap();
        assert_eq!(run_.job.status, JobStatus::Done);
        use AutopilotStepKind::*;
        use AutopilotStepStatus::*;
        assert_eq!(statuses(&run_), [(Note, Done), (Extract, Done), (Summary, Done)]);
        let sent = store.autopilot_sent(&job.id, Extract).unwrap().unwrap();
        assert_eq!(sent.messages, [host.sent[0].1.clone()]);
        assert!(sent.tokens.iter().any(|t| t.original == "Beispiel"), "the token table stays local");
        assert!(store.autopilot_sent(&job.id, Note).unwrap().is_none());

        // The audit log has the run, no text.
        let events = crate::audit::list(store.conn(), 50).unwrap();
        let end = events.iter().find(|e| e.action == "autopilot.end").unwrap();
        assert!(!end.meta_json.contains("schläft"));
        assert!(events.iter().any(|e| e.action == "autopilot.enqueue"));
    }

    #[test]
    fn autopilot_off_queues_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        assert!(store.autopilot_enabled().unwrap(), "on by default");
        store.set_autopilot(false).unwrap();
        let s = session(&store);
        let t = draft_transcript(&store, &s, dir.path());
        store.confirm_transcript(&t.id).unwrap();
        assert!(store.enqueue_autopilot(&s.id, AutopilotTrigger::Confirm).unwrap().is_none());
        assert!(store.claim_next_job().unwrap().is_none());
        assert!(store.session(&s.id).unwrap().note_id.is_none());
    }

    #[test]
    fn a_failed_step_doesnt_block_the_others_and_retry_runs_it_again() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let s = session(&store);
        let t = draft_transcript(&store, &s, dir.path());
        store.confirm_transcript(&t.id).unwrap();
        store.enqueue_autopilot(&s.id, AutopilotTrigger::Confirm).unwrap();
        // A second trigger while queued joins the queued run.
        store.enqueue_autopilot(&s.id, AutopilotTrigger::Signed).unwrap();
        let job = claim(&store);
        let mut host = Fake { fail: Some("llm.extract"), ..fake(&store) };
        run(&mut host, &job).unwrap();
        use AutopilotStepKind::*;
        use AutopilotStepStatus::*;
        let r = store.autopilot_run(&s.id).unwrap().unwrap();
        assert_eq!(statuses(&r), [(Note, Done), (Extract, Failed), (Summary, Done)]);
        assert!(r.steps[1].detail.as_deref().unwrap().contains("HTTP 500"));

        store.retry_autopilot(&job.id).unwrap();
        let job = claim(&store);
        run(&mut fake(&store), &job).unwrap();
        let r = store.autopilot_run(&s.id).unwrap().unwrap();
        // Only the failed step ran again; the facts now come through.
        assert_eq!(statuses(&r), [(Note, Done), (Extract, Done), (Summary, Done)]);
        assert_eq!(store.note_facts(&store.session(&s.id).unwrap().note_id.unwrap()).unwrap().len(), 1);
    }

    #[test]
    fn without_a_provider_the_ai_steps_are_skipped_and_an_edited_summary_stays() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let s = session(&store);
        let t = draft_transcript(&store, &s, dir.path());
        store.confirm_transcript(&t.id).unwrap();
        store.enqueue_autopilot(&s.id, AutopilotTrigger::Confirm).unwrap();
        let job = claim(&store);
        let mut host = Fake { model: None, ..fake(&store) };
        run(&mut host, &job).unwrap();
        use AutopilotStepKind::*;
        use AutopilotStepStatus::*;
        assert_eq!(statuses(&store.autopilot_run(&s.id).unwrap().unwrap()), [(Note, Done), (Extract, Skipped), (Summary, Skipped)]);
        assert!(host.sent.is_empty());

        // An edited summary is not overwritten by the autopilot.
        let meta = SummaryMeta { provider: "x".into(), model: "x".into(), prompt_hash: "h".into(), chunks: 1, dropped: 0, note_chars: 0 };
        let p = summary::SummaryPoint { text: "Eigener Punkt".into(), segments: vec![], note: vec![] };
        store.save_summary(&s.id, &meta, std::slice::from_ref(&p)).unwrap();
        store.edit_summary(&s.id, vec![p]).unwrap();
        store.enqueue_autopilot(&s.id, AutopilotTrigger::Signed).unwrap();
        let job = claim(&store);
        run(&mut fake(&store), &job).unwrap();
        assert_eq!(statuses(&store.autopilot_run(&s.id).unwrap().unwrap()), [(Summary, Skipped)]);
        assert_eq!(store.session_summary(&s.id).unwrap().unwrap().points[0].text, "Eigener Punkt");
    }

    #[test]
    fn a_signed_note_is_summarized_again_only_after_a_big_change() {
        assert!(resummarize(None, false, 10));
        assert!(!resummarize(Some(1000), false, 1100), "100 chars is too little");
        assert!(!resummarize(Some(2000), false, 2250), "250 chars but under 30 %");
        assert!(resummarize(Some(500), false, 800));
        assert!(resummarize(Some(0), false, 400));
        assert!(!resummarize(Some(0), true, 4000), "her edits win");
    }
}
