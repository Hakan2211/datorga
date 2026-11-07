//! Template builder (slice 51): her own forms and letters become her own Vorlagen.
//!
//! A form or letter (already redacted by the style review, slice 49) is read into an outline
//! locally: headings, the questions and instructions under them, the Betreff, reference lines,
//! the closing sentence and who it goes to. Optionally one AI request adds hints and questions
//! per section ([`prepare`] / [`parse`]). Either way the result is a draft [`Template`] she edits
//! in the template editor before it is saved; nothing here writes text for a patient.
//!
//! Safety rules are the template validator's (MDR boundary): a Diagnosen section is filled in by
//! her only (no AI, no questions), whatever the form or the model says.

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::RecipientKind;
use crate::llm::{ChatOptions, Effort};
use crate::pseudo::{Dictionary, Pseudonymized, Pseudonymizer, TokenMap};
use crate::templates::{Ask, Audience, Category, FieldKind, Hint, Layout, PatientRef, RefDef, Rules, SectionDef, Template};
use crate::{Error, Result};

pub const PROMPT_VERSION: &str = "builder-2026.1";
/// Keys of her own templates start with this; built-in keys never do.
pub const OWN_PREFIX: &str = "eigen-";
pub const MAX_TEXT_CHARS: usize = 30_000;
pub const MAX_SECTIONS: usize = 20;
const MAX_HINTS: usize = 6;
const MAX_ASKS: usize = 6;
const MAX_CHIPS: usize = 4;
const MAX_LINE: usize = 300;

/// Whether `key` names one of her own templates.
pub fn is_own(key: &str) -> bool {
    key.starts_with(OWN_PREFIX)
}

/// `eigen-<slug of the title>-<4 hex>`: stable once saved, unique enough per practice.
pub fn new_key(title: &str) -> String {
    let slug: String = slug(title).chars().take(32).collect();
    let mut h = Sha256::new();
    h.update(title.as_bytes());
    h.update(uuid::Uuid::new_v4().as_bytes());
    let tail = &hex::encode(h.finalize())[..4];
    if slug.is_empty() { format!("{OWN_PREFIX}vorlage-{tail}") } else { format!("{OWN_PREFIX}{slug}-{tail}") }
}

/// Lower-case ASCII words joined by `-` (umlauts spelled out).
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        match c {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            c if c.is_ascii_alphanumeric() => out.push(c),
            _ => {
                if !out.ends_with('-') && !out.is_empty() {
                    out.push('-')
                }
            }
        }
    }
    out.trim_end_matches('-').to_string()
}

/// A section key from its heading, unique among `taken`.
pub fn section_key(heading: &str, taken: &[String]) -> String {
    let base: String = slug(heading).replace('-', "_").chars().take(32).collect();
    let base = base.trim_end_matches('_').to_string();
    let base = if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) { format!("abschnitt_{base}").trim_end_matches('_').to_string() } else { base };
    if !taken.iter().any(|k| k == &base) {
        return base;
    }
    (2..).map(|n| format!("{base}_{n}")).find(|k| !taken.iter().any(|t| t == k)).expect("some key is free")
}

// ---------- local outline ----------

/// One heading of the document with what was written under it.
#[derive(Debug, Clone, PartialEq)]
struct Part {
    heading: String,
    /// Questions under the heading (end with "?").
    questions: Vec<String>,
    /// Instructions under the heading ("Bitte …", "Angaben zu …", "(z. B. …)").
    instructions: Vec<String>,
    /// Characters of other text (her own letter text: content, not template).
    prose: usize,
}

/// What the local reading found besides the sections.
#[derive(Debug, Default)]
struct Frame {
    subject: Option<String>,
    refs: Vec<RefDef>,
    closing: Option<String>,
}

/// A draft template from her redacted form or letter, without AI.
pub fn outline(text: &str, title: &str) -> Result<Template> {
    let text = text.trim();
    if text.chars().count() < 40 {
        return Err(Error::Invalid("Zu wenig Text, um eine Gliederung zu erkennen".into()));
    }
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut frame = Frame::default();
    let mut parts: Vec<Part> = Vec::new();
    let mut opening = Part { heading: String::new(), questions: vec![], instructions: vec![], prose: 0 };
    let mut in_body = false;
    let mut after_greeting = false;
    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        if after_greeting {
            break; // signature block
        }
        if let Some(subject) = strip_label(line, &["Betreff", "Betr.", "Betr"]) {
            frame.subject = Some(subject_placeholders(subject));
            continue;
        }
        if let Some(r) = ref_line(line) {
            if !frame.refs.iter().any(|x| x.label == r.label) {
                frame.refs.push(r);
            }
            continue;
        }
        if is_salutation(line) {
            in_body = true;
            continue;
        }
        if is_greeting(line) {
            // The sentence before "Mit freundlichen Grüßen" is the closing.
            if let Some(prev) = lines[..i].iter().rev().find(|l| !l.is_empty()) {
                if prev.ends_with('.') && prev.chars().count() <= 160 && !is_heading(prev, None) {
                    frame.closing = Some(prev.to_string());
                    if let Some(p) = parts.last_mut().or(Some(&mut opening)) {
                        p.prose = p.prose.saturating_sub(prev.chars().count());
                    }
                }
            }
            after_greeting = true;
            continue;
        }
        let next = lines[i + 1..].iter().find(|l| !l.is_empty()).copied();
        if is_heading(line, next) {
            in_body = true;
            let (heading, rest) = split_heading(line);
            parts.push(Part { heading, questions: vec![], instructions: vec![], prose: 0 });
            if let Some(rest) = rest {
                add_line(parts.last_mut().unwrap(), rest);
            }
            continue;
        }
        if !in_body && parts.is_empty() {
            // Letterhead and address block before the Anrede: not part of the template.
            if looks_like_address(line) {
                continue;
            }
        }
        add_line(parts.last_mut().unwrap_or(&mut opening), line);
    }

    let audience = guess_audience(text);
    let recipients = guess_recipients(text);
    let mut sections: Vec<SectionDef> = Vec::new();
    let mut taken: Vec<String> = Vec::new();
    if opening.prose > 0 || !opening.questions.is_empty() || !opening.instructions.is_empty() || parts.is_empty() {
        let key = "einleitung".to_string();
        taken.push(key.clone());
        sections.push(section(key, String::new(), &opening, false));
    }
    for p in parts.iter().take(MAX_SECTIONS) {
        let key = section_key(&p.heading, &taken);
        taken.push(key.clone());
        sections.push(section(key, p.heading.clone(), p, true));
    }
    let category = if parts.len() >= 3 || matches!(audience, Some(Audience::Behoerde | Audience::Arzt)) { Category::Bericht } else { Category::Brief };
    let audience = audience.or(Some(Audience::Frei));
    let title = title.trim();
    let title = if title.is_empty() { "Eigene Vorlage".to_string() } else { title.chars().take(80).collect() };
    let t = Template {
        key: new_key(&title),
        version: "1".into(),
        title,
        source: Some("Eigene Vorlage".into()),
        report_kinds: vec![],
        category,
        layout: Layout::Letter,
        description: None,
        subject: frame.subject,
        refs: frame.refs,
        closing: frame.closing,
        recipients,
        release: matches!(audience, Some(Audience::Behoerde | Audience::Arzt)),
        audience: if category == Category::Brief && audience == Some(Audience::Frei) { None } else { audience },
        rules: Rules { target_pages: None, keyword_style_allowed: false, signature_required: true, pseudonymized: false, numbered: false },
        intro: String::new(),
        sections,
    };
    let t = guard(t);
    t.validate()?;
    Ok(t)
}

fn section(key: String, heading: String, p: &Part, with_heading: bool) -> SectionDef {
    let diagnoses = with_heading && is_diagnoses(&heading);
    let mut s = SectionDef {
        key,
        number: None,
        heading: if with_heading { heading } else { String::new() },
        field: if diagnoses { FieldKind::Diagnoses } else { FieldKind::Prose },
        ai_allowed: !diagnoses,
        required: false,
        optional: false,
        dictation: false,
        starter: None,
        when: Default::default(),
        hints: p.instructions.iter().take(MAX_HINTS).map(|h| Hint { text: h.clone(), when: Default::default() }).collect(),
        asks: vec![],
        prior: vec![],
    };
    if !diagnoses {
        let mut keys: Vec<String> = Vec::new();
        for q in p.questions.iter().take(MAX_ASKS) {
            let k = section_key(q, &keys);
            keys.push(k.clone());
            s.asks.push(Ask { key: k, question: q.clone(), chips: vec![], record: None, when: Default::default() });
        }
        // A form's question is also what the section should say.
        if s.hints.is_empty() {
            s.hints = p.questions.iter().take(MAX_HINTS).map(|q| Hint { text: q.trim_end_matches('?').to_string(), when: Default::default() }).collect();
        }
    }
    s
}

fn add_line(p: &mut Part, line: &str) {
    let line = line.trim_start_matches(['-', '•', '–', '*', '□', '☐', '_']).trim();
    let line = line.trim_end_matches('_').trim();
    if line.is_empty() {
        return;
    }
    let short: String = line.chars().take(MAX_LINE).collect();
    if line.ends_with('?') && line.chars().count() <= MAX_LINE {
        p.questions.push(short);
    } else if is_instruction(line) {
        p.instructions.push(short.trim_end_matches(['.', ':']).to_string());
    } else {
        p.prose += line.chars().count();
    }
}

fn is_instruction(line: &str) -> bool {
    let l = line.to_lowercase();
    line.chars().count() <= MAX_LINE
        && (l.starts_with("bitte ")
            || l.starts_with("angaben ")
            || l.starts_with("angabe ")
            || l.starts_with("(z. b.")
            || l.starts_with("(z.b.")
            || l.starts_with("z. b.")
            || l.starts_with("hier ")
            || l.starts_with("beschreiben sie")
            || l.starts_with("schildern sie")
            || l.starts_with("nennen sie")
            || l.starts_with("geben sie"))
}

/// A heading: short, no full sentence, and numbered, ending in ":", all capitals, or a short
/// title line followed by text.
fn is_heading(line: &str, next: Option<&str>) -> bool {
    let (head, rest) = split_heading(line);
    let n = head.chars().count();
    if n < 3 || n > 80 || head.ends_with(['.', ',', ';']) || head.ends_with('?') {
        return false;
    }
    if is_salutation(line) || is_greeting(line) || head.contains(['[', ']']) {
        return false;
    }
    let words = head.split_whitespace().count();
    let numbered = numbering(line).is_some();
    let colon = line.trim_end().ends_with(':') || (rest.is_some() && line.contains(':'));
    let letters: Vec<char> = head.chars().filter(|c| c.is_alphabetic()).collect();
    let caps = letters.len() >= 4 && letters.iter().all(|c| c.is_uppercase());
    let starts_upper = head.chars().next().is_some_and(char::is_uppercase);
    let title_like = words <= 6 && starts_upper && !head.contains(':') && next.is_some_and(|n| n.chars().count() > n_chars_body());
    (numbered && words <= 12 && starts_upper_after_number(line)) || (colon && words <= 8 && starts_upper) || caps || title_like
}

fn n_chars_body() -> usize {
    60
}

fn starts_upper_after_number(line: &str) -> bool {
    let rest = numbering(line).map_or(line, |n| line[n..].trim_start());
    rest.chars().next().is_some_and(char::is_uppercase)
}

/// Byte length of a leading "1.", "2.3", "a)", "IV." numbering, if any.
fn numbering(line: &str) -> Option<usize> {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
        i += 1;
    }
    if i > 0 && i < b.len() && b[..i].iter().any(u8::is_ascii_digit) && (b[i - 1] == b'.' || b[i] == b')') {
        let j = if b[i] == b')' { i + 1 } else { i };
        return (j < b.len() && b[j] == b' ').then_some(j);
    }
    // "a) " / "B) "
    if b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b')' && b[2] == b' ' {
        return Some(2);
    }
    // Roman numerals "IV. "
    let roman = line.chars().take_while(|c| matches!(c, 'I' | 'V' | 'X')).count();
    if roman > 0 && line[roman..].starts_with(". ") {
        return Some(roman + 1);
    }
    None
}

/// Heading text without numbering and colon, and text on the same line after the colon.
fn split_heading(line: &str) -> (String, Option<&str>) {
    let start = numbering(line).unwrap_or(0);
    let body = line[start..].trim();
    match body.split_once(':') {
        Some((h, rest)) if !rest.trim().is_empty() && h.split_whitespace().count() <= 8 => (h.trim().to_string(), Some(rest.trim())),
        _ => (body.trim_end_matches(':').trim().to_string(), None),
    }
}

fn strip_label<'a>(line: &'a str, labels: &[&str]) -> Option<&'a str> {
    for l in labels {
        if let Some(rest) = line.strip_prefix(l) {
            let rest = rest.trim_start();
            let rest = rest.strip_prefix(':').unwrap_or(rest).trim();
            if !rest.is_empty() && (line[l.len()..].trim_start().starts_with(':') || l.ends_with('.')) {
                return Some(rest);
            }
        }
    }
    None
}

/// The redaction review's placeholders become the letter's: `[Name]` → `{Name}`, a date after
/// "geb." → `{Geburtsdatum}`, "Frau [Name]" → "{Frau/Herrn} {Name}".
fn subject_placeholders(subject: &str) -> String {
    // First and last name are marked one by one: "[Name] [Name]" is one name.
    let mut subject = subject.to_string();
    while subject.contains("[Name] [Name]") {
        subject = subject.replace("[Name] [Name]", "[Name]");
    }
    let s = subject
        .replace("geb. [Datum]", "geb. {Geburtsdatum}")
        .replace("geboren am [Datum]", "geboren am {Geburtsdatum}")
        .replace("Frau [Name]", "{Frau/Herrn} [Name]")
        .replace("Herrn [Name]", "{Frau/Herrn} [Name]")
        .replace("Herr [Name]", "{Frau/Herr} [Name]")
        .replace("[Name]", "{Name}");
    s.chars().take(200).collect()
}

/// "Aktenzeichen: [Zeichen]", "Versicherungsnummer: …": a reference line under the Betreff.
fn ref_line(line: &str) -> Option<RefDef> {
    let (label, value) = line.split_once(':')?;
    let label = label.trim();
    if value.trim().is_empty() && !line.ends_with(':') {
        return None;
    }
    let l = label.to_lowercase();
    let from = if l.contains("rentenversicherungsnummer") || (l.contains("versicherungsnummer") && !l.contains("kranken")) {
        Some(PatientRef::PensionNo)
    } else if l.contains("versichertennummer") || l.contains("kv-nummer") || l.contains("krankenversichertennummer") {
        Some(PatientRef::InsuranceNo)
    } else if l == "krankenkasse" || l == "kostenträger" {
        Some(PatientRef::Insurer)
    } else {
        None
    };
    let known = from.is_some()
        || ["aktenzeichen", "az.", "az", "ihr zeichen", "unser zeichen", "geschäftszeichen", "kennzeichen", "vorgangsnummer", "bg-nummer", "pflegegrad-antrag"]
            .iter()
            .any(|k| l == *k || l.starts_with(&format!("{k} ")));
    (known && label.chars().count() <= 40).then(|| RefDef { label: label.to_string(), from, required: false })
}

fn is_salutation(line: &str) -> bool {
    let l = line.to_lowercase();
    (l.starts_with("sehr geehrte") || l.starts_with("liebe ") || l.starts_with("lieber ") || l.starts_with("guten tag")) && line.chars().count() <= 100
}

fn is_greeting(line: &str) -> bool {
    let l = line.to_lowercase();
    l.starts_with("mit freundlichen grüßen") || l.starts_with("mit freundlichem gruß") || l.starts_with("freundliche grüße") || l.starts_with("mit kollegialen grüßen") || l.starts_with("mit besten grüßen")
}

fn looks_like_address(line: &str) -> bool {
    let l = line.to_lowercase();
    line.chars().count() <= 60 && !line.ends_with('.') || l.contains("[ort]") || l.contains("[kontakt]")
}

fn is_diagnoses(heading: &str) -> bool {
    let h = heading.to_lowercase();
    h.contains("diagnose") || h.contains("icd")
}

fn guess_audience(text: &str) -> Option<Audience> {
    let t = text.to_lowercase();
    let behoerde = ["rentenversicherung", "versorgungsamt", "landratsamt", "pflegekasse", "medizinischer dienst", "jobcenter", "agentur für arbeit", "sozialamt", "jugendamt", "berufsgenossenschaft", "gericht", "behörde"];
    if behoerde.iter().any(|k| t.contains(k)) {
        return Some(Audience::Behoerde);
    }
    if t.contains("kollegin") || t.contains("kollege") || t.contains("dr. med") || t.contains("arztbrief") || t.contains("hausarzt") {
        return Some(Audience::Arzt);
    }
    None
}

fn guess_recipients(text: &str) -> Vec<RecipientKind> {
    let t = text.to_lowercase();
    let mut out = Vec::new();
    for (k, words) in [
        (RecipientKind::Rentenversicherung, &["rentenversicherung", "drv"][..]),
        (RecipientKind::Pflegekasse, &["pflegekasse", "pflegegrad", "medizinischer dienst"][..]),
        (RecipientKind::Versorgungsamt, &["versorgungsamt", "landratsamt", "schwerbehindert", "gdb"][..]),
        (RecipientKind::Arzt, &["kollegin", "kollege", "dr. med", "hausarzt"][..]),
        (RecipientKind::Krankenkasse, &["krankenkasse", "aok", "techniker", "barmer"][..]),
    ] {
        if words.iter().any(|w| t.contains(w)) {
            out.push(k);
        }
    }
    out
}

/// The MDR boundary and the validator's rules, applied to any draft (local, AI or edited):
/// Diagnosen without AI and questions, questions only on AI prose, starters only in prose,
/// lengths capped.
pub fn guard(mut t: Template) -> Template {
    t.layout = Layout::Letter;
    if !matches!(t.category, Category::Bericht | Category::Brief) {
        t.category = Category::Bericht;
    }
    t.report_kinds.clear();
    if t.category == Category::Bericht && t.audience.is_none() {
        t.audience = Some(Audience::Frei);
    }
    t.sections.truncate(MAX_SECTIONS);
    for s in &mut t.sections {
        if is_diagnoses(&s.heading) {
            s.field = FieldKind::Diagnoses;
        }
        if s.field == FieldKind::Diagnoses {
            s.ai_allowed = false;
            s.asks.clear();
            s.starter = None;
        }
        if !s.ai_allowed {
            s.asks.clear();
        }
        s.hints.truncate(MAX_HINTS * 2);
        s.asks.truncate(MAX_ASKS * 2);
        for a in &mut s.asks {
            a.chips.truncate(MAX_CHIPS * 2);
        }
        // The opening paragraph has text from the start: its questions would never count as open.
        if s.starter.as_deref().is_some_and(|x| !x.trim().is_empty()) {
            s.asks.clear();
        }
    }
    t
}

// ---------- AI proposal ----------

pub struct Prepared {
    pub system: Pseudonymized<String>,
    pub user: Pseudonymized<String>,
    pub map: TokenMap,
    pub prompt_hash: String,
    pub digest: String,
}

impl Prepared {
    pub fn preview(&self) -> crate::extract::Preview {
        crate::extract::Preview {
            shared: Vec::new(),
            messages: vec![self.user.get().clone()],
            tokens: self.map.entries().iter().map(crate::extract::TokenView::from).collect(),
            digest: self.digest.clone(),
        }
    }
}

pub fn system_prompt() -> String {
    "Du hilfst einer Psychotherapeutin, aus einem ihrer Formulare oder Briefe eine wiederverwendbare \
Vorlage zu machen. Du bekommst den Text (Namen und Daten sind geschwärzt) und die Gliederung, die die \
App erkannt hat.\n\
Antworte nur mit JSON:\n\
{\"title\": \"…\", \"description\": \"ein Satz, wofür die Vorlage ist\", \
\"audience\": \"behoerde|arzt|patient|frei\", \"subject\": \"Betreff mit Platzhaltern oder leer\", \
\"sections\": [{\"heading\": \"…\", \"diagnoses\": false, \"optional\": false, \
\"hints\": [\"was in den Abschnitt gehört\"], \
\"questions\": [{\"question\": \"…?\", \"chips\": [\"kurze Antwort\"]}]}]}\n\
Regeln:\n\
- Übernimm die Gliederung des Dokuments: dieselben Abschnitte in derselben Reihenfolge, Überschriften \
wörtlich. Ein erster Abschnitt ohne Überschrift (\"heading\": \"\") ist die Einleitung.\n\
- \"hints\": 1 bis 4 kurze Stichpunkte je Abschnitt, was die Therapeutin dort schreibt. Allgemein, nie \
Inhalte über eine Person aus dem Text.\n\
- \"questions\": 0 bis 4 Fragen je Abschnitt, die die App ihr vor dem Schreiben stellt, mit bis zu 3 \
kurzen Antwortvorschlägen (\"keine\", \"nicht bekannt\"). Fragen aus einem Formular wörtlich übernehmen.\n\
- Ein Abschnitt zu Diagnosen oder ICD-Codes: \"diagnoses\": true, ohne Hinweise zur Formulierung und \
ohne Fragen. Diagnosen trägt nur die Therapeutin ein.\n\
- \"optional\": true für Abschnitte, die nicht in jedem Fall gebraucht werden.\n\
- Platzhalter im Betreff: {Name}, {Geburtsdatum}, {Frau/Herrn}. Keine anderen.\n\
- Erfinde keine Abschnitte, die das Dokument nicht hat. Höchstens 20 Abschnitte."
        .to_string()
}

/// The outgoing request: her (redacted) text with the local outline. Pseudonymized again with
/// `dict` (all patients and her terms), fail-closed like every request.
pub fn prepare(text: &str, outline: &Template, dict: Dictionary) -> Result<Prepared> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::Invalid("Kein Text für den Vorschlag".into()));
    }
    let chars = text.chars().count();
    if chars > MAX_TEXT_CHARS {
        return Err(Error::Invalid(format!("Das Dokument ist zu lang für einen Vorschlag ({chars} Zeichen, höchstens {MAX_TEXT_CHARS})")));
    }
    let mut user = String::from("Erkannte Gliederung:\n");
    for s in &outline.sections {
        user.push_str(&format!("- {}\n", if s.heading.trim().is_empty() { "(Einleitung)" } else { s.heading.trim() }));
    }
    user.push_str("\nDokument:\n");
    user.push_str(text);
    user.push('\n');
    let mut p = Pseudonymizer::new(dict);
    let system_raw = system_prompt();
    let prompt_hash = hex::encode(Sha256::digest(format!("{PROMPT_VERSION}\n{system_raw}").as_bytes()));
    let system = p.pseudonymize(&system_raw)?;
    let user = p.pseudonymize(&user)?;
    let mut h = Sha256::new();
    h.update(system.get());
    h.update([0]);
    h.update(user.get());
    Ok(Prepared { system, user, map: p.into_map(), prompt_hash, digest: hex::encode(h.finalize()) })
}

pub fn chat_options() -> ChatOptions {
    ChatOptions { temperature: 0.2, max_tokens: 6144, timeout: std::time::Duration::from_secs(300), reasoning: Some(Effort::Low) }
}

#[derive(Deserialize)]
struct RawTemplate {
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    audience: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    sections: Vec<RawSection>,
}

#[derive(Deserialize)]
struct RawSection {
    #[serde(default)]
    heading: String,
    #[serde(default)]
    diagnoses: bool,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    hints: Vec<Value>,
    #[serde(default)]
    questions: Vec<Value>,
}

/// The model's proposal merged onto the local outline: the outline keeps her letter frame
/// (refs, closing, recipients, key); sections, hints and questions come from the model, cleaned
/// and guarded. Tokens (`[PERSON_1]`) are cut, never restored: a template is reused for every
/// patient, so no name from the document may end up in it.
pub fn parse(raw: &str, outline: &Template) -> Result<Template> {
    let value = crate::extract::json_value(raw)?;
    let r: RawTemplate = serde_json::from_value(value).map_err(|e| Error::Llm(format!("Die KI hat keinen lesbaren Vorschlag geliefert ({e})")))?;
    let clean = |s: &str, max: usize| -> String {
        let s = strip_tokens(s.trim());
        s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
    };
    let mut t = outline.clone();
    let title = clean(&r.title, 80);
    if !title.is_empty() {
        t.title = title;
    }
    let description = clean(&r.description, 200);
    t.description = (!description.is_empty()).then_some(description);
    t.audience = match r.audience.trim() {
        "behoerde" => Some(Audience::Behoerde),
        "arzt" => Some(Audience::Arzt),
        "patient" => Some(Audience::Patient),
        "frei" => Some(Audience::Frei),
        _ => t.audience,
    };
    let subject = clean(&r.subject, 200);
    if !subject.is_empty() && t.subject.is_none() {
        t.subject = Some(subject);
    }
    let mut sections: Vec<SectionDef> = Vec::new();
    let mut taken: Vec<String> = Vec::new();
    for (i, rs) in r.sections.iter().take(MAX_SECTIONS).enumerate() {
        let heading = clean(&rs.heading, 80);
        if heading.is_empty() && i > 0 {
            continue;
        }
        let key = if heading.is_empty() { "einleitung".to_string() } else { section_key(&heading, &taken) };
        taken.push(key.clone());
        let diagnoses = rs.diagnoses || is_diagnoses(&heading);
        // A hint or question about someone in the document ("Befund bei [PERSON_1]") is dropped
        // whole: cut down it would read wrong, and it is about that person, not the form.
        let hints = rs.hints.iter().filter_map(|h| h.as_str()).filter(|h| !has_token(h)).map(|h| clean(h, MAX_LINE)).filter(|h| !h.is_empty()).take(MAX_HINTS);
        let mut asks: Vec<Ask> = Vec::new();
        let mut ask_keys: Vec<String> = Vec::new();
        for q in rs.questions.iter().take(MAX_ASKS) {
            let (raw_q, chips) = match q {
                Value::String(s) => (s.as_str(), vec![]),
                Value::Object(o) => (
                    o.get("question").and_then(Value::as_str).unwrap_or(""),
                    o.get("chips").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(|c| clean(c, 40)).filter(|c| !c.is_empty()).take(MAX_CHIPS).collect()).unwrap_or_default(),
                ),
                _ => continue,
            };
            let question = clean(raw_q, MAX_LINE);
            if question.is_empty() || has_token(raw_q) {
                continue;
            }
            let k = section_key(&question, &ask_keys);
            ask_keys.push(k.clone());
            asks.push(Ask { key: k, question, chips, record: None, when: Default::default() });
        }
        // The outline's version of the same heading keeps a starter she may already have.
        let old = outline.sections.iter().find(|s| s.heading.trim().eq_ignore_ascii_case(heading.trim()));
        sections.push(SectionDef {
            key: old.map_or(key, |o| o.key.clone()),
            number: None,
            heading,
            field: if diagnoses { FieldKind::Diagnoses } else { FieldKind::Prose },
            ai_allowed: !diagnoses,
            required: false,
            optional: rs.optional && !diagnoses,
            dictation: false,
            starter: old.and_then(|o| o.starter.clone()),
            when: Default::default(),
            hints: if diagnoses { vec![] } else { hints.map(|text| Hint { text, when: Default::default() }).collect() },
            asks: if diagnoses { vec![] } else { asks },
            prior: vec![],
        });
    }
    // Keys taken over from the outline may now repeat: make them unique again.
    let mut seen: Vec<String> = Vec::new();
    for s in &mut sections {
        if seen.contains(&s.key) {
            s.key = section_key(&s.heading, &seen);
        }
        seen.push(s.key.clone());
    }
    if sections.is_empty() {
        return Err(Error::Invalid("Die KI hat keine Abschnitte vorgeschlagen".into()));
    }
    t.sections = sections;
    if t.category == Category::Brief && t.audience.is_some_and(|a| a != Audience::Frei) && t.sections.len() >= 3 {
        t.category = Category::Bericht;
    }
    let t = guard(t);
    t.validate()?;
    Ok(t)
}

fn has_token(s: &str) -> bool {
    strip_tokens(s) != s
}

/// `[PERSON_1]`, `[ORT_2]`, … left in model text are cut.
fn strip_tokens(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        match after.find(']') {
            Some(j) if after[1..j].chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') && after[1..j].contains('_') => {
                rest = &after[j + 1..];
            }
            _ => {
                out.push('[');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A copy of a built-in letter template as her own, to adapt.
pub fn copy_of(builtin: &Template) -> Result<Template> {
    if builtin.layout != Layout::Letter || is_own(&builtin.key) {
        return Err(Error::Invalid("Nur mitgelieferte Brief- und Berichtvorlagen lassen sich als eigene Vorlage anpassen".into()));
    }
    let mut t = builtin.clone();
    t.title = format!("{} (eigene)", builtin.title).chars().take(80).collect();
    t.key = new_key(&builtin.title);
    t.version = "1".into();
    t.source = Some("Eigene Vorlage".into());
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORM: &str = "Landratsamt Musterkreis\nVersorgungsamt\n[Ort]\n\n\
Betreff: Befundbericht für Frau [Name], geb. [Datum]\n\
Aktenzeichen: [Zeichen]\n\n\
Sehr geehrte Damen und Herren,\n\n\
1. Seit wann und wie oft behandeln Sie die Antragstellerin?\n\n\
2. Diagnosen (ICD-10):\n\n\
3. Psychischer Befund\n\
Bitte beschreiben Sie den aktuellen psychischen Befund.\n\
Welche Funktionseinschränkungen bestehen im Alltag?\n\n\
4. Therapie und Prognose\n\
Ist eine Besserung zu erwarten?\n\n\
Für Rückfragen stehe ich gerne zur Verfügung.\n\n\
Mit freundlichen Grüßen\n[Name]\n";

    #[test]
    fn a_form_becomes_an_outline() {
        let t = outline(FORM, "Versorgungsamt Fragebogen").unwrap();
        assert!(is_own(&t.key), "{}", t.key);
        assert_eq!(t.layout, Layout::Letter);
        assert_eq!(t.category, Category::Bericht);
        assert_eq!(t.audience, Some(Audience::Behoerde));
        assert!(t.recipients.contains(&RecipientKind::Versorgungsamt));
        assert!(t.release);
        assert_eq!(t.subject.as_deref(), Some("Befundbericht für {Frau/Herrn} {Name}, geb. {Geburtsdatum}"));
        let two = outline(&FORM.replace("Frau [Name]", "Frau [Name] [Name]"), "x").unwrap();
        assert_eq!(two.subject, t.subject, "first and last name marked one by one");
        assert_eq!(t.refs.len(), 1);
        assert_eq!(t.refs[0].label, "Aktenzeichen");
        assert_eq!(t.closing.as_deref(), Some("Für Rückfragen stehe ich gerne zur Verfügung."));
        let heads: Vec<&str> = t.sections.iter().map(|s| s.heading.as_str()).collect();
        // A numbered question line is a heading too (the form's question 1).
        assert!(heads.contains(&"Diagnosen (ICD-10)"), "{heads:?}");
        assert!(heads.contains(&"Psychischer Befund"), "{heads:?}");
        assert!(heads.contains(&"Therapie und Prognose"), "{heads:?}");
        let diag = t.sections.iter().find(|s| s.heading.starts_with("Diagnosen")).unwrap();
        assert_eq!((diag.field, diag.ai_allowed, diag.asks.len()), (FieldKind::Diagnoses, false, 0));
        let befund = t.sections.iter().find(|s| s.heading == "Psychischer Befund").unwrap();
        assert_eq!(befund.hints[0].text, "Bitte beschreiben Sie den aktuellen psychischen Befund");
        assert_eq!(befund.asks[0].question, "Welche Funktionseinschränkungen bestehen im Alltag?");
        let prog = t.sections.iter().find(|s| s.heading == "Therapie und Prognose").unwrap();
        assert_eq!(prog.asks.len(), 1);
        assert!(!t.sections.iter().any(|s| s.heading.contains("Landratsamt")), "the address block is no section");
        t.validate().unwrap();
    }

    #[test]
    fn a_plain_letter_is_a_brief_with_one_text() {
        let t = outline("Sehr geehrte Frau [Name],\n\nhiermit bestätige ich, dass Sie sich seit [Datum] in meiner Behandlung befinden. Die Termine finden wöchentlich statt.\n\nMit freundlichen Grüßen\n", "Bestätigung").unwrap();
        assert_eq!(t.category, Category::Brief);
        assert_eq!(t.sections.len(), 1);
        assert_eq!(t.sections[0].heading, "");
        assert!(t.audience.is_none());
        t.validate().unwrap();
        assert!(outline("zu kurz", "x").is_err());
    }

    #[test]
    fn keys_and_slugs() {
        assert_eq!(slug("Befundbericht Versorgungsamt (Fragebogen)"), "befundbericht-versorgungsamt-fragebogen");
        assert_eq!(slug("Größe & Übel"), "groesse-uebel");
        assert_eq!(section_key("Psychischer Befund", &[]), "psychischer_befund");
        assert_eq!(section_key("Psychischer Befund", &["psychischer_befund".into()]), "psychischer_befund_2");
        assert_eq!(section_key("1990", &[]), "abschnitt_1990");
        assert!(new_key("Mein Brief").starts_with("eigen-mein-brief-"));
        assert_ne!(new_key("x"), new_key("x"));
    }

    #[test]
    fn the_guard_keeps_the_diagnoses_hers() {
        let mut t = outline(FORM, "x").unwrap();
        let d = t.sections.iter_mut().find(|s| s.field == FieldKind::Diagnoses).unwrap();
        d.ai_allowed = true;
        d.asks.push(Ask { key: "icd".into(), question: "Welche?".into(), chips: vec![], record: None, when: Default::default() });
        let t = guard(t);
        let d = t.sections.iter().find(|s| s.field == FieldKind::Diagnoses).unwrap();
        assert!(!d.ai_allowed && d.asks.is_empty());
        t.validate().unwrap();
    }

    #[test]
    fn the_ai_proposal_is_merged_and_guarded() {
        let base = outline(FORM, "Fragebogen").unwrap();
        let mut dict = Dictionary::default();
        dict.add(crate::pseudo::EntityKind::Person, "Gerda Muster");
        let p = prepare(&format!("{FORM}\nGerda Muster"), &base, dict).unwrap();
        assert!(!p.user.get().contains("Gerda"), "pseudonymized again");
        assert!(p.user.get().contains("Erkannte Gliederung"));
        let raw = r#"Hier: {"title": "Befundbericht Versorgungsamt (eigen)", "description": "Antworten auf den Fragebogen des Versorgungsamts", "audience": "behoerde",
          "sections": [
            {"heading": "", "hints": ["Seit wann in Behandlung"], "questions": []},
            {"heading": "Diagnosen (ICD-10)", "diagnoses": false, "hints": ["F32.1 nennen"], "questions": ["Welche Diagnose?"]},
            {"heading": "Psychischer Befund", "hints": ["Befund bei [PERSON_1]", "Antrieb, Stimmung"], "questions": [{"question": "Bestehen Funktionseinschränkungen?", "chips": ["keine", "leicht"]}]},
            {"heading": "Psychischer Befund", "hints": [], "questions": []}
          ]}"#;
        let t = parse(raw, &base).unwrap();
        assert_eq!(t.key, base.key, "the outline's key and frame stay");
        assert_eq!(t.refs.len(), 1);
        assert_eq!(t.title, "Befundbericht Versorgungsamt (eigen)");
        assert_eq!(t.sections.len(), 4);
        let d = &t.sections[1];
        assert_eq!((d.field, d.ai_allowed, d.hints.len(), d.asks.len()), (FieldKind::Diagnoses, false, 0, 0), "diagnoses stay hers");
        let b = &t.sections[2];
        assert_eq!(b.key, "psychischer_befund");
        assert_eq!(b.hints.iter().map(|h| h.text.as_str()).collect::<Vec<_>>(), ["Antrieb, Stimmung"], "the hint about a person is dropped");
        assert_eq!(b.asks[0].chips, ["keine", "leicht"]);
        assert_ne!(t.sections[3].key, b.key, "repeated heading gets its own key");
        assert!(parse("kein json", &base).is_err());
        assert!(parse(r#"{"sections": []}"#, &base).is_err());
    }

    #[test]
    fn copies_of_builtin_letters_only() {
        let all = crate::templates::builtin().unwrap();
        let drv = all.iter().find(|t| t.key == "drv-befund").unwrap();
        let c = copy_of(drv).unwrap();
        assert!(is_own(&c.key) && c.title.ends_with("(eigene)") && c.version == "1");
        assert_eq!(c.sections.len(), drv.sections.len());
        c.validate().unwrap();
        let ptv3 = all.iter().find(|t| t.key == "ptv3-fortfuehrung").unwrap();
        assert!(copy_of(ptv3).is_err());
        assert!(copy_of(&c).is_err());
    }
}
