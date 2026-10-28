//! The assistant loop against a scripted model: what leaves the machine is pseudonymized,
//! proposals never change data until applied, the deny-list holds, scoped threads see one
//! patient, and erasing a patient takes their threads.

use std::collections::VecDeque;

use serde_json::{json, Value};
use thera_core::agent::{self, Clock, Host, TurnEnd, Zone, ZoneChange};
use thera_core::domain::*;
use thera_core::llm::{self, ChatOptions, Completion, Endpoint, Message, ProviderId, ToolCall};
use thera_core::pseudo::TokenMap;
use thera_core::store::Store;
use time::OffsetDateTime;

struct Scripted<'a> {
    store: &'a Store,
    answers: VecDeque<Completion>,
    /// Every request body as it would be sent.
    sent: Vec<String>,
}

impl Host for Scripted<'_> {
    fn store<T>(&mut self, f: impl FnOnce(&Store) -> thera_core::Result<T>) -> thera_core::Result<T> {
        f(self.store)
    }

    fn complete(&mut self, messages: &[Message], tools: &Value, _map: &TokenMap) -> thera_core::Result<Option<Completion>> {
        let ep = Endpoint { provider: ProviderId::Openrouter, base_url: "https://openrouter.ai/api/v1".into(), api_key: None };
        self.sent.push(llm::tools_body(&ep, "m", messages, tools, &ChatOptions::default()).to_string());
        Ok(self.answers.pop_front())
    }
}

fn text(t: &str) -> Completion {
    Completion { text: t.into(), model: "m".into(), prompt_tokens: None, completion_tokens: None, reasoning_tokens: None, cost: None, latency_ms: 1, tool_calls: vec![] }
}

fn calls(list: &[(&str, Value)]) -> Completion {
    let mut c = text("");
    c.tool_calls = list
        .iter()
        .enumerate()
        .map(|(i, (name, args))| ToolCall { id: format!("c{i}"), name: (*name).into(), arguments: args.to_string() })
        .collect();
    c
}

fn clock() -> Clock {
    let zone = Zone { base: 120, changes: vec![ZoneChange { at: "2026-10-25T01:00:00Z".into(), offset: 60 }] };
    Clock::new(&zone, OffsetDateTime::parse("2026-10-28T13:05:00Z", &time::format_description::well_known::Rfc3339).unwrap()).unwrap()
}

fn patient(store: &Store, first: &str, last: &str) -> Patient {
    store
        .create_patient(PatientInput { first_name: first.into(), last_name: last.into(), birth_date: Some("1984-03-12".into()), ..Default::default() })
        .unwrap()
}

fn session(store: &Store, p: &Patient, at: &str) -> Session {
    store
        .create_session(SessionInput { patient_id: p.id.clone(), episode_id: None, scheduled_at: Some(at.into()), modality: None, planned_minutes: None })
        .unwrap()
}

fn run(store: &Store, thread: &str, question: &str, answers: Vec<Completion>) -> (TurnEnd, Vec<String>) {
    let mut host = Scripted { store, answers: answers.into(), sent: vec![] };
    let end = agent::run_turn(&mut host, thread, question, &clock()).unwrap();
    (end, host.sent)
}

#[test]
fn a_turn_reads_proposes_and_restores_but_changes_nothing() {
    let store = Store::open_in_memory().unwrap();
    let p = patient(&store, "Johanna", "Brenner");
    patient(&store, "Karl", "Albers");
    session(&store, &p, "2026-10-23T08:00:00Z");
    let thread = store.create_chat_thread(ChatScope::Global, None).unwrap();

    let (end, sent) = run(
        &store,
        &thread.id,
        "Plane einen Termin mit Frau Brenner am Montag, 2.11. um 10 Uhr.",
        vec![
            calls(&[("search_patients", json!({ "query": "[PERSON_1]" }))]),
            calls(&[("propose_session", json!({ "patient_id": p.id, "date": "2026-11-02", "time": "10:00" }))]),
            text("Ein Termin mit [PERSON_1] am Mo 2.11. um 10:00 liegt als Vorschlag bereit."),
        ],
    );
    assert_eq!(end, TurnEnd::Answered);
    assert_eq!(sent.len(), 3);
    for body in &sent {
        assert!(!body.contains("Brenner") && !body.contains("Johanna") && !body.contains("Albers") && !body.contains("1984"), "{body}");
    }
    assert!(sent[0].contains("Frau [PERSON_1]"), "{}", sent[0]);
    // The search found her by the restored name; the result went out tokenized.
    assert!(sent[1].contains(&format!("patient_id {}: [PERSON_", p.id)), "{}", sent[1]);
    assert!(sent[2].contains("Mo 2.11., 10:00"), "{}", sent[2]);

    let msgs = store.chat_messages(&thread.id).unwrap();
    let roles: Vec<ChatRole> = msgs.iter().map(|m| m.role).collect();
    use ChatRole::*;
    assert_eq!(roles, vec![User, Assistant, Tool, Assistant, Tool, Assistant]);
    assert_eq!(msgs[1].tool_calls[0].arguments, json!({ "query": "Brenner" }));
    assert!(msgs[2].content.contains("Johanna Brenner"));
    assert_eq!(msgs[5].content, "Ein Termin mit Brenner am Mo 2.11. um 10:00 liegt als Vorschlag bereit.");
    assert_eq!(store.chat_thread(&thread.id).unwrap().title.as_deref(), Some("Plane einen Termin mit Frau Brenner am Montag, 2.11. um 10…"));

    // Proposed, not done.
    let proposals = store.proposals(&thread.id).unwrap();
    assert_eq!(proposals.len(), 1);
    let prop = &proposals[0];
    assert_eq!((prop.kind, prop.status), (ProposalKind::Session, ProposalStatus::Open));
    assert_eq!(prop.summary, "Termin: Johanna Brenner, Mo 2.11., 10:00 (50 Min.)");
    assert_eq!(prop.message_id, msgs[4].id);
    assert_eq!(store.list_sessions(Some(&p.id)).unwrap().len(), 1);

    let applied = store.apply_proposal(&prop.id).unwrap();
    assert_eq!(applied.status, ProposalStatus::Applied);
    let sessions = store.list_sessions(Some(&p.id)).unwrap();
    assert_eq!(sessions.len(), 2);
    let new = sessions.iter().find(|s| Some(&s.id) == applied.result_id.as_ref()).unwrap();
    assert_eq!(new.scheduled_at.as_deref(), Some("2026-11-02T09:00:00Z"));
    assert!(store.apply_proposal(&prop.id).is_err(), "decided once");
    let audit = thera_core::audit::list(store.conn(), 50).unwrap();
    assert!(audit.iter().any(|e| e.action == "agent.apply" && e.meta_json.contains("session")));
    assert!(audit.iter().any(|e| e.action == "agent.propose"));
    assert!(!audit.iter().any(|e| e.meta_json.contains("Brenner")), "no names in the audit");
}

#[test]
fn unknown_tools_are_refused_and_nothing_is_signed() {
    let store = Store::open_in_memory().unwrap();
    let p = patient(&store, "Johanna", "Brenner");
    let s = session(&store, &p, "2026-10-23T08:00:00Z");
    let note = store.create_session_note(&s.id).unwrap();
    let thread = store.create_chat_thread(ChatScope::Global, None).unwrap();
    let (end, sent) = run(
        &store,
        &thread.id,
        "Unterschreib die Notiz.",
        vec![calls(&[("sign_report", json!({ "report_id": note.id })), ("propose_session", json!("kaputt"))]), text("Das kann ich nicht.")],
    );
    assert_eq!(end, TurnEnd::Answered);
    let msgs = store.chat_messages(&thread.id).unwrap();
    assert!(msgs[2].content.starts_with("Fehler: Das Werkzeug „sign_report“ gibt es nicht"), "{}", msgs[2].content);
    assert!(msgs[3].content.starts_with("Fehler: Die Argumente sind kein gültiges JSON-Objekt"), "{}", msgs[3].content);
    assert!(sent[1].contains("sign_report"));
    assert_eq!(store.report(&note.id).unwrap().status, ReportStatus::Draft);
    assert!(store.proposals(&thread.id).unwrap().is_empty());
}

#[test]
fn text_proposals_keep_out_of_diagnoses_and_signed_notes() {
    let store = Store::open_in_memory().unwrap();
    let p = patient(&store, "Johanna", "Brenner");
    let ep = store.create_episode(EpisodeInput { patient_id: p.id.clone(), method: Method::Vt, age_group: AgeGroup::Adult, setting: Setting::Einzel }).unwrap();
    let app = store.create_application(&ep.id, ApplicationKind::Erstantrag).unwrap();
    let report_id = app.report_id.clone().unwrap();
    let report = store.report(&report_id).unwrap();
    let prose = report.sections.iter().find(|s| s.key != "diagnose").unwrap().key.clone();
    let s = session(&store, &p, "2026-10-23T08:00:00Z");
    let thread = store.create_chat_thread(ChatScope::Patient, Some(&p.id)).unwrap();

    run(
        &store,
        &thread.id,
        "Schreib was in den Bericht.",
        vec![
            calls(&[
                ("propose_section_text", json!({ "report_id": report_id, "section_key": "diagnose", "text": "F32.1" })),
                ("propose_section_text", json!({ "report_id": report_id, "section_key": prose, "text": "Frau Brenner hat F32.1." })),
                ("propose_section_text", json!({ "report_id": report_id, "section_key": prose, "text": "Frau Brenner schläft schlecht." })),
                ("propose_note_text", json!({ "session_id": s.id, "section_key": "verlauf", "text": "Pat. berichtet von besserem Schlaf." })),
            ]),
            text("Fertig."),
        ],
    );
    let msgs = store.chat_messages(&thread.id).unwrap();
    assert!(msgs[2].content.contains("Diagnosen"), "{}", msgs[2].content);
    assert!(msgs[3].content.contains("ICD-Code"), "{}", msgs[3].content);
    let props = store.proposals(&thread.id).unwrap();
    assert_eq!(props.len(), 2);
    assert_eq!(props[0].payload["text"], "Frau Pat. schläft schlecht.".replace("Frau Pat.", "Pat."), "the Bericht gets \"Pat.\"");
    assert!(store.report(&report_id).unwrap().sections.iter().all(|x| x.body.is_empty()), "nothing written yet");
    assert!(store.session(&s.id).unwrap().note_id.is_none());

    // Applying the note text creates the note and appends to Verlauf.
    let applied = store.apply_proposal(&props[1].id).unwrap();
    let note = store.report(applied.result_id.as_deref().unwrap()).unwrap();
    let verlauf = note.sections.iter().find(|x| x.key == "verlauf").unwrap();
    assert_eq!(verlauf.body, "<p>Pat. berichtet von besserem Schlaf.</p>");
    assert_eq!(verlauf.origin, SectionOrigin::Ai);

    // A signed note takes no more proposals, not even an open one from before.
    let again = run(
        &store,
        &thread.id,
        "Noch ein Satz.",
        vec![calls(&[("propose_note_text", json!({ "session_id": s.id, "section_key": "vereinbarungen", "text": "Pat. führt Tagebuch." }))]), text("OK")],
    );
    assert_eq!(again.0, TurnEnd::Answered);
    let open = store.proposals(&thread.id).unwrap().into_iter().find(|x| x.status == ProposalStatus::Open && x.kind == ProposalKind::NoteText).unwrap();
    store.sign_report(&note.id, "T").unwrap();
    assert!(store.apply_proposal(&open.id).is_err());
    assert_eq!(store.proposal(&open.id).unwrap().status, ProposalStatus::Open);
    store.discard_proposal(&open.id).unwrap();
}

#[test]
fn a_patient_thread_sees_only_that_patient() {
    let store = Store::open_in_memory().unwrap();
    let a = patient(&store, "Johanna", "Brenner");
    let b = patient(&store, "Karl", "Albers");
    session(&store, &b, "2026-10-29T09:00:00Z");
    let thread = store.create_chat_thread(ChatScope::Patient, Some(&a.id)).unwrap();
    let (_, sent) = run(
        &store,
        &thread.id,
        "Was ist morgen?",
        vec![
            calls(&[("get_patient_overview", json!({ "patient_id": b.id })), ("calendar_day", json!({ "date": "2026-10-29" })), ("search_patients", json!({}))]),
            text("…"),
        ],
    );
    let msgs = store.chat_messages(&thread.id).unwrap();
    assert!(msgs[2].content.starts_with("Fehler: Dieses Gespräch gehört zu einer anderen Person"), "{}", msgs[2].content);
    assert!(msgs[3].content.contains("Do 29.10., 10:00, 50 Min.: belegt (andere Person)"), "{}", msgs[3].content);
    assert!(!msgs[3].content.contains("Albers") && !msgs[4].content.contains("Albers"));
    assert!(sent[0].contains(&format!("patient_id {}", a.id)), "the system prompt names the scope");
}

#[test]
fn erasing_a_patient_deletes_the_threads_that_touched_them() {
    let store = Store::open_in_memory().unwrap();
    let a = patient(&store, "Johanna", "Brenner");
    let b = patient(&store, "Karl", "Albers");
    let global = store.create_chat_thread(ChatScope::Global, None).unwrap();
    let other = store.create_chat_thread(ChatScope::Global, None).unwrap();
    let scoped = store.create_chat_thread(ChatScope::Patient, Some(&a.id)).unwrap();
    // Named only in the question: linked through the token map.
    run(&store, &global.id, "Wie geht es Herrn Brenner?", vec![text("Gut.")]);
    run(&store, &other.id, "Wer ist Herr Albers?", vec![text("Ein Patient.")]);
    store.erase_patient(&a.id).unwrap();
    assert!(store.chat_thread(&global.id).is_err());
    assert!(store.chat_thread(&scoped.id).is_err());
    assert!(store.chat_thread(&other.id).is_ok());
    store.erase_patient(&b.id).unwrap();
    assert!(store.chat_threads(None, None, 10).unwrap().is_empty());
}

#[test]
fn the_loop_stops_after_max_steps_and_on_cancel() {
    let store = Store::open_in_memory().unwrap();
    let thread = store.create_chat_thread(ChatScope::Global, None).unwrap();
    let busy: Vec<Completion> = (0..agent::MAX_STEPS).map(|_| calls(&[("due_items", json!({}))])).collect();
    let (end, sent) = run(&store, &thread.id, "Was ist fällig?", busy);
    assert_eq!((end, sent.len()), (TurnEnd::StepLimit, agent::MAX_STEPS));
    assert!(store.chat_messages(&thread.id).unwrap().last().unwrap().content.contains("aufgehört"));
    // No answer = cancelled: the question stays, nothing else is added.
    let before = store.chat_messages(&thread.id).unwrap().len();
    let (end, _) = run(&store, &thread.id, "Und jetzt?", vec![]);
    assert_eq!(end, TurnEnd::Cancelled);
    assert_eq!(store.chat_messages(&thread.id).unwrap().len(), before + 1);
    // Hand-typed tokens are refused before anything is stored.
    let mut host = Scripted { store: &store, answers: VecDeque::new(), sent: vec![] };
    assert!(agent::run_turn(&mut host, &thread.id, "Wer ist [PERSON_1]?", &clock()).is_err());
}
