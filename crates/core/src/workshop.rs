//! Diktat-Werkstatt (Phase 3, slice 24): the therapist thinks out loud, the dictated text appears
//! live, and "Entwurf erstellen" turns it into a proposal per section of the Sitzungsnotiz or the
//! Bericht. Proposals are stored like section drafts (`section_draft`, source `dictation`) and
//! applied with the same replace/append review; they cite no facts, their source is the dictation.
//!
//! One request covers all sections. The text is pseudonymized like every other cloud request; the
//! patient is written as "Pat." first. For a Bericht (it goes to the Gutachter under a Chiffre) a
//! sentence still naming someone else is dropped; in a Sitzungsnotiz (stays in the practice) the
//! names are restored. Diagnoses stay out (MDR): the diagnoses section is never a target, and a
//! sentence with an ICD code is dropped.

use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::Patient;
use crate::draft::{self, DraftSentence, Frame, Parsed, SectionResult};
use crate::extract::{json_list, sha256, Preview, TokenView, ICD};
use crate::llm::{ChatOptions, Effort};
use crate::pseudo::{Dictionary, Pseudonymized, Pseudonymizer, TokenMap};
use crate::templates::{SectionDef, Template};
use crate::{Error, Result};

/// Bump on any change to the prompt or the answer format.
pub const PROMPT_VERSION: &str = "workshop-2026.1";

/// Longest dictation sent in one request (fits a 16k-context local model with the prompt).
pub const MAX_TEXT_CHARS: usize = 20_000;

pub fn chat_options() -> ChatOptions {
    ChatOptions { temperature: 0.2, max_tokens: 8192, timeout: Duration::from_secs(900), reasoning: Some(Effort::Medium) }
}

/// What the proposals are for.
pub enum Audience {
    /// The Sitzungsnotiz of one session.
    Note,
    /// A Bericht an den Gutachter.
    Bericht(Frame),
}

pub struct Request<'a> {
    /// The dictation, plain text.
    pub text: &'a str,
    /// Current plain text per section, sent along so the proposal only adds to it.
    pub existing: &'a [(String, String)],
    pub instruction: Option<&'a str>,
}

pub struct Prepared {
    pub system: Pseudonymized<String>,
    pub user: Pseudonymized<String>,
    /// Sections the model may fill, in template order.
    pub keys: Vec<String>,
    /// Sections whose current text went along.
    pub with_existing: Vec<String>,
    pub map: TokenMap,
    /// SHA-256 of version + system prompt (no patient data), stored with the drafts.
    pub prompt_hash: String,
    /// SHA-256 over the outgoing messages: what the preview showed is what is sent.
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

pub fn system_prompt(template: &Template, audience: &Audience) -> String {
    let what = match audience {
        Audience::Note => "einer Sitzungsnotiz (Dokumentation einer Psychotherapiesitzung, bleibt in der Praxis)".to_string(),
        Audience::Bericht(f) => format!(
            "eines Berichts an die Gutachterin oder den Gutachter ({}; {}, {})",
            template.title,
            draft::kind_name(f.kind),
            draft::method_name(f.method)
        ),
    };
    let others = match audience {
        Audience::Note => "",
        Audience::Bericht(_) => {
            "- Andere Personen nur über ihre Rolle (die Schwester, der Partner, die Vorgesetzte), keine Orte und keine Arbeitgeber nennen.\n"
        }
    };
    format!(
        "Du unterstützt eine Psychotherapeutin. Sie hat frei diktiert, was ihr zu {what} einfällt. \
         Das Diktat ist automatisch erkannte Sprache: Füllwörter, Wiederholungen und Erkennungsfehler sind möglich.\n\n\
         Aufgabe: Ordne den Inhalt des Diktats den Abschnitten zu und formuliere ihn als sachlichen, knappen \
         Fachtext in ganzen Sätzen, auf Deutsch.\n\
         Regeln:\n\
         - Nur was im Diktat steht. Nichts ergänzen, nichts erfinden, keine eigenen Bewertungen, Prognosen oder Ziele.\n\
         - Korrekturen im Diktat befolgen (\"nein, streich das\", \"ich meine …\"); Füllwörter und Selbstgespräche weglassen.\n\
         - Jeden Inhalt nur einem Abschnitt zuordnen, dem passendsten. Abschnitte ohne passenden Inhalt weglassen.\n\
         - Steht schon Text in einem Abschnitt, nichts davon wiederholen; nur ergänzen, was dort noch fehlt.\n\
         - Die Patientin oder der Patient heißt durchgehend \"Pat.\". Keine Anrede.\n\
         {others}\
         - Platzhalter in eckigen Klammern wie [PERSON_n] unverändert übernehmen, nicht auflösen.\n\
         - Keine Diagnosen, keine ICD-Codes, keine Vorschläge zu Medikamenten oder Dosierungen, auch wenn das Diktat sie nennt.\n\
         - Eine Anweisung der Therapeutin befolgen, soweit sie diesen Regeln nicht widerspricht.\n\n\
         Antworte ausschließlich mit JSON in dieser Form, ohne weiteren Text:\n\
         {{\"sections\": [{{\"key\": \"<Schlüssel>\", \"sentences\": [\"…\", \"…\"]}}]}}\n"
    )
}

fn user_message(defs: &[&SectionDef], existing: &[(String, String)], instruction: Option<&str>, text: &str) -> String {
    let mut m = String::from("Abschnitte (Schlüssel: Überschrift):\n");
    for d in defs {
        m.push_str(&format!("- {}: {}\n", d.key, d.heading.trim()));
        for h in &d.hints {
            m.push_str(&format!("  · {}\n", h.text.trim()));
        }
    }
    for (key, body) in existing {
        if let Some(d) = defs.iter().find(|d| &d.key == key) {
            m.push_str(&format!("\nBereits im Abschnitt „{}“ (nicht wiederholen, nur ergänzen):\n{body}\n", d.heading.trim()));
        }
    }
    if let Some(i) = instruction {
        m.push_str(&format!("\nAnweisung der Therapeutin: {i}\n"));
    }
    m.push_str("\nDiktat der Therapeutin:\n\"\"\"\n");
    m.push_str(text.trim());
    m.push_str("\n\"\"\"\n");
    m
}

/// Everything that leaves the machine for one Werkstatt draft.
pub fn prepare(template: &Template, audience: &Audience, req: &Request, patient: &Patient, dict: Dictionary) -> Result<Prepared> {
    let text = req.text.trim();
    if text.is_empty() {
        return Err(Error::Invalid("Das Diktat ist leer".into()));
    }
    let chars = text.chars().count();
    if chars > MAX_TEXT_CHARS {
        return Err(Error::Invalid(format!("Das Diktat ist zu lang für einen Entwurf ({chars} Zeichen, höchstens {MAX_TEXT_CHARS})")));
    }
    let defs = draft::target_sections(template);
    if defs.is_empty() {
        return Err(Error::Invalid("Diese Vorlage hat keine Abschnitte für KI-Text".into()));
    }
    let mut existing = Vec::new();
    for (key, body) in req.existing {
        let body = body.trim();
        if body.is_empty() || !defs.iter().any(|d| &d.key == key) {
            continue;
        }
        if body.chars().count() > draft::MAX_EXISTING_CHARS {
            return Err(Error::Invalid(format!(
                "Ein Abschnitt ist zu lang, um ihn mitzuschicken (höchstens {} Zeichen). Ohne „Vorhandenen Text berücksichtigen“ entwerfen.",
                draft::MAX_EXISTING_CHARS
            )));
        }
        existing.push((key.clone(), draft::patient_as_pat(body, patient)));
    }
    let instruction = draft::clean_instruction(req.instruction)?.map(|i| draft::patient_as_pat(&i, patient));

    let mut p = Pseudonymizer::new(dict);
    let system_raw = system_prompt(template, audience);
    let prompt_hash = sha256(&format!("{PROMPT_VERSION}\n{system_raw}"));
    let system = p.pseudonymize(&system_raw)?;
    let message = user_message(&defs, &existing, instruction.as_deref(), &draft::patient_as_pat(text, patient));
    let user = p.pseudonymize(&message)?;
    let mut h = Sha256::new();
    h.update(system.get());
    h.update([0]);
    h.update(user.get());
    Ok(Prepared {
        system,
        user,
        keys: defs.iter().map(|d| d.key.clone()).collect(),
        with_existing: existing.into_iter().map(|(k, _)| k).collect(),
        map: p.into_map(),
        prompt_hash,
        digest: hex::encode(h.finalize()),
    })
}

/// Checks the answer: one result per section that got sentences, in template order. Fails only
/// without any JSON.
pub fn parse(answer: &str, prepared: &Prepared, audience: &Audience) -> Result<Vec<SectionResult>> {
    let mut by_key: Vec<(String, Parsed)> = prepared.keys.iter().map(|k| (k.clone(), Parsed::default())).collect();
    let mut stray = 0;
    for item in json_list(answer, "sections")? {
        let key = item.get("key").and_then(Value::as_str).map(str::trim).unwrap_or_default();
        let sentences = item.get("sentences").and_then(Value::as_array).cloned().unwrap_or_default();
        let Some((_, parsed)) = by_key.iter_mut().find(|(k, _)| k == key) else {
            stray += sentences.len().max(1);
            continue;
        };
        for s in sentences {
            let text = match &s {
                Value::String(t) => Some(t.as_str()),
                Value::Object(o) => o.get("text").and_then(Value::as_str),
                _ => None,
            };
            match text.and_then(|t| check(t, &prepared.map, audience)) {
                Some(t) if !parsed.sentences.iter().any(|x| x.text == t) => parsed.sentences.push(DraftSentence { text: t, facts: vec![] }),
                Some(_) => {}
                None => parsed.dropped += 1,
            }
        }
    }
    // Entries for unknown sections count as dropped in the first section's result.
    if let Some((_, first)) = by_key.iter_mut().find(|(_, p)| !p.sentences.is_empty()) {
        first.dropped += stray;
    }
    Ok(by_key
        .into_iter()
        .filter(|(_, p)| !p.sentences.is_empty())
        .map(|(key, parsed)| {
            let with_existing = prepared.with_existing.contains(&key);
            SectionResult { key, parsed, with_existing }
        })
        .collect())
}

fn check(raw: &str, map: &TokenMap, audience: &Audience) -> Option<String> {
    let text = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() || text.chars().count() > draft::MAX_SENTENCE_CHARS || ICD.is_match(&text) {
        return None;
    }
    // A Bericht gets dates back only; a name, place or employer means the sentence names someone.
    if matches!(audience, Audience::Bericht(_)) && draft::TOKEN.captures_iter(&text).any(|c| &c[1] != "DATUM") {
        return None;
    }
    let restored = map.restore(&text);
    if !restored.unknown.is_empty() {
        return None;
    }
    Some(restored.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgeGroup, ApplicationKind, Method};
    use crate::templates::{self, Context};

    fn template(key: &str) -> Template {
        let t = templates::builtin().unwrap().into_iter().find(|t| t.key == key).unwrap();
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
            gender: None,
            consent_recording: false,
            status: "active".into(),
            created_at: String::new(),
            archived_at: None,
        }
    }

    fn dict() -> Dictionary {
        let mut d = Dictionary::from_patient(&patient());
        d.add(crate::pseudo::EntityKind::Person, "Jonas");
        d
    }

    fn bericht() -> Audience {
        Audience::Bericht(Frame { method: Method::Vt, age_group: AgeGroup::Adult, kind: ApplicationKind::Erstantrag })
    }

    #[test]
    fn prepare_pseudonymizes_and_lists_only_ai_prose_sections() {
        let t = template("sitzungsnotiz");
        let req = Request {
            text: "Frau Beispiel berichtet, dass Jonas am 12.03.2026 ausgezogen ist. Sie schläft schlecht.",
            existing: &[],
            instruction: Some("kurz halten"),
        };
        let p = prepare(&t, &Audience::Note, &req, &patient(), dict()).unwrap();
        let msg = p.user.get();
        assert!(!msg.contains("Beispiel") && !msg.contains("Jonas") && !msg.contains("12.03.2026"), "{msg}");
        assert!(msg.contains("Pat. berichtet") && msg.contains("[PERSON_"), "{msg}");
        assert!(msg.contains("Anweisung der Therapeutin: kurz halten"));
        let targets: Vec<String> = draft::target_sections(&t).iter().map(|d| d.key.clone()).collect();
        assert_eq!(p.keys, targets);
        assert_eq!(p.preview().messages.len(), 1);
        assert_eq!(p.digest.len(), 64);
        let b = template("ptv3-erst-umwandlung");
        let p = prepare(&b, &bericht(), &req, &patient(), dict()).unwrap();
        assert!(!p.keys.iter().any(|k| k.contains("diagnos")), "diagnoses are never a target: {:?}", p.keys);
        assert!(p.system.get().contains("Gutachter"));
    }

    #[test]
    fn empty_and_overlong_dictations_are_refused() {
        let t = template("sitzungsnotiz");
        let empty = Request { text: "  ", existing: &[], instruction: None };
        assert!(prepare(&t, &Audience::Note, &empty, &patient(), dict()).is_err());
        let long = "a".repeat(MAX_TEXT_CHARS + 1);
        let req = Request { text: &long, existing: &[], instruction: None };
        assert!(prepare(&t, &Audience::Note, &req, &patient(), dict()).is_err());
    }

    #[test]
    fn existing_text_goes_along_for_its_section() {
        let t = template("sitzungsnotiz");
        let key = draft::target_sections(&t)[0].key.clone();
        let existing = vec![(key.clone(), "Anna Beispiel kam pünktlich.".to_string()), ("nope".into(), "x".into())];
        let req = Request { text: "Sie wirkte müde.", existing: &existing, instruction: None };
        let p = prepare(&t, &Audience::Note, &req, &patient(), dict()).unwrap();
        assert_eq!(p.with_existing, vec![key]);
        assert!(p.user.get().contains("Pat. kam pünktlich."), "{}", p.user.get());
    }

    #[test]
    fn parse_restores_names_in_a_note_but_drops_them_in_a_bericht() {
        let t = template("sitzungsnotiz");
        let req = Request { text: "Jonas ist am 12.03.2026 ausgezogen.", existing: &[], instruction: None };
        let p = prepare(&t, &Audience::Note, &req, &patient(), dict()).unwrap();
        let person = p.map.entries().iter().find(|e| e.token.starts_with("[PERSON")).unwrap().token.clone();
        let date = p.map.entries().iter().find(|e| e.token.starts_with("[DATUM")).unwrap().token.clone();
        let (a, b) = (p.keys[0].clone(), p.keys[1].clone());
        let answer = format!(
            r#"Hier: {{"sections": [
                {{"key": "{a}", "sentences": ["{person} ist am {date} ausgezogen.", "Pat. schläft schlecht.", "Pat. schläft schlecht.", "Verdacht auf F32.1."]}},
                {{"key": "{b}", "sentences": [{{"text": "Erfundener [PERSON_99] kommt vor."}}]}},
                {{"key": "unbekannt", "sentences": ["x"]}}
            ]}}"#
        );
        let note = parse(&answer, &p, &Audience::Note).unwrap();
        assert_eq!(note.len(), 1, "a section with nothing left is left out");
        let texts: Vec<&str> = note[0].parsed.sentences.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["Jonas ist am 12.03.2026 ausgezogen.", "Pat. schläft schlecht."]);
        assert!(note[0].parsed.sentences.iter().all(|s| s.facts.is_empty()));
        assert_eq!(note[0].parsed.dropped, 2, "ICD sentence + the stray section");

        let bericht = parse(&answer, &p, &bericht()).unwrap();
        let texts: Vec<&str> = bericht[0].parsed.sentences.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["Pat. schläft schlecht."], "a named person stays out of the Bericht");
        assert!(parse("kein json", &p, &Audience::Note).is_err());
    }
}
