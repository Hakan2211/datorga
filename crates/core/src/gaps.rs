//! Bericht-Lotse "Fragen" (Phase 5, slice 38): the template's questions (`asks:`) that the session
//! facts don't answer yet, so the therapist can answer them before the draft instead of finding
//! thin sections afterwards.
//!
//! One pseudonymized request per Bericht: the open questions and the facts a draft would see. The
//! model names the questions the facts already answer and the facts that do; a question counts as
//! answered only with a cited fact that a draft of its section would see. Everything else stays a
//! question. Without an AI model every question is shown, which works too.

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::contingent;
use crate::domain::{EpisodeFact, Gender, Patient};
use crate::draft::{fact_list, patient_as_pat, section_facts, InputFact, FACT_CHARS};
use crate::extract::{citation_number, json_list, sha256, Preview, TokenView};
use crate::llm::{ChatOptions, Effort};
use crate::pseudo::{Dictionary, Pseudonymized, Pseudonymizer, TokenMap};
use crate::templates::{Ask, RecordField, SectionDef, Template};
use crate::{Error, Result};

/// Bump on any change to the prompt or the answer format.
pub const PROMPT_VERSION: &str = "gaps-2026.1";

/// Longest answer kept (a few sentences; the draft turns it into the section's text).
pub const MAX_ANSWER_CHARS: usize = 2_000;

/// One short answer for the whole Bericht, medium effort like a draft.
pub fn chat_options() -> ChatOptions {
    ChatOptions { temperature: 0.1, max_tokens: 4096, timeout: Duration::from_secs(600), reasoning: Some(Effort::Medium) }
}

/// A question of a resolved template, with its section.
#[derive(Debug, Clone, Copy)]
pub struct Question<'a> {
    pub section: &'a SectionDef,
    pub ask: &'a Ask,
}

/// The questions of a (resolved) template, in order, leaving out sections `skip` names.
pub fn questions<'a>(template: &'a Template, skip: impl Fn(&str) -> bool) -> Vec<Question<'a>> {
    template
        .sections
        .iter()
        .filter(|s| !skip(&s.key))
        .flat_map(|section| section.asks.iter().map(move |ask| Question { section, ask }))
        .collect()
}

/// The answer the patient record gives ("41 Jahre, weiblich"), offered as a chip.
pub fn record_answer(field: RecordField, patient: &Patient, now: &str) -> Option<String> {
    match field {
        RecordField::AgeGender => {
            let age = patient.birth_date.as_deref().and_then(|b| contingent::age(b, now)).map(|a| format!("{a} Jahre"));
            let gender = patient.gender.map(|g| match g {
                Gender::W => "weiblich",
                Gender::M => "männlich",
                Gender::D => "divers",
            });
            let parts: Vec<String> = age.into_iter().chain(gender.map(String::from)).collect();
            (!parts.is_empty()).then(|| parts.join(", "))
        }
    }
}

/// The facts a check saw (ids, text, tags): a different digest later means new or changed facts.
pub fn facts_digest(facts: &[EpisodeFact]) -> String {
    let mut h = Sha256::new();
    for f in facts {
        h.update(&f.fact_id);
        h.update([0]);
        h.update(&f.text);
        h.update([0]);
        h.update(f.bericht.as_ref().map(|b| b.join(",")).unwrap_or_else(|| "*".into()));
        h.update([1]);
    }
    hex::encode(h.finalize())
}

pub struct Prepared {
    pub system: Pseudonymized<String>,
    pub user: Pseudonymized<String>,
    /// (section key, ask key) per question number - 1.
    pub asks: Vec<(String, String)>,
    /// Per question: the facts a draft of its section sees (only these may answer it).
    pub subsets: Vec<Vec<usize>>,
    pub map: TokenMap,
    /// SHA-256 of version + system prompt (no patient data).
    pub prompt_hash: String,
    /// SHA-256 over both messages: what the preview showed is what is sent.
    pub digest: String,
}

impl Prepared {
    pub fn preview(&self) -> Preview {
        Preview {
            messages: vec![self.user.get().clone()],
            tokens: self.map.entries().iter().map(TokenView::from).collect(),
            digest: self.digest.clone(),
        }
    }
}

/// The system prompt. Holds no patient data.
pub fn system_prompt() -> String {
    "Du unterstützt eine Psychotherapeutin bei einem Bericht an die Gutachterin oder den Gutachter. \
     Du erhältst nummerierte Fragen ([Q1], [Q2] …) zu Abschnitten des Berichts und nummerierte Fakten \
     ([F1], [F2] …) aus ihren geprüften Sitzungsnotizen.\n\n\
     Aufgabe: Finde die Fragen, die die Fakten schon beantworten, damit die Therapeutin nur noch die \
     übrigen selbst beantworten muss.\n\
     Regeln:\n\
     - Eine Frage gilt nur als beantwortet, wenn Fakten die Antwort ausdrücklich enthalten. Ein Fakt, \
     der das Thema nur streift, reicht nicht.\n\
     - Fragt eine Frage nach mehreren Dingen, gilt sie nur als beantwortet, wenn die Fakten das \
     Wesentliche davon enthalten.\n\
     - Im Zweifel gilt die Frage als offen.\n\
     - Nenne zu jeder beantworteten Frage im Feld \"facts\" die Nummern der Fakten, die sie beantworten.\n\
     - Offene Fragen weglassen. Beantwortet kein Fakt eine Frage, ist eine leere Liste die richtige Antwort.\n\n\
     Antworte ausschließlich mit JSON in dieser Form, ohne weiteren Text:\n\
     {\"answered\": [{\"question\": 3, \"facts\": [1, 4]}]}\n"
        .to_string()
}

/// Everything that leaves the machine for checking `questions` against the session `facts`.
pub fn prepare(questions: &[Question], facts: &[InputFact], tag_keys: &[String], patient: &Patient, dict: Dictionary) -> Result<Prepared> {
    if questions.is_empty() {
        return Err(Error::Invalid("Keine offenen Fragen zum Prüfen".into()));
    }
    if facts.is_empty() {
        return Err(Error::Invalid("Keine übernommenen Fakten, die Fragen beantworten könnten".into()));
    }
    let all: Vec<usize> = (0..facts.len()).collect();
    let list = fact_list(facts, &all, patient);
    let chars = list.chars().count();
    if chars > FACT_CHARS {
        return Err(Error::Invalid(format!("Zu viele Fakten für die Prüfung ({chars} Zeichen, höchstens {FACT_CHARS})")));
    }
    let mut user = String::from("Fragen:\n");
    let (mut asks, mut subsets) = (Vec::new(), Vec::new());
    for (i, q) in questions.iter().enumerate() {
        let number = q.section.number.as_deref().map(|n| format!("{n}. ")).unwrap_or_default();
        user.push_str(&format!(
            "[Q{}] (Abschnitt {number}{}) {}\n",
            i + 1,
            short_heading(&q.section.heading),
            patient_as_pat(q.ask.question.trim(), patient)
        ));
        asks.push((q.section.key.clone(), q.ask.key.clone()));
        subsets.push(section_facts(facts, &q.section.key, tag_keys));
    }
    user.push_str("\nFakten aus den Sitzungsnotizen:\n");
    user.push_str(&list);
    user.push_str("\nNenne nur Fragen, die diese Fakten ausdrücklich beantworten.\n");

    let mut p = Pseudonymizer::new(dict);
    let system_raw = system_prompt();
    let prompt_hash = sha256(&format!("{PROMPT_VERSION}\n{system_raw}"));
    let system = p.pseudonymize(&system_raw)?;
    let user = p.pseudonymize(&user)?;
    let mut h = Sha256::new();
    h.update(system.get());
    h.update([0]);
    h.update(user.get());
    Ok(Prepared { system, user, asks, subsets, map: p.into_map(), prompt_hash, digest: hex::encode(h.finalize()) })
}

/// The heading up to its first comma or parenthesis: the long PTV 3 headings are noise here.
fn short_heading(heading: &str) -> &str {
    let h = heading.trim();
    h.find([',', '(']).map_or(h, |i| h[..i].trim_end())
}

/// A question the facts answer: its keys and the facts (indices into the check's facts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Covered {
    pub section_key: String,
    pub ask_key: String,
    pub facts: Vec<usize>,
}

#[derive(Deserialize)]
struct RawCovered {
    #[serde(default)]
    question: Value,
    #[serde(default)]
    facts: Vec<Value>,
}

/// Checks the model's answer. An entry counts only with a known question and at least one fact a
/// draft of its section would see; the rest is dropped (counted). Fails only without any JSON.
pub fn parse(answer: &str, prepared: &Prepared) -> Result<(Vec<Covered>, usize)> {
    let (mut out, mut dropped) = (Vec::<Covered>::new(), 0);
    for item in json_list(answer, "answered")? {
        let Ok(raw) = serde_json::from_value::<RawCovered>(item) else {
            dropped += 1;
            continue;
        };
        let Some(q) = citation_number(&raw.question, 'Q').filter(|&n| n >= 1 && n <= prepared.asks.len()).map(|n| n - 1) else {
            dropped += 1;
            continue;
        };
        let shown = &prepared.subsets[q];
        let mut facts: Vec<usize> = raw
            .facts
            .iter()
            .filter_map(|v| citation_number(v, 'F'))
            .filter(|&n| n >= 1 && shown.contains(&(n - 1)))
            .map(|n| n - 1)
            .collect();
        facts.sort_unstable();
        facts.dedup();
        let (section_key, ask_key) = prepared.asks[q].clone();
        if facts.is_empty() || out.iter().any(|c| c.section_key == section_key && c.ask_key == ask_key) {
            dropped += 1;
            continue;
        }
        out.push(Covered { section_key, ask_key, facts });
    }
    Ok((out, dropped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgeGroup, ApplicationKind, Method};
    use crate::pseudo::EntityKind;
    use crate::templates::{self, Context};

    fn bericht() -> Template {
        let t = templates::builtin().unwrap().into_iter().find(|t| t.key == "ptv3-erst-umwandlung").unwrap();
        t.resolve(&Context { method: Method::Vt, age_group: AgeGroup::Adult, kind: Some(ApplicationKind::Erstantrag) })
    }

    fn patient() -> Patient {
        Patient {
            id: "p".into(),
            external_ref: None,
            first_name: "Anna".into(),
            last_name: "Beispiel".into(),
            birth_date: Some("1985-03-14".into()),
            chiffre: Some("B140385".into()),
            pronouns: None,
            gender: Some(Gender::W),
            consent_recording: false,
            status: "active".into(),
            created_at: String::new(),
            archived_at: None,
        }
    }

    fn tagged(session_no: usize, text: &str, tags: &[&str]) -> InputFact {
        InputFact { session_no, label: "Verlauf".into(), text: text.into(), bericht: Some(tags.iter().map(|t| t.to_string()).collect()), own: false }
    }

    #[test]
    fn record_answer_from_birth_date_and_gender() {
        let p = patient();
        assert_eq!(record_answer(RecordField::AgeGender, &p, "2026-10-29T10:00:00Z").as_deref(), Some("41 Jahre, weiblich"));
        let p = Patient { birth_date: None, gender: None, ..patient() };
        assert_eq!(record_answer(RecordField::AgeGender, &p, "2026-10-29"), None);
    }

    #[test]
    fn questions_skip_sections_and_check_covers_only_shown_facts() {
        let t = bericht();
        let all = questions(&t, |_| false);
        assert!(all.iter().any(|q| q.ask.key == "medikation"));
        let qs: Vec<_> = questions(&t, |k| k == "somatik").into_iter().filter(|q| q.ask.record.is_none()).collect();
        assert!(!qs.iter().any(|q| q.section.key == "somatik"));
        let beruf = qs.iter().position(|q| q.ask.key == "beruf").unwrap() + 1;
        let ziele = qs.iter().position(|q| q.ask.key == "ziele").unwrap() + 1;
        let tests = qs.iter().position(|q| q.ask.key == "tests").unwrap() + 1;

        let mut dict = Dictionary::from_patient(&patient());
        dict.add(EntityKind::Arbeitgeber, "Kita Sonnenschein");
        let facts = vec![
            tagged(1, "Anna arbeitet als Erzieherin in der Kita Sonnenschein", &["soziodemographie"]),
            tagged(2, "Ziel: wieder allein Bus fahren", &["behandlungsplan_prognose"]),
            tagged(2, "BDI-II 28", &["symptomatik_befund"]),
        ];
        let tags: Vec<String> = t.sections.iter().map(|s| s.key.clone()).collect();
        let p = prepare(&qs, &facts, &tags, &patient(), dict).unwrap();
        let m = p.user.get();
        assert!(m.starts_with("Fragen:\n[Q1] (Abschnitt 1. Relevante soziodemographische Daten) "), "{m}");
        assert!(m.contains("(Abschnitt 4. Behandlungsrelevante Angaben zur Lebensgeschichte) "), "short heading: {m}");
        assert!(m.contains("Sitzung 1:\n[F1] (Verlauf) Pat. arbeitet als Erzieherin in der [ARBEITGEBER_1]"), "{m}");
        assert!(!m.contains("Anna") && !m.contains("Sonnenschein"));
        assert_eq!(p.preview().messages.len(), 1);

        let answer = format!(
            r#"{{"answered": [
              {{"question": "Q{beruf}", "facts": ["F1"]}},
              {{"question": {ziele}, "facts": [1]}},
              {{"question": {tests}, "facts": [3, 3]}},
              {{"question": {tests}, "facts": [3]}},
              {{"question": 99, "facts": [1]}},
              {{"question": 1, "facts": []}}
            ]}}"#
        );
        let (covered, dropped) = parse(&answer, &p).unwrap();
        assert_eq!(covered, vec![
            Covered { section_key: "soziodemographie".into(), ask_key: "beruf".into(), facts: vec![0] },
            Covered { section_key: "symptomatik_befund".into(), ask_key: "tests".into(), facts: vec![2] },
        ]);
        assert_eq!(dropped, 4, "the goal fact isn't one §1 sees; duplicate, unknown and uncited entries");
        assert!(parse("[]", &p).unwrap().0.is_empty());
        assert!(parse("Weiß nicht.", &p).is_err());

        assert!(prepare(&[], &facts, &tags, &patient(), Dictionary::new()).is_err());
        assert!(prepare(&qs, &[], &tags, &patient(), Dictionary::new()).is_err());
    }

    #[test]
    fn digest_follows_the_facts() {
        let f = |id: &str, text: &str| EpisodeFact {
            fact_id: id.into(),
            session_id: "s".into(),
            note_id: "n".into(),
            session_at: None,
            session_no: 1,
            section_key: "verlauf".into(),
            text: text.into(),
            bericht: None,
        };
        let a = facts_digest(&[f("1", "a"), f("2", "b")]);
        assert_eq!(a, facts_digest(&[f("1", "a"), f("2", "b")]));
        assert_ne!(a, facts_digest(&[f("1", "a"), f("2", "c")]));
        assert_ne!(a, facts_digest(&[f("1", "a")]));
    }
}
