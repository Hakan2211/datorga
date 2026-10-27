//! Bericht drafting per section (Phase 2, slice 14): the facts the therapist applied to the
//! Sitzungsnotizen of an episode become a draft for one Bericht section, sentence by sentence,
//! each sentence citing the facts it rests on (fact → transcript segment via `fact_citation`).
//!
//! The model sees numbered, pseudonymized facts (`[F7] (Befund) …`), one request per section.
//! The patient is written as "Pat." before pseudonymizing, since the Bericht goes to the Gutachter
//! under a Chiffre. A sentence that still names someone else (a placeholder for a person, place or
//! employer) is dropped rather than restored: those names must not reach the Gutachter either.

use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::{AgeGroup, ApplicationKind, Method, Patient};
use crate::extract::{citation_number, json_list, sha256, Preview, TokenView, ICD};
use crate::pseudo::{DetectionSource, Dictionary, EntityKind, Pseudonymized, Pseudonymizer, TokenMap};
use crate::templates::{FieldKind, SectionDef, Template};
use crate::{Error, Result};

/// Bump on any change to the prompt or the answer format.
pub const PROMPT_VERSION: &str = "draft-2026.1";

/// Fact characters per request, like `extract::CHUNK_CHARS` (fits a 16k-context local model).
pub const FACT_CHARS: usize = 24_000;

/// Longest sentence kept.
const MAX_SENTENCE_CHARS: usize = 500;

/// Longest instruction the therapist may add to a request.
pub const MAX_INSTRUCTION_CHARS: usize = 500;

/// An applied Sitzungsnotiz fact as input to a draft.
#[derive(Debug, Clone)]
pub struct InputFact {
    /// 1-based position of its session among the episode's sessions with facts.
    pub session_no: usize,
    /// Heading of the note section it was filed under ("Befund").
    pub label: String,
    pub text: String,
}

/// One request: the draft for section `key`.
pub struct PreparedSection {
    pub key: String,
    pub user: Pseudonymized<String>,
}

pub struct Prepared {
    pub system: Pseudonymized<String>,
    pub sections: Vec<PreparedSection>,
    pub map: TokenMap,
    /// SHA-256 of version + system prompt (no patient data), stored with the draft.
    pub prompt_hash: String,
    /// SHA-256 over all outgoing messages: what the preview showed is what is sent.
    pub digest: String,
}

impl Prepared {
    pub fn preview(&self) -> Preview {
        Preview {
            messages: self.sections.iter().map(|s| s.user.get().clone()).collect(),
            tokens: self.map.entries().iter().map(TokenView::from).collect(),
            digest: self.digest.clone(),
        }
    }
}

/// A sentence as proposed by the model, dates restored, citations as 0-based fact indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftSentence {
    pub text: String,
    pub facts: Vec<usize>,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub sentences: Vec<DraftSentence>,
    pub dropped: usize,
}

/// Sections the model may draft: prose sections that allow AI text.
pub fn target_sections(template: &Template) -> Vec<&SectionDef> {
    template.sections.iter().filter(|s| s.field == FieldKind::Prose && s.ai_allowed).collect()
}

/// What the Bericht is for, in the words of the prompt. Holds no patient data.
pub struct Frame {
    pub method: Method,
    pub age_group: AgeGroup,
    pub kind: ApplicationKind,
}

fn method_name(m: Method) -> &'static str {
    match m {
        Method::Vt => "Verhaltenstherapie",
        Method::Tp => "tiefenpsychologisch fundierte Psychotherapie",
        Method::Ap => "analytische Psychotherapie",
    }
}

fn kind_name(k: ApplicationKind) -> &'static str {
    match k {
        ApplicationKind::Erstantrag => "Erstantrag",
        ApplicationKind::Umwandlung => "Umwandlungsantrag",
        ApplicationKind::Fortfuehrung => "Fortführungsantrag",
    }
}

/// The system prompt for a (resolved) Bericht template. Holds no patient data.
pub fn system_prompt(template: &Template, frame: &Frame) -> String {
    let age = if frame.age_group == AgeGroup::Kjp { "Kinder und Jugendliche" } else { "Erwachsene" };
    let style = if template.rules.keyword_style_allowed {
        "Sachlich und knapp; Stichworte sind erlaubt, ganze Sätze sind besser lesbar."
    } else {
        "Sachlich und knapp, in ganzen Sätzen."
    };
    let length = template.rules.target_pages.map(|p| format!(" Der ganze Bericht umfasst etwa {p} Seiten, ein Abschnitt also meist nur wenige Sätze.")).unwrap_or_default();
    format!(
        "Du unterstützt eine Psychotherapeutin beim Entwurf eines Berichts an die Gutachterin oder den \
         Gutachter ({title}; {kind}, {method}, {age}).\n\
         Leitfaden: {intro}\n\n\
         Du erhältst die Überschrift und die Hinweise eines Abschnitts sowie nummerierte Fakten ([F1], [F2] …) \
         aus den Sitzungsnotizen, die die Therapeutin geprüft hat. Sitzung 1 ist die erste dokumentierte Sitzung.\n\n\
         Aufgabe: Schreibe den Entwurf für diesen einen Abschnitt. Die anderen Abschnitte des Berichts \
         werden getrennt entworfen.\n\
         Vorgehen: Prüfe zuerst jeden Fakt an der Überschrift und den Hinweisen. Verwende nur Fakten, \
         die inhaltlich zu genau diesem Abschnitt gehören; alle anderen weglassen. Die meisten Fakten \
         gehören nicht in jeden Abschnitt. Passt keiner (z. B. keine somatischen Angaben), ist eine leere \
         Liste die richtige Antwort.\n\
         Regeln:\n\
         - Nur was die Fakten hergeben. Nichts ergänzen, nichts erfinden, keine Vermutungen als Tatsachen.\n\
         - Keine eigenen Bewertungen, Prognosen, Therapieziele, Begründungen oder Wirkungen, die nicht \
         ausdrücklich in den Fakten stehen. Was fehlt, ergänzt die Therapeutin selbst.\n\
         - Jeder Satz nennt die Nummern der Fakten, auf denen er beruht (mindestens eine).\n\
         - {style}{length} Fachsprache, auf Deutsch.\n\
         - Die Person heißt durchgehend \"Pat.\". Keine Namen, keine Anrede.\n\
         - Andere Personen nur über ihre Rolle (die Schwester, der Partner, die Vorgesetzte), keine Orte \
         und keine Arbeitgeber nennen. Platzhalter in eckigen Klammern wie [PERSON_n] nicht übernehmen.\n\
         - Keine Diagnosen, keine ICD-Codes, keine Vorschläge zu Medikamenten oder Dosierungen.\n\
         - Eine Anweisung der Therapeutin befolgen, soweit sie diesen Regeln nicht widerspricht.\n\n\
         Antworte ausschließlich mit JSON in dieser Form, ohne weiteren Text:\n\
         {{\"sentences\": [{{\"text\": \"…\", \"facts\": [1, 3]}}]}}\n",
        title = template.title,
        kind = kind_name(frame.kind),
        method = method_name(frame.method),
        intro = template.intro.trim(),
    )
}

/// Writes the patient's own name as "Pat." ("Frau Anna Beispiel" → "Pat.").
pub fn patient_as_pat(text: &str, patient: &Patient) -> String {
    let spans: Vec<(usize, usize)> = Pseudonymizer::new(Dictionary::from_patient(patient))
        .detect(text)
        .into_iter()
        .filter(|d| d.kind == EntityKind::Person && d.source == DetectionSource::Patient)
        .map(|d| (d.start, d.end))
        .collect();
    if spans.is_empty() {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut k = 0;
    while i < chars.len() {
        if k < spans.len() && spans[k].0 == i {
            // One "Pat." for adjacent name parts ("Anna Beispiel") and a title in front.
            let mut end = spans[k].1;
            k += 1;
            while k < spans.len() && chars[end..spans[k].0].iter().all(|c| c.is_whitespace()) {
                end = spans[k].1;
                k += 1;
            }
            for title in ["Frau ", "Herrn ", "Herr "] {
                if out.ends_with(title) {
                    out.truncate(out.len() - title.len());
                    break;
                }
            }
            out.push_str("Pat.");
            // Genitive "Beispiels" → "Pat."; the dictionary leaves the s outside the span.
            if chars.get(end) == Some(&'s') && chars.get(end + 1).is_none_or(|c| !c.is_alphanumeric()) {
                end += 1;
            }
            i = end;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn section_message(def: &SectionDef, instruction: Option<&str>, facts: &str) -> String {
    let mut m = String::from("Abschnitt: ");
    if let Some(n) = &def.number {
        m.push_str(&format!("{n}. "));
    }
    m.push_str(def.heading.trim());
    m.push('\n');
    if !def.hints.is_empty() {
        m.push_str("Hinweise des Leitfadens:\n");
        for h in &def.hints {
            m.push_str(&format!("- {}\n", h.text.trim()));
        }
    }
    if let Some(i) = instruction {
        m.push_str(&format!("Anweisung der Therapeutin: {i}\n"));
    }
    m.push_str("\nFakten aus den Sitzungsnotizen:\n");
    m.push_str(facts);
    // Small models follow what comes last: repeat the scope after the long fact list.
    m.push_str(&format!(
        "\nSchreibe nur Abschnitt \"{}\". Fakten, die nicht dazu gehören, weglassen; passt keiner, leere Liste.\n",
        def.heading.trim()
    ));
    m
}

/// Normalizes the therapist's optional instruction ("kürzer", "Schwerpunkt Schlaf").
pub fn clean_instruction(instruction: Option<&str>) -> Result<Option<String>> {
    let Some(i) = instruction.map(|i| i.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|i| !i.is_empty()) else {
        return Ok(None);
    };
    if i.chars().count() > MAX_INSTRUCTION_CHARS {
        return Err(Error::Invalid(format!("Die Anweisung ist zu lang (höchstens {MAX_INSTRUCTION_CHARS} Zeichen)")));
    }
    Ok(Some(i))
}

/// Everything that leaves the machine for drafting `keys`, one pseudonymizer for all requests.
pub fn prepare(
    template: &Template,
    frame: &Frame,
    keys: &[String],
    facts: &[InputFact],
    patient: &Patient,
    dict: Dictionary,
    instruction: Option<&str>,
) -> Result<Prepared> {
    if facts.is_empty() {
        return Err(Error::Invalid(
            "Keine übernommenen Fakten aus Sitzungsnotizen dieses Behandlungsfalls. Zuerst in den Sitzungsnotizen Fakten übernehmen.".into(),
        ));
    }
    let targets = target_sections(template);
    let mut defs = Vec::new();
    for key in keys {
        let def = targets.iter().find(|d| &d.key == key).ok_or_else(|| Error::Invalid(format!("Abschnitt {key} kann nicht entworfen werden")))?;
        if !defs.iter().any(|d: &&SectionDef| d.key == def.key) {
            defs.push(*def);
        }
    }
    if defs.is_empty() {
        return Err(Error::Invalid("Kein Abschnitt gewählt".into()));
    }
    let instruction = clean_instruction(instruction)?;

    let mut list = String::new();
    let mut session = 0;
    for (i, f) in facts.iter().enumerate() {
        if f.session_no != session {
            session = f.session_no;
            list.push_str(&format!("Sitzung {session}:\n"));
        }
        list.push_str(&format!("[F{}] ({}) {}\n", i + 1, f.label, patient_as_pat(f.text.trim(), patient)));
    }
    if list.chars().count() > FACT_CHARS {
        return Err(Error::Invalid(format!(
            "Zu viele Fakten für einen Entwurf ({} Zeichen, höchstens {FACT_CHARS})",
            list.chars().count()
        )));
    }
    let instruction = instruction.map(|i| patient_as_pat(&i, patient));

    let mut p = Pseudonymizer::new(dict);
    let system_raw = system_prompt(template, frame);
    let prompt_hash = sha256(&format!("{PROMPT_VERSION}\n{system_raw}"));
    let system = p.pseudonymize(&system_raw)?;
    let mut sections = Vec::new();
    for def in defs {
        let user = p.pseudonymize(&section_message(def, instruction.as_deref(), &list))?;
        sections.push(PreparedSection { key: def.key.clone(), user });
    }
    let mut h = Sha256::new();
    h.update(system.get());
    for s in &sections {
        h.update([0]);
        h.update(s.user.get());
    }
    Ok(Prepared { system, sections, map: p.into_map(), prompt_hash, digest: hex::encode(h.finalize()) })
}

static TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(PERSON|ORT|ARBEITGEBER|DATUM)_\d+\]").expect("valid regex"));
// Citation markers some models write into the text as well ("… pro Woche [F1].", "(F2, F3)").
static MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*[\[(]F\s?\d+(?:\s*[,;]\s*F?\s?\d+)*[\])]").expect("valid regex"));

#[derive(Deserialize)]
struct RawSentence {
    #[serde(default)]
    text: String,
    #[serde(default)]
    facts: Vec<Value>,
}

/// Checks a model answer for a request with `n_facts` facts. Fails only without any JSON.
pub fn parse(answer: &str, n_facts: usize, map: &TokenMap) -> Result<Parsed> {
    let mut out = Parsed::default();
    for item in json_list(answer, "sentences")? {
        let Ok(raw) = serde_json::from_value::<RawSentence>(item) else {
            out.dropped += 1;
            continue;
        };
        match check(raw, n_facts, map) {
            Some(s) if !out.sentences.iter().any(|t| t.text == s.text) => out.sentences.push(s),
            Some(_) => {}
            None => out.dropped += 1,
        }
    }
    Ok(out)
}

fn check(raw: RawSentence, n_facts: usize, map: &TokenMap) -> Option<DraftSentence> {
    let text = MARKER.replace_all(&raw.text, "").split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() || text.chars().count() > MAX_SENTENCE_CHARS || ICD.is_match(&text) {
        return None;
    }
    let mut facts = Vec::new();
    for v in &raw.facts {
        let n = citation_number(v, 'F')?;
        if n == 0 || n > n_facts {
            return None;
        }
        if !facts.contains(&(n - 1)) {
            facts.push(n - 1);
        }
    }
    if facts.is_empty() {
        return None;
    }
    facts.sort_unstable();
    // Only dates come back; a name, place or employer token means the sentence names someone.
    if TOKEN.captures_iter(&text).any(|c| &c[1] != "DATUM") {
        return None;
    }
    let restored = map.restore(&text);
    if !restored.unknown.is_empty() {
        return None;
    }
    Some(DraftSentence { text: restored.text, facts })
}

/// The sentences as one paragraph of TipTap HTML.
pub fn html<'a>(sentences: impl IntoIterator<Item = &'a str>) -> String {
    let text: Vec<String> = sentences.into_iter().map(escape).collect();
    format!("<p>{}</p>", text.join(" "))
}

fn escape(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::{self, Context};

    fn bericht() -> Template {
        let t = templates::builtin().unwrap().into_iter().find(|t| t.key == "ptv3-erst-umwandlung").unwrap();
        t.resolve(&Context { method: Method::Vt, age_group: AgeGroup::Adult, kind: Some(ApplicationKind::Erstantrag) })
    }

    fn frame() -> Frame {
        Frame { method: Method::Vt, age_group: AgeGroup::Adult, kind: ApplicationKind::Erstantrag }
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
            consent_recording: false,
            status: "active".into(),
            created_at: String::new(),
            archived_at: None,
        }
    }

    fn fact(session_no: usize, label: &str, text: &str) -> InputFact {
        InputFact { session_no, label: label.into(), text: text.into() }
    }

    fn keys(k: &[&str]) -> Vec<String> {
        k.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn prompt_has_frame_and_no_tokens() {
        let p = system_prompt(&bericht(), &frame());
        assert!(p.contains("Erstantrag") && p.contains("Verhaltenstherapie") && p.contains("Erwachsene"));
        assert!(p.contains("etwa 2 Seiten"));
        assert!(Pseudonymizer::new(Dictionary::new()).pseudonymize(&p).is_ok());
        let targets: Vec<_> = target_sections(&bericht()).iter().map(|s| s.key.clone()).collect();
        assert!(targets.contains(&"symptomatik_befund".to_string()));
        assert!(!targets.contains(&"diagnose".to_string()), "diagnoses stay with the therapist");
        assert!(!targets.contains(&"umwandlung".to_string()), "resolved for the Erstantrag");
    }

    #[test]
    fn patient_becomes_pat() {
        let p = patient();
        assert_eq!(patient_as_pat("Frau Anna Beispiel berichtet, Beispiels Schwester hilft.", &p), "Pat. berichtet, Pat. Schwester hilft.");
        assert_eq!(patient_as_pat("Herr Muster und Anna", &p), "Herr Muster und Pat.");
        assert_eq!(patient_as_pat("Annabell kam", &p), "Annabell kam");
        assert_eq!(patient_as_pat("Schläft schlecht.", &p), "Schläft schlecht.");
    }

    #[test]
    fn prepare_numbers_facts_and_hides_names() {
        let mut dict = Dictionary::from_patient(&patient());
        dict.add(EntityKind::Person, "Gerda");
        let facts = vec![
            fact(1, "Verlauf", "Anna Beispiel berichtet Schlafstörungen seit dem 3.5.2024."),
            fact(1, "Befund", "Stimmung gedrückt"),
            fact(2, "Verlauf", "Streit mit Schwester Gerda"),
        ];
        let p = prepare(&bericht(), &frame(), &keys(&["symptomatik_befund", "soziodemographie", "symptomatik_befund"]), &facts, &patient(), dict.clone(), Some("  kürzer   bitte ")).unwrap();
        assert_eq!(p.sections.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(), ["symptomatik_befund", "soziodemographie"]);
        let m = p.sections[0].user.get();
        assert!(m.starts_with("Abschnitt: 2. Symptomatik und psychischer Befund\nHinweise des Leitfadens:\n- "), "{m}");
        assert!(m.contains("Anweisung der Therapeutin: kürzer bitte\n"));
        assert!(m.contains("Sitzung 1:\n[F1] (Verlauf) Pat. berichtet Schlafstörungen seit dem [DATUM_1].\n[F2] (Befund) Stimmung gedrückt\nSitzung 2:\n[F3] (Verlauf) Streit mit Schwester [PERSON_1]"), "{m}");
        assert!(!m.contains("Anna") && !m.contains("Beispiel") && !m.contains("Gerda"));
        let again = prepare(&bericht(), &frame(), &keys(&["symptomatik_befund", "soziodemographie"]), &facts, &patient(), dict.clone(), Some("kürzer bitte")).unwrap();
        assert_eq!(again.digest, p.digest);
        assert_ne!(prepare(&bericht(), &frame(), &keys(&["symptomatik_befund", "soziodemographie"]), &facts, &patient(), dict.clone(), None).unwrap().digest, p.digest);

        assert!(prepare(&bericht(), &frame(), &keys(&["diagnose"]), &facts, &patient(), dict.clone(), None).is_err(), "not AI-draftable");
        assert!(prepare(&bericht(), &frame(), &keys(&[]), &facts, &patient(), dict.clone(), None).is_err());
        assert!(prepare(&bericht(), &frame(), &keys(&["somatik"]), &[], &patient(), dict.clone(), None).is_err(), "no facts");
        let long = "x".repeat(MAX_INSTRUCTION_CHARS + 1);
        assert!(prepare(&bericht(), &frame(), &keys(&["somatik"]), &facts, &patient(), dict.clone(), Some(&long)).is_err());
        let many: Vec<_> = (0..300).map(|_| fact(1, "Verlauf", &"y".repeat(100))).collect();
        assert!(prepare(&bericht(), &frame(), &keys(&["somatik"]), &many, &patient(), dict, None).is_err(), "over budget");
    }

    #[test]
    fn parse_checks_citations_and_names() {
        let facts = vec![fact(1, "Verlauf", "Seit dem 3.5.2024 Schlafstörungen"), fact(1, "Verlauf", "Schwester Gerda hilft")];
        let mut dict = Dictionary::from_patient(&patient());
        dict.add(EntityKind::Person, "Gerda");
        let p = prepare(&bericht(), &frame(), &keys(&["symptomatik_befund"]), &facts, &patient(), dict, None).unwrap();
        let answer = r#"<think>…</think>```json
        {"sentences": [
          {"text": "Pat.  berichtet seit dem [DATUM_1]\nSchlafstörungen.", "facts": [1]},
          {"text": "Pat. berichtet seit dem [DATUM_1] Schlafstörungen.", "facts": ["F1"]},
          {"text": "Unterstützung durch die Schwester [F2] (F1, F2).", "facts": ["[F2]", 2, 1]},
          {"text": "Unterstützung durch [PERSON_1].", "facts": [2]},
          {"text": "Verdacht auf F51.0.", "facts": [1]},
          {"text": "Ohne Beleg.", "facts": []},
          {"text": "Falscher Beleg.", "facts": [3]},
          {"text": "Erfunden [ORT_4].", "facts": [1]},
          {"text": "", "facts": [1]},
          42
        ]}
        ```"#;
        let out = parse(answer, facts.len(), &p.map).unwrap();
        assert_eq!(
            out.sentences,
            vec![
                DraftSentence { text: "Pat. berichtet seit dem 3.5.2024 Schlafstörungen.".into(), facts: vec![0] },
                DraftSentence { text: "Unterstützung durch die Schwester.".into(), facts: vec![0, 1] },
            ]
        );
        assert_eq!(out.dropped, 7);
        assert!(parse("[]", 2, &p.map).unwrap().sentences.is_empty());
        assert!(parse("Geht nicht.", 2, &p.map).is_err());
        assert_eq!(html(out.sentences.iter().map(|s| s.text.as_str())), "<p>Pat. berichtet seit dem 3.5.2024 Schlafstörungen. Unterstützung durch die Schwester.</p>");
        assert_eq!(html(["a < b & c"]), "<p>a &lt; b &amp; c</p>");
    }
}
