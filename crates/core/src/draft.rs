//! Bericht drafting per section (Phase 2, slice 14): the facts the therapist applied to the
//! Sitzungsnotizen of an episode become a draft for one Bericht section, sentence by sentence,
//! each sentence citing the facts it rests on (fact → transcript segment via `fact_citation`).
//!
//! The model sees numbered, pseudonymized facts (`[F7] (Befund) …`), one request per section.
//! The patient is written as "Pat." before pseudonymizing, since the Bericht goes to the Gutachter
//! under a Chiffre. A sentence that still names someone else (a placeholder for a person, place or
//! employer) is dropped rather than restored: those names must not reach the Gutachter either.
//!
//! Slice 19: a section tagged in extraction sees only the facts tagged for it (plus untagged ones
//! from before tagging existed); on "ergänzen" the section's current text goes along so the draft
//! doesn't repeat it; and a sentence about prognosis, motivation, goals or setting is kept only if
//! a fact it cites says so too (small models invent these).

use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::{AgeGroup, ApplicationKind, Method, Patient};
use crate::extract::{citation_number, json_list, sha256, Preview, TokenView, ICD};
use crate::llm::{ChatOptions, Effort};
use crate::pseudo::{DetectionSource, Dictionary, EntityKind, Pseudonymized, Pseudonymizer, TokenMap};
use crate::templates::{FieldKind, SectionDef, Template};
use crate::{Error, Result};

/// Bump on any change to the prompt or the answer format.
pub const PROMPT_VERSION: &str = "draft-2026.3";

/// Fact characters per request, like `extract::CHUNK_CHARS` (fits a 16k-context local model).
pub const FACT_CHARS: usize = 24_000;

/// Request options for one section draft (the eval harness uses the same).
pub fn chat_options() -> ChatOptions {
    ChatOptions { temperature: 0.2, max_tokens: 8192, timeout: Duration::from_secs(900), reasoning: Some(Effort::Medium) }
}

/// Longest sentence kept.
pub(crate) const MAX_SENTENCE_CHARS: usize = 500;

/// Longest instruction the therapist may add to a request.
pub const MAX_INSTRUCTION_CHARS: usize = 500;

/// Longest existing section text sent along on "ergänzen".
pub const MAX_EXISTING_CHARS: usize = 8_000;

/// An applied Sitzungsnotiz fact as input to a draft.
#[derive(Debug, Clone)]
pub struct InputFact {
    /// 1-based position of its session among the episode's sessions with facts.
    pub session_no: usize,
    /// Heading of the note section it was filed under ("Befund").
    pub label: String,
    pub text: String,
    /// Bericht sections it was tagged with; `None` for a fact from before tagging (offered to
    /// every section).
    pub bericht: Option<Vec<String>>,
}

/// What to draft and from what.
pub struct Request<'a> {
    pub keys: &'a [String],
    pub facts: &'a [InputFact],
    /// Sections that draw only on facts tagged for them (the tag template's sections); any other
    /// section gets all facts.
    pub tag_keys: &'a [String],
    /// Current plain text per section, sent along so the draft only adds to it ("ergänzen").
    pub existing: &'a [(String, String)],
    pub instruction: Option<&'a str>,
}

/// One request: the draft for section `key`.
pub struct PreparedSection {
    pub key: String,
    pub user: Pseudonymized<String>,
    /// The facts (indices into the request's facts) this request shows; only these may be cited.
    pub facts: Vec<usize>,
    /// The section's current text went along.
    pub with_existing: bool,
}

pub struct Prepared {
    pub system: Pseudonymized<String>,
    pub sections: Vec<PreparedSection>,
    /// Sections with no fact tagged for them: nothing is sent, their draft is empty.
    pub skipped: Vec<String>,
    /// Facts without tags, offered to every tagged section.
    pub untagged: usize,
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

/// One section's checked answer, ready to store.
#[derive(Debug)]
pub struct SectionResult {
    pub key: String,
    pub parsed: Parsed,
    /// The section's existing text went along ("ergänzen").
    pub with_existing: bool,
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

pub(crate) fn method_name(m: Method) -> &'static str {
    match m {
        Method::Vt => "Verhaltenstherapie",
        Method::Tp => "tiefenpsychologisch fundierte Psychotherapie",
        Method::Ap => "analytische Psychotherapie",
    }
}

pub(crate) fn kind_name(k: ApplicationKind) -> &'static str {
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
         - Prognose, Motivation, Therapieziele, Setting (Einzel/Gruppe) und Sitzungszahl nur, wenn ein \
         zitierter Fakt sie ausdrücklich nennt; sonst weglassen, auch im Abschnitt zu Behandlungsplan und Prognose.\n\
         - Steht schon Text im Abschnitt, nichts davon wiederholen; nur ergänzen, was dort noch fehlt. \
         Ist nichts Neues dabei, leere Liste.\n\
         - Ein Eintrag pro Satz. Jeder Satz nennt im Feld \"facts\" die Nummern der Fakten, auf denen er \
         beruht (mindestens eine). Keine Nummern wie [F1] in den Text schreiben.\n\
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

fn section_message(def: &SectionDef, instruction: Option<&str>, existing: Option<&str>, facts: &str) -> String {
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
    if let Some(e) = existing {
        m.push_str(&format!("\nBereits im Abschnitt (nicht wiederholen, nur ergänzen):\n{e}\n"));
    }
    m.push_str("\nFakten aus den Sitzungsnotizen:\n");
    m.push_str(facts);
    // Small models follow what comes last: repeat the scope after the long fact list.
    m.push_str(&format!(
        "\nSchreibe nur Abschnitt \"{}\". Fakten, die nicht dazu gehören, weglassen; passt keiner, leere Liste.\n",
        def.heading.trim()
    ));
    if existing.is_some() {
        m.push_str("Nur ergänzen, was noch nicht im vorhandenen Text steht.\n");
    }
    m
}

/// The facts `subset` as numbered lines under their session (numbers count in all facts).
fn fact_list(facts: &[InputFact], subset: &[usize], patient: &Patient) -> String {
    let mut list = String::new();
    let mut session = 0;
    for &i in subset {
        let f = &facts[i];
        if f.session_no != session {
            session = f.session_no;
            list.push_str(&format!("Sitzung {session}:\n"));
        }
        list.push_str(&format!("[F{}] ({}) {}\n", i + 1, f.label, patient_as_pat(f.text.trim(), patient)));
    }
    list
}

static BREAK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>|</(p|li|h[1-6]|div|blockquote)>").expect("valid regex"));
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]*>").expect("valid regex"));

/// Plain text of a section's TipTap HTML, one line per paragraph or list item (unlike
/// `pii::plain_text`, which flattens it), so the model sees the structure it adds to.
pub fn plain_text(html: &str) -> String {
    let text = BREAK.replace_all(html, "\n");
    let text = TAG.replace_all(&text, "");
    let text = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
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

/// Everything that leaves the machine for drafting `req.keys`, one pseudonymizer for all requests.
pub fn prepare(template: &Template, frame: &Frame, req: &Request, patient: &Patient, dict: Dictionary) -> Result<Prepared> {
    let (keys, facts) = (req.keys, req.facts);
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
    let instruction = clean_instruction(req.instruction)?.map(|i| patient_as_pat(&i, patient));

    let all: Vec<usize> = (0..facts.len()).collect();
    let chars = fact_list(facts, &all, patient).chars().count();
    if chars > FACT_CHARS {
        return Err(Error::Invalid(format!("Zu viele Fakten für einen Entwurf ({chars} Zeichen, höchstens {FACT_CHARS})")));
    }

    let mut p = Pseudonymizer::new(dict);
    let system_raw = system_prompt(template, frame);
    let prompt_hash = sha256(&format!("{PROMPT_VERSION}\n{system_raw}"));
    let system = p.pseudonymize(&system_raw)?;
    let (mut sections, mut skipped) = (Vec::new(), Vec::new());
    for def in defs {
        let subset: Vec<usize> = if req.tag_keys.contains(&def.key) {
            all.iter().copied().filter(|&i| facts[i].bericht.as_ref().is_none_or(|b| b.contains(&def.key))).collect()
        } else {
            all.clone()
        };
        if subset.is_empty() {
            skipped.push(def.key.clone());
            continue;
        }
        let existing = req.existing.iter().find(|(k, _)| k == &def.key).map(|(_, t)| t.trim()).filter(|t| !t.is_empty());
        if existing.is_some_and(|t| t.chars().count() > MAX_EXISTING_CHARS) {
            return Err(Error::Invalid(format!(
                "Der Abschnitt „{}“ ist zu lang, um ihn mitzuschicken (höchstens {MAX_EXISTING_CHARS} Zeichen). Ohne „Vorhandenen Text berücksichtigen“ entwerfen.",
                def.heading.trim()
            )));
        }
        let existing = existing.map(|t| patient_as_pat(t, patient));
        let message = section_message(def, instruction.as_deref(), existing.as_deref(), &fact_list(facts, &subset, patient));
        let user = p.pseudonymize(&message)?;
        sections.push(PreparedSection { key: def.key.clone(), user, facts: subset, with_existing: existing.is_some() });
    }
    let mut h = Sha256::new();
    h.update(system.get());
    for s in &sections {
        h.update([0]);
        h.update(s.user.get());
    }
    for k in &skipped {
        h.update([1]);
        h.update(k);
    }
    let untagged = facts.iter().filter(|f| f.bericht.is_none()).count();
    Ok(Prepared { system, sections, skipped, untagged, map: p.into_map(), prompt_hash, digest: hex::encode(h.finalize()) })
}

pub(crate) static TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(PERSON|ORT|ARBEITGEBER|DATUM)_\d+\]").expect("valid regex"));
// Citation markers some models write into the text as well ("… pro Woche [F1].", "(F2, F3)",
// "… Fokus. [F1], [F2]"): a run of them, with the separators between them.
const MARKER_ONE: &str = r"[\[(]F\s?\d+(?:\s*[,;]\s*F?\s?\d+)*[\])]";
static MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\s*{MARKER_ONE}(?:\s*[,;]?\s*{MARKER_ONE})*")).expect("valid regex"));
// The end of a sentence that cites inline: "… Arbeit [F1]." or "… Arbeit. [F1, F2]".
static CITED_END: LazyLock<Regex> = LazyLock::new(|| {
    let run = format!(r"{MARKER_ONE}(?:\s*[,;]?\s*{MARKER_ONE})*");
    Regex::new(&format!(r"{run}\s*[.!?]|[.!?]\s*{run}")).expect("valid regex")
});
static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").expect("valid regex"));
// Separators left before the end of a sentence once the markers are gone ("Zurückweisung,.").
static DANGLING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*[,;]+(\s*[.!?]|\s*$)").expect("valid regex"));

#[derive(Deserialize)]
struct RawSentence {
    #[serde(default)]
    text: String,
    #[serde(default)]
    facts: Vec<Value>,
}

// Statements small models add on their own (prognosis, motivation, goals, setting): a sentence
// matching one of these is kept only if a fact it cites matches the same pattern.
static GROUNDED: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"prognos",
        r"günstig",
        r"motiv(iert|ation)",
        r"ziel",
        r"erfolg",
        r"zuversicht",
        r"einzel(therapie|setting|sitzung|behandlung)|gruppen(therapie|setting|behandlung)|setting|kombinationsbehandlung",
        r"(veränderungs|änderungs|umstellungs)(bereit|fähig)|introspekt|reflexionsfähig|krankheitseinsicht|leidensdruck",
    ]
    .iter()
    .map(|p| regex::RegexBuilder::new(p).case_insensitive(true).build().expect("valid regex"))
    .collect()
});

/// A sentence states something of the [`GROUNDED`] kind that none of its cited facts states.
fn ungrounded(text: &str, cited: &[usize], facts: &[InputFact]) -> bool {
    GROUNDED.iter().any(|r| r.is_match(text) && !cited.iter().any(|&i| facts.get(i).is_some_and(|f| r.is_match(&f.text))))
}

/// Checks a model answer for `section` (one of the prepared requests over `facts`). Fails only
/// without any JSON.
pub fn parse(answer: &str, section: &PreparedSection, facts: &[InputFact], map: &TokenMap) -> Result<Parsed> {
    let mut out = Parsed::default();
    for item in json_list(answer, "sentences")? {
        let Ok(raw) = serde_json::from_value::<RawSentence>(item) else {
            out.dropped += 1;
            continue;
        };
        for raw in split_inline(raw) {
            match check(raw, &section.facts, map).filter(|s| !ungrounded(&s.text, &s.facts, facts)) {
                Some(s) if !out.sentences.iter().any(|t| t.text == s.text) => out.sentences.push(s),
                Some(_) => {}
                None => out.dropped += 1,
            }
        }
    }
    Ok(out)
}

/// Some models write a whole paragraph into one entry and cite inline ("… [F1]. … [F3, F4].")
/// instead of filling `facts`. Splits it at the cited sentence ends, each part citing its own
/// markers; a trailing part without one stays uncited (and is dropped).
fn split_inline(raw: RawSentence) -> Vec<RawSentence> {
    if !raw.facts.is_empty() || !MARKER.is_match(&raw.text) {
        return vec![raw];
    }
    let ends: Vec<usize> = CITED_END.find_iter(&raw.text).map(|m| m.end()).collect();
    let mut parts = Vec::new();
    let mut start = 0;
    for end in ends.into_iter().chain([raw.text.len()]) {
        let text = raw.text[start..end].trim();
        start = end;
        if text.is_empty() {
            continue;
        }
        let facts = MARKER
            .find_iter(text)
            .flat_map(|m| NUMBER.find_iter(m.as_str()).map(|n| Value::String(n.as_str().to_string())).collect::<Vec<_>>())
            .collect();
        parts.push(RawSentence { text: text.to_string(), facts });
    }
    parts
}

fn check(raw: RawSentence, shown: &[usize], map: &TokenMap) -> Option<DraftSentence> {
    let text = MARKER.replace_all(&raw.text, "");
    let text = DANGLING.replace_all(&text, "$1").split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() || text.chars().count() > MAX_SENTENCE_CHARS || ICD.is_match(&text) {
        return None;
    }
    let mut facts = Vec::new();
    for v in &raw.facts {
        let n = citation_number(v, 'F')?;
        if n == 0 || !shown.contains(&(n - 1)) {
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
        InputFact { session_no, label: label.into(), text: text.into(), bericht: None }
    }

    fn tagged(session_no: usize, label: &str, text: &str, tags: &[&str]) -> InputFact {
        InputFact { bericht: Some(keys(tags)), ..fact(session_no, label, text) }
    }

    fn keys(k: &[&str]) -> Vec<String> {
        k.iter().map(|s| s.to_string()).collect()
    }

    fn tag_keys() -> Vec<String> {
        target_sections(&bericht()).iter().map(|s| s.key.clone()).collect()
    }

    /// Prepares `k` over `facts` (tag filtering on, no existing text).
    fn prep(k: &[&str], facts: &[InputFact], dict: Dictionary, instruction: Option<&str>) -> Result<Prepared> {
        let (k, tags) = (keys(k), tag_keys());
        prepare(&bericht(), &frame(), &Request { keys: &k, facts, tag_keys: &tags, existing: &[], instruction }, &patient(), dict)
    }

    #[test]
    fn prompt_has_frame_and_no_tokens() {
        let p = system_prompt(&bericht(), &frame());
        assert!(p.contains("Erstantrag") && p.contains("Verhaltenstherapie") && p.contains("Erwachsene"));
        assert!(p.contains("etwa 2 Seiten"));
        assert!(Pseudonymizer::new(Dictionary::new()).pseudonymize(&p).is_ok());
        let targets = tag_keys();
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
        let p = prep(&["symptomatik_befund", "soziodemographie", "symptomatik_befund"], &facts, dict.clone(), Some("  kürzer   bitte ")).unwrap();
        assert_eq!(p.sections.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(), ["symptomatik_befund", "soziodemographie"]);
        assert_eq!((p.untagged, p.skipped.len()), (3, 0), "untagged facts go to every section");
        let m = p.sections[0].user.get();
        assert!(m.starts_with("Abschnitt: 2. Symptomatik und psychischer Befund\nHinweise des Leitfadens:\n- "), "{m}");
        assert!(m.contains("Anweisung der Therapeutin: kürzer bitte\n"));
        assert!(m.contains("Sitzung 1:\n[F1] (Verlauf) Pat. berichtet Schlafstörungen seit dem [DATUM_1].\n[F2] (Befund) Stimmung gedrückt\nSitzung 2:\n[F3] (Verlauf) Streit mit Schwester [PERSON_1]"), "{m}");
        assert!(!m.contains("Anna") && !m.contains("Beispiel") && !m.contains("Gerda"));
        assert!(!m.contains("Bereits im Abschnitt"));
        let again = prep(&["symptomatik_befund", "soziodemographie"], &facts, dict.clone(), Some("kürzer bitte")).unwrap();
        assert_eq!(again.digest, p.digest);
        assert_ne!(prep(&["symptomatik_befund", "soziodemographie"], &facts, dict.clone(), None).unwrap().digest, p.digest);

        assert!(prep(&["diagnose"], &facts, dict.clone(), None).is_err(), "not AI-draftable");
        assert!(prep(&[], &facts, dict.clone(), None).is_err());
        assert!(prep(&["somatik"], &[], dict.clone(), None).is_err(), "no facts");
        let long = "x".repeat(MAX_INSTRUCTION_CHARS + 1);
        assert!(prep(&["somatik"], &facts, dict.clone(), Some(&long)).is_err());
        let many: Vec<_> = (0..300).map(|_| fact(1, "Verlauf", &"y".repeat(100))).collect();
        assert!(prep(&["somatik"], &many, dict, None).is_err(), "over budget");
    }

    #[test]
    fn a_tagged_section_sees_only_its_facts() {
        let facts = vec![
            tagged(1, "Verlauf", "Panik im Bus", &["symptomatik_befund"]),
            tagged(1, "Verlauf", "Arbeitet als Erzieherin", &["soziodemographie"]),
            tagged(2, "Befund", "Stimmung gedrückt", &["symptomatik_befund", "soziodemographie"]),
            tagged(2, "Vereinbarungen", "Expositionsübungen vereinbart", &["behandlungsplan_prognose", "symptomatik_befund"]),
            tagged(2, "Verlauf", "Smalltalk über den Urlaub", &[]),
        ];
        let p = prep(&["symptomatik_befund", "somatik", "soziodemographie"], &facts, Dictionary::new(), None).unwrap();
        assert_eq!(p.skipped, ["somatik"], "no fact for Somatik: nothing sent");
        assert_eq!(p.untagged, 0);
        assert_eq!(p.sections.iter().map(|s| (s.key.as_str(), s.facts.clone())).collect::<Vec<_>>(), [
            ("symptomatik_befund", vec![0, 2, 3]),
            ("soziodemographie", vec![1, 2]),
        ]);
        let m = p.sections[0].user.get();
        assert!(m.contains("Sitzung 1:\n[F1] (Verlauf) Panik im Bus\nSitzung 2:\n[F3] (Befund) Stimmung gedrückt\n[F4] (Vereinbarungen) Expositionsübungen"), "numbers stay global: {m}");
        assert!(!m.contains("Erzieherin") && !m.contains("Urlaub"));
        // A section outside the tag vocabulary (Umwandlung, Fortführung) gets everything.
        let k = keys(&["symptomatik_befund"]);
        let all = prepare(&bericht(), &frame(), &Request { keys: &k, facts: &facts, tag_keys: &[], existing: &[], instruction: None }, &patient(), Dictionary::new()).unwrap();
        assert_eq!(all.sections[0].facts, vec![0, 1, 2, 3, 4]);
        // Only facts shown in a request may be cited.
        let out = parse(r#"[{"text": "Panik im Bus.", "facts": [1]}, {"text": "Erzieherin.", "facts": [2]}]"#, &p.sections[0], &facts, &p.map).unwrap();
        assert_eq!(out.sentences, vec![DraftSentence { text: "Panik im Bus.".into(), facts: vec![0] }]);
        assert_eq!(out.dropped, 1);
    }

    #[test]
    fn existing_text_goes_along_on_append() {
        let facts = vec![fact(1, "Verlauf", "Anna schläft schlecht"), fact(1, "Befund", "Stimmung gedrückt")];
        let (k, tags) = (keys(&["symptomatik_befund", "soziodemographie"]), tag_keys());
        let existing = vec![("symptomatik_befund".to_string(), plain_text("<p>Frau Beispiel berichtet <strong>Panik</strong> &amp; Unruhe.</p><ul><li><p>seit Mai</p></li></ul>"))];
        assert_eq!(existing[0].1, "Frau Beispiel berichtet Panik & Unruhe.\nseit Mai");
        let req = Request { keys: &k, facts: &facts, tag_keys: &tags, existing: &existing, instruction: None };
        let p = prepare(&bericht(), &frame(), &req, &patient(), Dictionary::from_patient(&patient())).unwrap();
        let m = p.sections[0].user.get();
        assert!(p.sections[0].with_existing && !p.sections[1].with_existing);
        assert!(m.contains("Bereits im Abschnitt (nicht wiederholen, nur ergänzen):\nPat. berichtet Panik & Unruhe.\nseit Mai\n\nFakten"), "{m}");
        assert!(m.ends_with("Nur ergänzen, was noch nicht im vorhandenen Text steht.\n"));
        assert!(!p.sections[1].user.get().contains("Bereits"));
        let long = vec![("symptomatik_befund".to_string(), "z".repeat(MAX_EXISTING_CHARS + 1))];
        assert!(prepare(&bericht(), &frame(), &Request { existing: &long, ..req }, &patient(), Dictionary::new()).is_err());
    }

    #[test]
    fn prognosis_and_goals_need_a_fact_that_says_so() {
        let facts = vec![
            fact(1, "Verlauf", "Panik im Bus"),
            fact(1, "Vereinbarungen", "Ziel: wieder allein Bus fahren"),
            fact(2, "Verlauf", "Pat. wirkt motiviert, kommt pünktlich"),
        ];
        let p = prep(&["behandlungsplan_prognose"], &facts, Dictionary::new(), None).unwrap();
        let answer = r#"{"sentences": [
          {"text": "Ziel ist, wieder allein Bus zu fahren.", "facts": [2]},
          {"text": "Die Prognose ist günstig.", "facts": [1]},
          {"text": "Pat. ist hoch motiviert.", "facts": [3]},
          {"text": "Pat. ist hoch motiviert, die Prognose günstig.", "facts": [3]},
          {"text": "Therapieziel ist der Abbau der Panik.", "facts": [1]},
          {"text": "Geplant ist eine Einzeltherapie.", "facts": [1, 2]},
          {"text": "Expositionen im Bus sind geplant.", "facts": [1]}
        ]}"#;
        let out = parse(answer, &p.sections[0], &facts, &p.map).unwrap();
        assert_eq!(
            out.sentences.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
            ["Ziel ist, wieder allein Bus zu fahren.", "Pat. ist hoch motiviert.", "Expositionen im Bus sind geplant."]
        );
        assert_eq!(out.dropped, 4);
    }

    #[test]
    fn a_paragraph_cited_inline_is_split_into_sentences() {
        let facts = vec![fact(1, "Verlauf", "Schläft schlecht"), fact(1, "Verlauf", "Schwester hilft"), fact(2, "Befund", "Gedrückt")];
        let p = prep(&["symptomatik_befund"], &facts, Dictionary::new(), None).unwrap();
        // Wrong closing bracket too (`}}` for `}]}`), as Qwen3 14B writes it.
        let answer = r#"{"sentences": [{"text": "Pat. schläft schlecht [F1]. Die Schwester hilft, Stimmung gedrückt. [F2, F3] Ohne Beleg."}}"#;
        let out = parse(answer, &p.sections[0], &facts, &p.map).unwrap();
        assert_eq!(
            out.sentences,
            vec![
                DraftSentence { text: "Pat. schläft schlecht.".into(), facts: vec![0] },
                DraftSentence { text: "Die Schwester hilft, Stimmung gedrückt.".into(), facts: vec![1, 2] },
            ]
        );
        assert_eq!(out.dropped, 1);
        // With `facts` given, inline markers are only stripped.
        let out = parse(r#"[{"text": "A [F1]. B [F2].", "facts": [3]}]"#, &p.sections[0], &facts, &p.map).unwrap();
        assert_eq!(out.sentences, vec![DraftSentence { text: "A. B.".into(), facts: vec![2] }]);
    }

    #[test]
    fn parse_checks_citations_and_names() {
        let facts = vec![fact(1, "Verlauf", "Seit dem 3.5.2024 Schlafstörungen"), fact(1, "Verlauf", "Schwester Gerda hilft")];
        let mut dict = Dictionary::from_patient(&patient());
        dict.add(EntityKind::Person, "Gerda");
        let p = prep(&["symptomatik_befund"], &facts, dict, None).unwrap();
        let answer = r#"<think>…</think>```json
        {"sentences": [
          {"text": "Pat.  berichtet seit dem [DATUM_1]\nSchlafstörungen.", "facts": [1]},
          {"text": "Pat. berichtet seit dem [DATUM_1] Schlafstörungen.", "facts": ["F1"]},
          {"text": "Unterstützung durch die Schwester [F2] (F1, F2).", "facts": ["[F2]", 2, 1]},
          {"text": "Schwester hilft, auch nachts. [F1], [F2],", "facts": [1, 2]},
          {"text": "Schwester hilft im Alltag, [F2].", "facts": [2]},
          {"text": "Unterstützung durch [PERSON_1].", "facts": [2]},
          {"text": "Verdacht auf F51.0.", "facts": [1]},
          {"text": "Ohne Beleg.", "facts": []},
          {"text": "Falscher Beleg.", "facts": [3]},
          {"text": "Erfunden [ORT_4].", "facts": [1]},
          {"text": "", "facts": [1]},
          42
        ]}
        ```"#;
        let out = parse(answer, &p.sections[0], &facts, &p.map).unwrap();
        assert_eq!(
            out.sentences,
            vec![
                DraftSentence { text: "Pat. berichtet seit dem 3.5.2024 Schlafstörungen.".into(), facts: vec![0] },
                DraftSentence { text: "Unterstützung durch die Schwester.".into(), facts: vec![0, 1] },
                DraftSentence { text: "Schwester hilft, auch nachts.".into(), facts: vec![0, 1] },
                DraftSentence { text: "Schwester hilft im Alltag.".into(), facts: vec![1] },
            ]
        );
        assert_eq!(out.dropped, 7);
        assert!(parse("[]", &p.sections[0], &facts, &p.map).unwrap().sentences.is_empty());
        assert!(parse("Geht nicht.", &p.sections[0], &facts, &p.map).is_err());
        assert_eq!(html(out.sentences.iter().map(|s| s.text.as_str())), "<p>Pat. berichtet seit dem 3.5.2024 Schlafstörungen. Unterstützung durch die Schwester. Schwester hilft, auch nachts. Schwester hilft im Alltag.</p>");
        assert_eq!(html(["a < b & c"]), "<p>a &lt; b &amp; c</p>");
    }
}
