//! Bericht-Lotse "Prüfen" (Phase 5, slice 40): the AI Gutachter-Check. One pseudonymized request
//! with the whole Bericht and the PTV 3 hints per section; the model answers with hints per
//! section ("Ein Gutachter würde fragen: …"). It never changes the text: the therapist goes to
//! the section and edits herself, or ignores the hint.
//!
//! The diagnoses stay out (the therapist sets them; no AI comment on them), so does any section
//! marked "nicht relevant". Empty sections, identifiers, the Chiffre and the length are checked
//! by the app itself (`pii::checklist`), not by the model.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::domain::{AgeGroup, Patient};
use crate::draft::{kind_name, method_name, patient_as_pat, Frame};
use crate::extract::{citation_number, json_list, sha256, Preview, TokenView, ICD};
use crate::llm::{ChatOptions, Effort};
use crate::pseudo::{Dictionary, Pseudonymized, Pseudonymizer, TokenMap};
use crate::templates::{FieldKind, SectionDef, Template};
use crate::{Error, Result};

/// Bump on any change to the prompt or the answer format.
pub const PROMPT_VERSION: &str = "review-2026.1";

/// Longest Bericht text sent (a PTV 3 Bericht is about two pages, ~7 000 characters).
pub const MAX_TEXT_CHARS: usize = 30_000;

/// Hints kept, the important ones first.
pub const MAX_HINTS: usize = 12;

/// Longest hint kept.
const MAX_HINT_CHARS: usize = 600;

/// One answer for the whole Bericht, medium effort like a draft.
pub fn chat_options() -> ChatOptions {
    ChatOptions { temperature: 0.2, max_tokens: 4096, timeout: Duration::from_secs(600), reasoning: Some(Effort::Medium) }
}

/// How sure the model is that a Gutachter would object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HintLevel {
    /// A Gutachter would likely object.
    Important,
    /// Worth a look.
    Note,
}

impl HintLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            HintLevel::Important => "important",
            HintLevel::Note => "note",
        }
    }
}

impl std::str::FromStr for HintLevel {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "important" => Ok(HintLevel::Important),
            "note" => Ok(HintLevel::Note),
            other => Err(Error::Invalid(format!("unknown hint level {other}"))),
        }
    }
}

/// A checked hint: its section (`None`: the whole Bericht), level and text (names restored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub section_key: Option<String>,
    pub level: HintLevel,
    pub text: String,
}

/// The sections the check reads: prose with text, not marked "nicht relevant" (`skip`), in order.
/// `text_of` gives a section's plain text.
pub fn sections<'a>(template: &'a Template, skip: impl Fn(&str) -> bool, text_of: impl Fn(&str) -> String) -> Vec<(&'a SectionDef, String)> {
    template
        .sections
        .iter()
        .filter(|d| d.field == FieldKind::Prose && !skip(&d.key))
        .map(|d| (d, text_of(&d.key)))
        .filter(|(_, t)| !t.trim().is_empty())
        .collect()
}

/// The text a check read (keys and text): a different digest later means she changed the Bericht.
pub fn text_digest(sections: &[(&SectionDef, String)]) -> String {
    let mut h = Sha256::new();
    for (d, t) in sections {
        h.update(&d.key);
        h.update([0]);
        h.update(t.trim());
        h.update([1]);
    }
    hex::encode(h.finalize())
}

pub struct Prepared {
    pub system: Pseudonymized<String>,
    pub user: Pseudonymized<String>,
    /// Section key per section number - 1 (`[A1]` …).
    pub keys: Vec<String>,
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

/// The system prompt for a (resolved) Bericht template. Holds no patient data.
pub fn system_prompt(template: &Template, frame: &Frame) -> String {
    let age = if frame.age_group == AgeGroup::Kjp { "Kinder und Jugendliche" } else { "Erwachsene" };
    format!(
        "Du bist erfahrene Gutachterin bzw. erfahrener Gutachter im Gutachterverfahren der gesetzlichen \
         Krankenversicherung (Psychotherapie-Richtlinie). Eine Psychotherapeutin bittet dich, den Entwurf ihres \
         Berichts ({title}; {kind}, {method}, {age}) gegenzulesen, bevor sie ihn abschickt.\n\
         Leitfaden: {intro}\n\n\
         Du erhältst die Abschnitte nummeriert ([A1], [A2] …), jeweils mit Überschrift, den Hinweisen des \
         Leitfadens und dem Text.\n\n\
         Aufgabe: Nenne, was ein Gutachter bemängeln oder nachfragen würde: Pflichtangaben des Leitfadens, die \
         fehlen; Behauptungen ohne Begründung (etwa eine Prognose ohne Grund); Widersprüche zwischen Abschnitten; \
         ein fehlender Bezug zwischen Symptomatik, Modell, Zielen und Behandlungsplan; bei Umwandlung und \
         Fortführung eine fehlende Begründung, warum weitere Behandlung nötig ist.\n\
         Regeln:\n\
         - Nur Hinweise. Keine Umformulierungen und kein Text für den Bericht. Keine Inhalte vorschlagen, die \
         nicht im Bericht stehen; stattdessen danach fragen.\n\
         - Höchstens {max} Hinweise, die wichtigsten zuerst. Jeder Hinweis ein oder zwei Sätze, gern in der Form \
         \"Ein Gutachter würde fragen: …\".\n\
         - Nicht kommentieren: Abschnitte, die fehlen oder leer sind, Länge, Rechtschreibung, Formales. Das prüft \
         die App selbst.\n\
         - Keine Diagnosen, keine ICD-Codes, keine Vorschläge zu Medikamenten. Die Diagnosen setzt die \
         Therapeutin; sie sind nicht Teil der Prüfung.\n\
         - Die Person heißt \"Pat.\". Platzhalter in eckigen Klammern wie [PERSON_n] unverändert lassen.\n\
         - \"level\": \"wichtig\" für das, was ein Gutachter wahrscheinlich beanstandet, sonst \"hinweis\".\n\
         - \"section\": die Nummer des Abschnitts ([A3] → 3), null für den ganzen Bericht.\n\
         - Ist nichts zu bemängeln, ist eine leere Liste die richtige Antwort.\n\n\
         Antworte ausschließlich mit JSON in dieser Form, ohne weiteren Text:\n\
         {{\"hints\": [{{\"section\": 3, \"level\": \"wichtig\", \"text\": \"…\"}}]}}\n",
        title = template.title,
        kind = kind_name(frame.kind),
        method = method_name(frame.method),
        intro = template.intro.trim(),
        max = MAX_HINTS,
    )
}

/// Everything that leaves the machine for checking the Bericht's `sections` (see [`sections`]).
pub fn prepare(template: &Template, frame: &Frame, sections: &[(&SectionDef, String)], patient: &Patient, dict: Dictionary) -> Result<Prepared> {
    if sections.is_empty() {
        return Err(Error::Invalid("Der Bericht hat noch keinen Text zum Prüfen".into()));
    }
    let chars: usize = sections.iter().map(|(_, t)| t.chars().count()).sum();
    if chars > MAX_TEXT_CHARS {
        return Err(Error::Invalid(format!("Der Bericht ist zu lang für die Prüfung ({chars} Zeichen, höchstens {MAX_TEXT_CHARS})")));
    }
    let mut user = String::from("Bericht:\n");
    let mut keys = Vec::new();
    for (i, (def, text)) in sections.iter().enumerate() {
        let number = def.number.as_deref().map(|n| format!("{n}. ")).unwrap_or_default();
        user.push_str(&format!("\n[A{}] {number}{}\n", i + 1, def.heading.split_whitespace().collect::<Vec<_>>().join(" ")));
        if !def.hints.is_empty() {
            user.push_str("Hinweise des Leitfadens:\n");
            for h in &def.hints {
                user.push_str(&format!("- {}\n", h.text.trim()));
            }
        }
        user.push_str(&format!("Text:\n{}\n", patient_as_pat(text.trim(), patient)));
        keys.push(def.key.clone());
    }
    user.push_str("\nNenne, was ein Gutachter bemängeln oder nachfragen würde. Keinen Text für den Bericht schreiben.\n");

    let mut p = Pseudonymizer::new(dict);
    let system_raw = system_prompt(template, frame);
    let prompt_hash = sha256(&format!("{PROMPT_VERSION}\n{system_raw}"));
    let system = p.pseudonymize(&system_raw)?;
    let user = p.pseudonymize(&user)?;
    let mut h = Sha256::new();
    h.update(system.get());
    h.update([0]);
    h.update(user.get());
    Ok(Prepared { system, user, keys, map: p.into_map(), prompt_hash, digest: hex::encode(h.finalize()) })
}

#[derive(Deserialize)]
struct RawHint {
    #[serde(default)]
    section: Value,
    #[serde(default)]
    level: String,
    #[serde(default)]
    text: String,
}

/// Checks the model's answer: hints with a known section (or none), without ICD codes or unknown
/// placeholders, deduplicated, the important ones first, at most [`MAX_HINTS`]. The rest is
/// dropped (counted). Fails only without any JSON.
pub fn parse(answer: &str, prepared: &Prepared) -> Result<(Vec<Hint>, usize)> {
    let (mut out, mut dropped) = (Vec::<(usize, Hint)>::new(), 0);
    for item in json_list(answer, "hints")? {
        let Ok(raw) = serde_json::from_value::<RawHint>(item) else {
            dropped += 1;
            continue;
        };
        let section = match &raw.section {
            Value::Null => None,
            Value::String(s) if s.trim().is_empty() => None,
            v => match citation_number(v, 'A').filter(|&n| n >= 1 && n <= prepared.keys.len()) {
                Some(n) => Some(n - 1),
                None => {
                    dropped += 1;
                    continue;
                }
            },
        };
        let text = raw.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() || text.chars().count() > MAX_HINT_CHARS || ICD.is_match(&text) {
            dropped += 1;
            continue;
        }
        let restored = prepared.map.restore(&text);
        if !restored.unknown.is_empty() || out.iter().any(|(_, h)| h.text == restored.text) {
            dropped += 1;
            continue;
        }
        let level = if raw.level.trim().to_lowercase().starts_with("wichtig") { HintLevel::Important } else { HintLevel::Note };
        // The whole Bericht sorts after its sections.
        let order = section.unwrap_or(prepared.keys.len());
        out.push((order, Hint { section_key: section.map(|i| prepared.keys[i].clone()), level, text: restored.text }));
    }
    // Stable: within a level, section order; same section keeps the model's order.
    out.sort_by_key(|(order, h)| (h.level, *order));
    dropped += out.len().saturating_sub(MAX_HINTS);
    out.truncate(MAX_HINTS);
    Ok((out.into_iter().map(|(_, h)| h).collect(), dropped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ApplicationKind, Gender, Method};
    use crate::pseudo::EntityKind;
    use crate::templates::{self, Context};

    fn bericht() -> Template {
        let t = templates::builtin().unwrap().into_iter().find(|t| t.key == "ptv3-erst-umwandlung").unwrap();
        t.resolve(&Context { method: Method::Vt, age_group: AgeGroup::Adult, kind: Some(ApplicationKind::Umwandlung) })
    }

    fn frame() -> Frame {
        Frame { method: Method::Vt, age_group: AgeGroup::Adult, kind: ApplicationKind::Umwandlung }
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

    fn texts(key: &str) -> String {
        match key {
            "soziodemographie" => "Anna Beispiel, 41 Jahre, Erzieherin bei der Kita Sonnenschein.".into(),
            "symptomatik_befund" => "Panikattacken im Bus seit 2024.".into(),
            "somatik" => "Keine Medikation.".into(),
            "behandlungsplan_prognose" => "Exposition. Die Prognose ist günstig.".into(),
            "diagnose" => "{\"diagnoses\": [{\"code\": \"F41.0\"}]}".into(),
            _ => String::new(),
        }
    }

    #[test]
    fn sends_prose_with_hints_and_leaves_out_diagnoses_and_skipped_sections() {
        let t = bericht();
        let secs = sections(&t, |k| k == "somatik", texts);
        assert_eq!(secs.iter().map(|(d, _)| d.key.as_str()).collect::<Vec<_>>(), ["soziodemographie", "symptomatik_befund", "behandlungsplan_prognose"]);
        let mut dict = Dictionary::from_patient(&patient());
        dict.add(EntityKind::Arbeitgeber, "Kita Sonnenschein");
        let p = prepare(&t, &frame(), &secs, &patient(), dict).unwrap();
        let m = p.user.get();
        assert!(m.starts_with("Bericht:\n\n[A1] 1. Relevante soziodemographische Daten"), "{m}");
        assert!(m.contains("Text:\nPat., 41 Jahre, Erzieherin bei der [ARBEITGEBER_1].\n"), "{m}");
        assert!(m.contains("[A3] 6. ") && m.contains("Hinweise des Leitfadens:\n- "));
        assert!(!m.contains("Anna") && !m.contains("Sonnenschein") && !m.contains("F41") && !m.contains("Medikation"));
        assert!(p.system.get().contains("Umwandlungsantrag") && p.system.get().contains("Verhaltenstherapie"));
        assert_eq!(p.preview().messages.len(), 1);
        assert!(prepare(&t, &frame(), &[], &patient(), Dictionary::new()).is_err());

        let d1 = text_digest(&secs);
        assert_eq!(d1, text_digest(&sections(&t, |k| k == "somatik", texts)));
        assert_ne!(d1, text_digest(&sections(&t, |_| false, texts)));
    }

    #[test]
    fn parse_checks_sections_and_ranks_hints() {
        let t = bericht();
        let secs = sections(&t, |_| false, texts);
        let mut dict = Dictionary::from_patient(&patient());
        dict.add(EntityKind::Arbeitgeber, "Kita Sonnenschein");
        let p = prepare(&t, &frame(), &secs, &patient(), dict).unwrap();
        let answer = r#"<think>…</think>{"hints": [
          {"section": 1, "level": "hinweis", "text": "Beruf bei [ARBEITGEBER_1]  genauer?"},
          {"section": "A4", "level": "wichtig", "text": "Ein Gutachter würde fragen: Woran machen Sie die günstige Prognose fest?"},
          {"section": null, "level": "Wichtig", "text": "Der Bezug zwischen Modell und Plan fehlt."},
          {"section": 2, "level": "wichtig", "text": "Spricht für F41.0."},
          {"section": 9, "level": "wichtig", "text": "Unbekannter Abschnitt."},
          {"section": 2, "level": "hinweis", "text": "Erfunden [PERSON_7]."},
          {"section": 2, "level": "hinweis", "text": ""},
          {"section": 1, "level": "hinweis", "text": "Beruf bei [ARBEITGEBER_1] genauer?"},
          7
        ]}"#;
        let (hints, dropped) = parse(answer, &p).unwrap();
        assert_eq!(hints, vec![
            Hint { section_key: Some("behandlungsplan_prognose".into()), level: HintLevel::Important, text: "Ein Gutachter würde fragen: Woran machen Sie die günstige Prognose fest?".into() },
            Hint { section_key: None, level: HintLevel::Important, text: "Der Bezug zwischen Modell und Plan fehlt.".into() },
            Hint { section_key: Some("soziodemographie".into()), level: HintLevel::Note, text: "Beruf bei Kita Sonnenschein genauer?".into() },
        ]);
        assert_eq!(dropped, 6, "ICD code, unknown section, unknown placeholder, empty, duplicate, not an object");
        assert!(parse("[]", &p).unwrap().0.is_empty());
        assert!(parse("Alles gut.", &p).is_err());

        let many: Vec<String> = (0..20).map(|i| format!(r#"{{"section": 1, "level": "hinweis", "text": "Hinweis {i}"}}"#)).collect();
        let (hints, dropped) = parse(&format!("[{}]", many.join(",")), &p).unwrap();
        assert_eq!((hints.len(), dropped), (MAX_HINTS, 20 - MAX_HINTS));
    }
}
