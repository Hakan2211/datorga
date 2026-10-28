//! Per-session fact extraction (Phase 2, slice 13): the confirmed transcript of a session becomes
//! short facts per Sitzungsnotiz section, each citing the transcript segments it came from.
//!
//! The model sees numbered, pseudonymized segments (`[S12] …`) and answers with JSON. Its answer
//! is checked, never trusted: unknown sections, citations outside the chunk, invented tokens and
//! ICD codes (diagnoses stay with the therapist) are dropped. Long sessions are split into chunks
//! so a 16k-context local model can take them.
//!
//! Each fact also names the Bericht sections it belongs to (slice 19), so a section draft later
//! sees only its own facts. The tags use the sections of the Bericht for an Erst- or
//! Umwandlungsantrag; the two broad sections of a Fortführungsbericht take all facts of the
//! chosen sessions. Facts the model repeats across chunks are merged locally ([`merge`]).

use std::ops::Range;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::domain::{SourceKind, SourceSegment, Speaker};
use crate::llm::{ChatOptions, Effort};
use crate::pseudo::{DetectionSource, Dictionary, EntityKind, Pseudonymized, Pseudonymizer, TokenEntry, TokenMap};
use crate::templates::{FieldKind, Template};
use crate::{Error, Result};

/// Bump on any change to the prompt or the answer format.
pub const PROMPT_VERSION: &str = "extract-2026.3";

/// The Bericht template whose sections facts are tagged with (resolved for an Erstantrag).
pub const TAG_TEMPLATE_KEY: &str = "ptv3-erst-umwandlung";

/// Most Bericht sections one fact is tagged with.
pub const MAX_TAGS: usize = 3;

/// Facts small models leave untagged or misfile (eval, slice 19), as rules for the tag prompt.
/// Only rules whose section is offered are used.
const TAG_RULES: &[(&str, &str)] = &[
    ("symptomatik_befund", "Auch verneinte Befunde (z. B. Suizidgedanken verneint, keine Halluzinationen)"),
    (
        "lebensgeschichte_modell",
        "Auslöser, aufrechterhaltende Bedingungen, Gedanken und Gefühle in der Situation, Verstärkung durch Vermeiden, Konflikte",
    ),
    ("behandlungsplan_prognose", "Vereinbarungen, Hausaufgaben, geplante oder begonnene Interventionen, Einbezug von Bezugspersonen"),
];

/// Transcript characters per request. German runs at roughly 3–4 characters per token, so this
/// leaves room for the prompt and the answer within a 16k context.
pub const CHUNK_CHARS: usize = 24_000;

/// Request options for one extraction chunk (the eval harness uses the same).
pub fn chat_options() -> ChatOptions {
    ChatOptions { temperature: 0.1, max_tokens: 8192, timeout: Duration::from_secs(900), reasoning: Some(Effort::Low) }
}

/// Longest fact kept; anything longer is a paraphrase of the transcript, not a fact.
const MAX_FACT_CHARS: usize = 600;

/// One request: the segments `range` (0-based, end exclusive) as a pseudonymized user message.
pub struct PreparedChunk {
    pub range: Range<usize>,
    pub user: Pseudonymized<String>,
}

/// Everything that leaves the machine for one extraction, plus the local token map.
pub struct Prepared {
    pub system: Pseudonymized<String>,
    pub chunks: Vec<PreparedChunk>,
    pub map: TokenMap,
    /// SHA-256 of the system prompt (without patient data), stored with the result.
    pub prompt_hash: String,
    /// SHA-256 over all outgoing messages: what the therapist saw in the preview is what is sent.
    pub digest: String,
}

/// A fact as proposed by the model, tokens restored, citations as 0-based segment indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftFact {
    pub section: String,
    pub text: String,
    pub segments: Vec<usize>,
    /// Bericht sections the fact belongs to (keys of the tag template), possibly none.
    pub bericht: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub facts: Vec<DraftFact>,
    /// Entries that failed a check (counted for the audit log, contents discarded).
    pub dropped: usize,
}

/// The redaction preview: the outgoing text and which original each token stands for.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Preview {
    pub messages: Vec<String>,
    pub tokens: Vec<TokenView>,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TokenView {
    pub token: String,
    pub kind: EntityKind,
    pub original: String,
}

impl From<&TokenEntry> for TokenView {
    fn from(e: &TokenEntry) -> Self {
        Self { token: e.token.clone(), kind: e.kind, original: e.original.clone() }
    }
}

impl Prepared {
    pub fn preview(&self) -> Preview {
        Preview {
            messages: self.chunks.iter().map(|c| c.user.get().clone()).collect(),
            tokens: self.map.entries().iter().map(TokenView::from).collect(),
            digest: self.digest.clone(),
        }
    }
}

/// Sections the model may fill: prose sections that allow AI text.
pub fn target_sections(template: &Template) -> Vec<&crate::templates::SectionDef> {
    template.sections.iter().filter(|s| s.field == FieldKind::Prose && s.ai_allowed).collect()
}

/// Bericht sections a fact can be tagged with: the draftable sections of the tag template,
/// resolved for the episode's method and age group and an Erstantrag.
pub fn tag_sections(bericht: &Template) -> Vec<&crate::templates::SectionDef> {
    crate::draft::target_sections(bericht)
}

/// The system prompt for a (resolved) note template and the Bericht sections to tag facts
/// with (none: no tags asked). Holds no patient data.
pub fn system_prompt(template: &Template, tags: &[&crate::templates::SectionDef]) -> String {
    let mut sections = String::new();
    for s in target_sections(template) {
        sections.push_str(&format!("- \"{}\": {}\n", s.key, s.heading));
        for h in &s.hints {
            sections.push_str(&format!("    · {}\n", h.text.trim()));
        }
    }
    let (tag_task, tag_example) = if tags.is_empty() {
        (String::new(), "")
    } else {
        let mut list = String::new();
        for s in tags {
            let hints: Vec<&str> = s.hints.iter().map(|h| h.text.trim()).collect();
            list.push_str(&format!("- \"{}\": {}", s.key, s.heading.trim()));
            if !hints.is_empty() {
                list.push_str(&format!(" ({})", hints.join("; ")));
            }
            list.push('\n');
        }
        let rules: Vec<String> =
            TAG_RULES.iter().filter(|(key, _)| tags.iter().any(|s| s.key == *key)).map(|(key, rule)| format!("- {rule} → \"{key}\"\n")).collect();
        if !rules.is_empty() {
            list.push_str(&format!("Zuordnungshilfen:\n{}", rules.concat()));
        }
        (
            format!(
                "Ordne jeden Fakt außerdem im Feld \"bericht\" den Abschnitten des späteren Berichts an die \
                 Gutachterin zu, in die er inhaltlich gehört: meist einer, höchstens {MAX_TAGS}, eine leere Liste, \
                 wenn er in keinen gehört. Nur zuordnen, was der Abschnitt wirklich braucht:\n{list}\n"
            ),
            ", \"bericht\": [\"symptomatik_befund\"]",
        )
    };
    format!(
        "Du unterstützt eine Psychotherapeutin bei der Verlaufsdokumentation einer Sitzung.\n\
         Du erhältst die Abschrift der Sitzung als nummerierte Abschnitte ([S1], [S2] …). \
         Diktate der Therapeutin sind mit \"Diktat\" markiert, Sitzungsaufnahmen mit \"Sitzung\". \
         In Sitzungsaufnahmen steht vor dem Text, wer spricht: \"T:\" die Therapeutin, \"P:\" Pat.; \
         ohne Kürzel ist der Sprecher nicht bekannt. Fragen, Deutungen und Beobachtungen der Therapeutin \
         sind keine Aussagen von Pat.: als Beobachtung (\"wirkt …\") oder Intervention festhalten.\n\n\
         Aufgabe: Ziehe die dokumentationswürdigen Fakten heraus und ordne jeden einem dieser \
         Abschnitte der Sitzungsnotiz zu:\n{sections}\n\
         {tag_task}\
         Regeln:\n\
         - Nur was in der Abschrift steht. Nichts ergänzen, nicht deuten, nicht bewerten.\n\
         - Jeder Fakt nennt die Nummern der Abschnitte, aus denen er stammt (mindestens eine).\n\
         - Stichwortartig, sachlich, ein Gedanke pro Fakt, höchstens zwei kurze Sätze, auf Deutsch.\n\
         - Enthält ein Abschnitt Angaben für mehrere Abschnitte der Notiz, teile sie auf \
         (z. B. Stimmung und Antrieb in den Befund, Methoden in die Interventionen).\n\
         - Verneinte Angaben sind Fakten: \"Suizidgedanken verneint\" gehört in die Risikoeinschätzung.\n\
         - Keine Diagnosen, keine ICD-Codes, keine Vorschläge zu Medikamenten oder Dosierungen.\n\
         - Platzhalter in eckigen Klammern wie [PERSON_n] oder [ORT_n] unverändert übernehmen, \
         keine neuen erfinden. Beziehungen (Schwester, Mutter, Partner …) beibehalten, \
         keine Anrede (Frau, Herr) vor einen Platzhalter setzen, die nicht in der Abschrift steht.\n\
         - Smalltalk, Organisatorisches ohne Bezug zur Behandlung und Wiederholungen weglassen.\n\
         - Gibt die Abschrift nichts Relevantes her, antworte mit leerer Liste.\n\n\
         Antworte ausschließlich mit JSON in dieser Form, ohne weiteren Text:\n\
         {{\"facts\": [{{\"section\": \"verlauf\", \"text\": \"…\", \"segments\": [3, 4]{tag_example}}}]}}\n"
    )
}

/// Splits segments into chunks of at most `budget` characters (a longer single segment gets a
/// chunk of its own).
pub fn chunks(segments: &[SourceSegment], budget: usize) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let (mut start, mut size) = (0, 0);
    for (i, s) in segments.iter().enumerate() {
        let n = s.text.chars().count() + 24;
        if i > start && size + n > budget {
            out.push(start..i);
            (start, size) = (i, 0);
        }
        size += n;
    }
    if start < segments.len() {
        out.push(start..segments.len());
    }
    out
}

fn line(i: usize, s: &SourceSegment) -> String {
    let (kind, who) = match (s.kind, s.role) {
        (SourceKind::Dictation, _) => ("Diktat", ""),
        (SourceKind::SessionAudio, Speaker::Therapist) => ("Sitzung", "T: "),
        (SourceKind::SessionAudio, Speaker::Patient) => ("Sitzung", "P: "),
        (SourceKind::SessionAudio, Speaker::Unknown) => ("Sitzung", ""),
    };
    format!("[S{}] ({kind}, {}) {who}{}", i + 1, clock(s.start_ms), s.text.trim())
}

/// `mm:ss` (or `h:mm:ss`) from the start of the recording.
pub fn clock(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

/// Pseudonymizes the whole session with one token map. A name that the rules find anywhere
/// ("Frau X") is tokenized in every chunk, not just the one it appeared in.
pub fn prepare(
    template: &Template,
    tags: &[&crate::templates::SectionDef],
    segments: &[SourceSegment],
    mut dict: Dictionary,
    budget: usize,
) -> Result<Prepared> {
    if segments.is_empty() {
        return Err(Error::Invalid("Keine bestätigte Abschrift für diese Sitzung".into()));
    }
    if target_sections(template).is_empty() {
        return Err(Error::Invalid("Die Vorlage hat keine Abschnitte für KI-Entwürfe".into()));
    }
    let all: String = segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("\n");
    for d in Pseudonymizer::new(dict.clone()).detect(&all) {
        if d.source == DetectionSource::Rule && d.kind == EntityKind::Person {
            dict.add(EntityKind::Person, &d.text);
        }
    }
    let mut p = Pseudonymizer::new(dict);
    let system_raw = system_prompt(template, tags);
    let prompt_hash = sha256(&format!("{PROMPT_VERSION}\n{system_raw}"));
    let system = p.pseudonymize(&system_raw)?;
    let mut prepared = Vec::new();
    for range in chunks(segments, budget) {
        let text: Vec<String> = range.clone().map(|i| line(i, &segments[i])).collect();
        let user = p.pseudonymize(&format!("Abschrift:\n\n{}", text.join("\n")))?;
        prepared.push(PreparedChunk { range, user });
    }
    let mut h = Sha256::new();
    h.update(system.get());
    for c in &prepared {
        h.update([0]);
        h.update(c.user.get());
    }
    Ok(Prepared { system, chunks: prepared, map: p.into_map(), prompt_hash, digest: hex::encode(h.finalize()) })
}

pub fn sha256(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

static THINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<think>.*?</think>").expect("valid regex"));
// ICD-10 codes (F32.1, F43, Z73.0…). "U" codes are left out: they are rare and "U18" is no diagnosis talk.
pub(crate) static ICD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-TV-Z][0-9]{2}(\.[0-9]{1,2})?\b").expect("valid regex"));

/// The list in a model answer: `{"<field>": [...]}` or a bare array, after any `<think>` block
/// and around code fences or chatter.
/// Fixes closing brackets that don't match what is open (`…"}}` for `…"}]}`) and adds missing
/// ones at the end, a slip small models make. Gives up on an unterminated string (a cut-off
/// answer stays an error rather than keeping half a sentence).
fn repair_brackets(raw: &str) -> Option<String> {
    let (mut out, mut open, mut in_str, mut escaped) = (String::with_capacity(raw.len() + 8), Vec::new(), false, false);
    for c in raw.chars() {
        if in_str {
            out.push(c);
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => open.push('}'),
            '[' => open.push(']'),
            '}' | ']' => {
                // Close whatever is still open inside until the matching bracket.
                while let Some(&want) = open.last() {
                    if want == c {
                        break;
                    }
                    out.push(want);
                    open.pop();
                }
                if open.pop().is_none() {
                    continue;
                }
            }
            _ => {}
        }
        out.push(c);
    }
    if in_str {
        return None;
    }
    out.extend(open.into_iter().rev());
    Some(out)
}

pub(crate) fn json_list(answer: &str, field: &str) -> Result<Vec<Value>> {
    let answer = THINK.replace_all(answer, "");
    let invalid = || Error::Llm("Die Antwort des Modells war kein gültiges JSON".into());
    let v: Value = match (answer.find(['{', '[']), answer.rfind(['}', ']'])) {
        (Some(a), Some(b)) if a < b => {
            let raw = &answer[a..=b];
            serde_json::from_str(raw).or_else(|_| repair_brackets(raw).ok_or(()).and_then(|r| serde_json::from_str(&r).map_err(|_| ()))).map_err(|_| invalid())?
        }
        _ => return Err(invalid()),
    };
    match v {
        Value::Array(a) => Ok(a),
        Value::Object(mut o) => match o.remove(field) {
            Some(Value::Array(a)) => Ok(a),
            _ => Err(invalid()),
        },
        _ => Err(invalid()),
    }
}

/// A citation number as models write it: `3`, `"3"`, `"S3"`, `"[S3]"` (`prefix` = `S`).
pub(crate) fn citation_number(v: &Value, prefix: char) -> Option<usize> {
    let n = match v {
        Value::Number(n) => n.as_u64(),
        Value::String(t) => t.trim().trim_matches(['[', ']']).trim_start_matches([prefix, prefix.to_ascii_lowercase()]).parse().ok(),
        _ => None,
    }?;
    Some(n as usize)
}

#[derive(Deserialize)]
struct RawFact {
    #[serde(default)]
    section: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    segments: Vec<Value>,
    #[serde(default)]
    bericht: Vec<Value>,
}

/// Checks a model answer for the chunk `range`. `tags`: the Bericht section keys a fact may be
/// tagged with (unknown tags are left out, the fact stays). Fails only if the answer has no JSON.
pub fn parse(answer: &str, range: &Range<usize>, sections: &[&str], tags: &[&str], map: &TokenMap) -> Result<Parsed> {
    let list = json_list(answer, "facts")?;

    let mut out = Parsed::default();
    for item in list {
        let Ok(raw) = serde_json::from_value::<RawFact>(item) else {
            out.dropped += 1;
            continue;
        };
        match check(raw, range, sections, tags, map) {
            Some(f) if !out.facts.iter().any(|g| g.section == f.section && g.text == f.text) => out.facts.push(f),
            Some(_) => {}
            None => out.dropped += 1,
        }
    }
    Ok(out)
}

fn check(raw: RawFact, range: &Range<usize>, sections: &[&str], tags: &[&str], map: &TokenMap) -> Option<DraftFact> {
    let section = raw.section.trim().to_string();
    if !sections.contains(&section.as_str()) {
        return None;
    }
    let text = raw.text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() || text.chars().count() > MAX_FACT_CHARS || ICD.is_match(&text) {
        return None;
    }
    let mut segments = Vec::new();
    for s in raw.segments {
        // "S3", "[S3]", "3" and 3 all mean segment 3.
        let n = citation_number(&s, 'S')?;
        if n == 0 || !range.contains(&(n - 1)) {
            return None;
        }
        if !segments.contains(&(n - 1)) {
            segments.push(n - 1);
        }
    }
    if segments.is_empty() {
        return None;
    }
    segments.sort_unstable();
    let restored = map.restore(&text);
    if !restored.unknown.is_empty() {
        return None;
    }
    let mut bericht: Vec<String> = Vec::new();
    for t in &raw.bericht {
        let Some(key) = t.as_str().map(str::trim).filter(|k| tags.contains(k)) else { continue };
        if !bericht.iter().any(|b| b == key) && bericht.len() < MAX_TAGS {
            bericht.push(key.to_string());
        }
    }
    Some(DraftFact { section, text: restored.text, segments, bericht })
}

/// Words that say nothing about which fact it is.
const STOPWORDS: &[&str] = &[
    "aber", "auch", "auf", "aus", "bei", "das", "dass", "dem", "den", "der", "des", "die", "ein", "eine", "einem", "einen",
    "einer", "es", "für", "hat", "ihr", "ihre", "im", "in", "ist", "mit", "nach", "nicht", "noch", "seit", "sich", "sie",
    "sind", "und", "von", "vor", "war", "wie", "wird", "zu", "zum", "zur",
];

/// Lowercased word stems (first five letters, so "Schlafstörung" and "Schlafstörungen" agree),
/// without stopwords.
fn stems(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() > 1 && !STOPWORDS.contains(w))
        .map(|w| w.chars().take(5).collect())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Share of the shorter fact's stems that the other one has too.
fn overlap(a: &[String], b: &[String]) -> f64 {
    let small = a.len().min(b.len());
    if small == 0 {
        return 0.0;
    }
    a.iter().filter(|w| b.binary_search(w).is_ok()).count() as f64 / small as f64
}

/// Folds facts the model proposed twice (often once per chunk of a long session) into one, kept
/// under the longer wording with all citations and tags. Deterministic, no model call: same
/// note section and nearly the same words, or a looser match that cites the same segments.
/// Returns the facts in their first order and how many were folded away.
pub fn merge(facts: Vec<DraftFact>) -> (Vec<DraftFact>, usize) {
    let mut out: Vec<(DraftFact, Vec<String>)> = Vec::new();
    let mut merged = 0;
    for f in facts {
        let s = stems(&f.text);
        let twin = out.iter().position(|(g, gs)| {
            let o = overlap(&s, gs);
            g.section == f.section
                && s.len().min(gs.len()) >= 2
                && (o >= 0.8 || (o >= 0.7 && g.segments.iter().any(|x| f.segments.contains(x))))
        });
        let Some(i) = twin else {
            out.push((f, s));
            continue;
        };
        merged += 1;
        let (g, gs) = &mut out[i];
        if f.text.chars().count() > g.text.chars().count() {
            g.text = f.text;
            *gs = s;
        }
        for x in f.segments {
            if !g.segments.contains(&x) {
                g.segments.push(x);
            }
        }
        g.segments.sort_unstable();
        for t in f.bericht {
            if !g.bericht.contains(&t) && g.bericht.len() < MAX_TAGS {
                g.bericht.push(t);
            }
        }
    }
    (out.into_iter().map(|(f, _)| f).collect(), merged)
}

#[cfg(test)]
mod tests {
    #[test]
    fn mismatched_brackets_are_repaired() {
        use super::json_list;
        let list = |a: &str| json_list(a, "sentences").map(|l| l.len());
        assert_eq!(list(r#"{"sentences": [{"text": "a } ] b", "facts": [1]}}"#).unwrap(), 1);
        assert_eq!(list(r#"{"sentences": [{"text": "a", "facts": [1]}, {"text": "b"]}"#).unwrap(), 2);
        assert_eq!(list(r#"{"sentences": [{"text": "a", "facts": [1]}"#).unwrap(), 1);
        assert!(list(r#"{"sentences": [{"text": "abgeschnit}"#).is_err());
    }

    use super::*;
    use crate::domain::{AgeGroup, Method};
    use crate::templates::{self, Context};

    fn note() -> Template {
        let t = templates::builtin().unwrap().into_iter().find(|t| t.key == templates::SESSION_NOTE_KEY).unwrap();
        t.resolve(&Context { method: Method::Vt, age_group: AgeGroup::Adult, kind: None })
    }

    fn bericht() -> Template {
        let t = templates::builtin().unwrap().into_iter().find(|t| t.key == TAG_TEMPLATE_KEY).unwrap();
        t.resolve(&Context { method: Method::Vt, age_group: AgeGroup::Adult, kind: Some(crate::domain::ApplicationKind::Erstantrag) })
    }

    fn seg(role: Speaker, start_ms: i64, text: &str) -> SourceSegment {
        let kind = if role == Speaker::Therapist { SourceKind::Dictation } else { SourceKind::SessionAudio };
        SourceSegment { segment_id: String::new(), source_id: "src".into(), kind, role, start_ms, end_ms: start_ms + 4000, text: text.into() }
    }

    #[test]
    fn session_lines_name_the_speaker() {
        let mut s = seg(Speaker::Patient, 62_000, "Ich schlafe schlecht.");
        assert_eq!(line(0, &s), "[S1] (Sitzung, 01:02) P: Ich schlafe schlecht.");
        s.role = Speaker::Therapist;
        assert_eq!(line(1, &s), "[S2] (Sitzung, 01:02) T: Ich schlafe schlecht.");
        s.role = Speaker::Unknown;
        assert_eq!(line(2, &s), "[S3] (Sitzung, 01:02) Ich schlafe schlecht.");
        s.kind = SourceKind::Dictation;
        s.role = Speaker::Therapist;
        assert_eq!(line(3, &s), "[S4] (Diktat, 01:02) Ich schlafe schlecht.");
    }

    fn dict() -> Dictionary {
        let mut d = Dictionary::new();
        d.add(EntityKind::Person, "Beispiel");
        d
    }

    #[test]
    fn prompt_names_sections_and_has_no_tokens() {
        let b = bericht();
        let p = system_prompt(&note(), &tag_sections(&b));
        for key in ["verlauf", "befund", "interventionen", "risiko", "vereinbarungen"] {
            assert!(p.contains(&format!("\"{key}\"")), "{key}");
        }
        assert!(p.contains("Verhaltensanalyse"), "VT hint resolved");
        assert!(!p.contains("Übertragung"), "TP hint dropped");
        for key in ["soziodemographie", "symptomatik_befund", "somatik", "lebensgeschichte_modell", "behandlungsplan_prognose"] {
            assert!(p.contains(&format!("- \"{key}\": ")), "tag {key}");
        }
        assert!(!p.contains("\"diagnose\"") && !p.contains("\"umwandlung\""), "no tags for diagnoses or the Umwandlung");
        assert!(p.contains("\"bericht\": [\"symptomatik_befund\"]"));
        let mut ps = Pseudonymizer::new(Dictionary::new());
        assert!(ps.pseudonymize(&p).is_ok(), "the prompt itself must pass the pseudonymizer");
        assert!(!system_prompt(&note(), &[]).contains("bericht"), "no tags asked without sections");
    }

    #[test]
    fn chunks_respect_the_budget() {
        let segs: Vec<_> = (0..10).map(|i| seg(Speaker::Unknown, i * 1000, &"x".repeat(76))).collect();
        assert_eq!(chunks(&segs, 1000), vec![0..10]);
        assert_eq!(chunks(&segs, 300), vec![0..3, 3..6, 6..9, 9..10]);
        let big = vec![seg(Speaker::Unknown, 0, &"y".repeat(500)), seg(Speaker::Unknown, 1, "z")];
        assert_eq!(chunks(&big, 100), vec![0..1, 1..2]);
        assert!(chunks(&[], 100).is_empty());
    }

    #[test]
    fn prepare_tokenizes_across_chunks_and_numbers_globally() {
        let segs = vec![
            seg(Speaker::Therapist, 0, "Frau Muster berichtet von Streit mit Herrn Beispiel."),
            seg(Speaker::Unknown, 65_000, &"Füllwort ".repeat(30)),
            seg(Speaker::Unknown, 3_725_000, "Muster will am 4.3.2025 wieder arbeiten."),
        ];
        let p = prepare(&note(), &[], &segs, dict(), 400).unwrap();
        assert_eq!(p.chunks.len(), 2);
        let all: String = p.chunks.iter().map(|c| c.user.get().as_str()).collect();
        assert!(!all.contains("Muster") && !all.contains("Beispiel") && !all.contains("4.3.2025"), "{all}");
        assert!(all.contains("[S1] (Diktat, 00:00)"));
        assert!(all.contains("[S3] (Sitzung, 1:02:05) [PERSON_"), "name found by rule in chunk 1 is tokenized in chunk 2: {all}");
        assert_eq!(p.chunks[1].range, 2..3);
        let again = prepare(&note(), &[], &segs, dict(), 400).unwrap();
        assert_eq!(again.digest, p.digest, "same input, same outgoing text");
        let mut d = dict();
        d.add(EntityKind::Ort, "Streit");
        assert_ne!(prepare(&note(), &[], &segs, d, 400).unwrap().digest, p.digest);
        assert!(prepare(&note(), &[], &[], dict(), 400).is_err());
    }

    #[test]
    fn parse_keeps_valid_facts_and_restores_tokens() {
        let segs = vec![seg(Speaker::Unknown, 0, "Herr Beispiel schläft schlecht."), seg(Speaker::Unknown, 1, "Hausaufgabe: Schlaftagebuch.")];
        let p = prepare(&note(), &[], &segs, dict(), CHUNK_CHARS).unwrap();
        let template = note();
        let sections: Vec<&str> = target_sections(&template).iter().map(|s| s.key.as_str()).collect();
        let answer = r#"<think>hmm</think>```json
        {"facts": [
          {"section": "verlauf", "text": "[PERSON_1]  berichtet\nSchlafstörungen", "segments": [1]},
          {"section": "verlauf", "text": "[PERSON_1] berichtet Schlafstörungen", "segments": ["S1"]},
          {"section": "vereinbarungen", "text": "Schlaftagebuch", "segments": ["[S2]", 2, 1], "bericht": ["behandlungsplan_prognose", "diagnose", 7, "behandlungsplan_prognose", "somatik", "symptomatik_befund", "soziodemographie"]},
          {"section": "diagnosen", "text": "x", "segments": [1]},
          {"section": "befund", "text": "Verdacht auf F32.1", "segments": [1]},
          {"section": "befund", "text": "gedrückt", "segments": [7]},
          {"section": "befund", "text": "gedrückt", "segments": []},
          {"section": "verlauf", "text": "[PERSON_9] kam mit", "segments": [1]},
          {"section": "verlauf", "text": "", "segments": [1]},
          "kaputt"
        ]}
        ```"#;
        let tags = ["behandlungsplan_prognose", "somatik", "symptomatik_befund", "soziodemographie"];
        let out = parse(answer, &p.chunks[0].range, &sections, &tags, &p.map).unwrap();
        assert_eq!(
            out.facts,
            vec![
                DraftFact { section: "verlauf".into(), text: "Beispiel berichtet Schlafstörungen".into(), segments: vec![0], bericht: vec![] },
                DraftFact {
                    section: "vereinbarungen".into(),
                    text: "Schlaftagebuch".into(),
                    segments: vec![0, 1],
                    bericht: vec!["behandlungsplan_prognose".into(), "somatik".into(), "symptomatik_befund".into()],
                },
            ]
        );
        assert_eq!(out.dropped, 7);
        assert!(parse("[]", &(0..2), &sections, &tags, &p.map).unwrap().facts.is_empty());
        assert!(parse("Ich kann das nicht.", &(0..2), &sections, &tags, &p.map).is_err());
        assert!(parse(r#"{"answer": 1}"#, &(0..2), &sections, &tags, &p.map).is_err());
    }

    #[test]
    fn citations_outside_the_chunk_are_dropped() {
        let out = parse(r#"[{"section":"verlauf","text":"a","segments":[2]},{"section":"verlauf","text":"b","segments":[4]}]"#, &(3..5), &["verlauf"], &[], &TokenMap::default()).unwrap();
        assert_eq!(out.facts.len(), 1);
        assert_eq!(out.facts[0].segments, vec![3]);
    }

    fn fact(section: &str, text: &str, segments: &[usize], bericht: &[&str]) -> DraftFact {
        DraftFact { section: section.into(), text: text.into(), segments: segments.to_vec(), bericht: bericht.iter().map(|b| b.to_string()).collect() }
    }

    #[test]
    fn repeated_facts_are_merged_with_all_citations() {
        let facts = vec![
            fact("verlauf", "Panikattacke im Bus mit Herzrasen", &[2], &["symptomatik_befund"]),
            fact("befund", "Stimmung gedrückt", &[3], &[]),
            fact("verlauf", "Schlafstörungen seit Mai", &[4], &[]),
            // Chunk 2 says it again: other words around it, other segments.
            fact("verlauf", "Im Bus eine Panikattacke mit starkem Herzrasen", &[40], &["lebensgeschichte_modell"]),
            fact("befund", "Stimmung gedrückt.", &[41], &["symptomatik_befund"]),
            // Same segment, mostly the same words: the model split one statement twice.
            fact("verlauf", "Schlafstörung seit Mai, wacht nachts auf", &[4], &[]),
            // Not the same: another place, another symptom, another section.
            fact("verlauf", "Panikattacke im Supermarkt", &[42], &[]),
            fact("verlauf", "Panikattacke im Bus mit Schwindel", &[43], &[]),
            fact("risiko", "Stimmung gedrückt", &[44], &[]),
        ];
        let (out, merged) = merge(facts);
        assert_eq!(merged, 3);
        assert_eq!(
            out,
            vec![
                fact("verlauf", "Im Bus eine Panikattacke mit starkem Herzrasen", &[2, 40], &["symptomatik_befund", "lebensgeschichte_modell"]),
                fact("befund", "Stimmung gedrückt.", &[3, 41], &["symptomatik_befund"]),
                fact("verlauf", "Schlafstörung seit Mai, wacht nachts auf", &[4], &[]),
                fact("verlauf", "Panikattacke im Supermarkt", &[42], &[]),
                fact("verlauf", "Panikattacke im Bus mit Schwindel", &[43], &[]),
                fact("risiko", "Stimmung gedrückt", &[44], &[]),
            ]
        );
        assert_eq!(merge(vec![]).1, 0);
    }

    #[test]
    fn clock_formats() {
        assert_eq!(clock(0), "00:00");
        assert_eq!(clock(754_999), "12:34");
        assert_eq!(clock(3_725_000), "1:02:05");
    }
}
