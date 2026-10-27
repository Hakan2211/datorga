//! Per-session fact extraction (Phase 2, slice 13): the confirmed transcript of a session becomes
//! short facts per Sitzungsnotiz section, each citing the transcript segments it came from.
//!
//! The model sees numbered, pseudonymized segments (`[S12] …`) and answers with JSON. Its answer
//! is checked, never trusted: unknown sections, citations outside the chunk, invented tokens and
//! ICD codes (diagnoses stay with the therapist) are dropped. Long sessions are split into chunks
//! so a 16k-context local model can take them.

use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::domain::{SourceSegment, Speaker};
use crate::pseudo::{DetectionSource, Dictionary, EntityKind, Pseudonymized, Pseudonymizer, TokenEntry, TokenMap};
use crate::templates::{FieldKind, Template};
use crate::{Error, Result};

/// Bump on any change to the prompt or the answer format.
pub const PROMPT_VERSION: &str = "extract-2026.1";

/// Transcript characters per request. German runs at roughly 3–4 characters per token, so this
/// leaves room for the prompt and the answer within a 16k context.
pub const CHUNK_CHARS: usize = 24_000;

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

/// The system prompt for a (resolved) note template. Holds no patient data.
pub fn system_prompt(template: &Template) -> String {
    let mut sections = String::new();
    for s in target_sections(template) {
        sections.push_str(&format!("- \"{}\": {}\n", s.key, s.heading));
        for h in &s.hints {
            sections.push_str(&format!("    · {}\n", h.text.trim()));
        }
    }
    format!(
        "Du unterstützt eine Psychotherapeutin bei der Verlaufsdokumentation einer Sitzung.\n\
         Du erhältst die Abschrift der Sitzung als nummerierte Abschnitte ([S1], [S2] …). \
         Diktate der Therapeutin sind mit \"Diktat\" markiert, Sitzungsaufnahmen mit \"Sitzung\" \
         (Sprecher dort nicht getrennt).\n\n\
         Aufgabe: Ziehe die dokumentationswürdigen Fakten heraus und ordne jeden einem dieser \
         Abschnitte der Sitzungsnotiz zu:\n{sections}\n\
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
         {{\"facts\": [{{\"section\": \"verlauf\", \"text\": \"…\", \"segments\": [3, 4]}}]}}\n"
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
    let kind = if s.role == Speaker::Therapist { "Diktat" } else { "Sitzung" };
    format!("[S{}] ({kind}, {}) {}", i + 1, clock(s.start_ms), s.text.trim())
}

/// `mm:ss` (or `h:mm:ss`) from the start of the recording.
pub fn clock(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

/// Pseudonymizes the whole session with one token map. A name that the rules find anywhere
/// ("Frau X") is tokenized in every chunk, not just the one it appeared in.
pub fn prepare(template: &Template, segments: &[SourceSegment], mut dict: Dictionary, budget: usize) -> Result<Prepared> {
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
    let system_raw = system_prompt(template);
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

pub(crate) fn sha256(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

static THINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<think>.*?</think>").expect("valid regex"));
// ICD-10 codes (F32.1, F43, Z73.0…). "U" codes are left out: they are rare and "U18" is no diagnosis talk.
pub(crate) static ICD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-TV-Z][0-9]{2}(\.[0-9]{1,2})?\b").expect("valid regex"));

/// The list in a model answer: `{"<field>": [...]}` or a bare array, after any `<think>` block
/// and around code fences or chatter.
pub(crate) fn json_list(answer: &str, field: &str) -> Result<Vec<Value>> {
    let answer = THINK.replace_all(answer, "");
    let invalid = || Error::Llm("Die Antwort des Modells war kein gültiges JSON".into());
    let v: Value = match (answer.find(['{', '[']), answer.rfind(['}', ']'])) {
        (Some(a), Some(b)) if a < b => serde_json::from_str(&answer[a..=b]).map_err(|_| invalid())?,
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
}

/// Checks a model answer for the chunk `range`. Fails only if the answer has no JSON at all.
pub fn parse(answer: &str, range: &Range<usize>, sections: &[&str], map: &TokenMap) -> Result<Parsed> {
    let list = json_list(answer, "facts")?;

    let mut out = Parsed::default();
    for item in list {
        let Ok(raw) = serde_json::from_value::<RawFact>(item) else {
            out.dropped += 1;
            continue;
        };
        match check(raw, range, sections, map) {
            Some(f) if !out.facts.iter().any(|g| g.section == f.section && g.text == f.text) => out.facts.push(f),
            Some(_) => {}
            None => out.dropped += 1,
        }
    }
    Ok(out)
}

fn check(raw: RawFact, range: &Range<usize>, sections: &[&str], map: &TokenMap) -> Option<DraftFact> {
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
    Some(DraftFact { section, text: restored.text, segments })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgeGroup, Method};
    use crate::templates::{self, Context};

    fn note() -> Template {
        let t = templates::builtin().unwrap().into_iter().find(|t| t.key == templates::SESSION_NOTE_KEY).unwrap();
        t.resolve(&Context { method: Method::Vt, age_group: AgeGroup::Adult, kind: None })
    }

    fn seg(role: Speaker, start_ms: i64, text: &str) -> SourceSegment {
        SourceSegment { segment_id: String::new(), source_id: "src".into(), role, start_ms, end_ms: start_ms + 4000, text: text.into() }
    }

    fn dict() -> Dictionary {
        let mut d = Dictionary::new();
        d.add(EntityKind::Person, "Beispiel");
        d
    }

    #[test]
    fn prompt_names_sections_and_has_no_tokens() {
        let p = system_prompt(&note());
        for key in ["verlauf", "befund", "interventionen", "risiko", "vereinbarungen"] {
            assert!(p.contains(&format!("\"{key}\"")), "{key}");
        }
        assert!(p.contains("Verhaltensanalyse"), "VT hint resolved");
        assert!(!p.contains("Übertragung"), "TP hint dropped");
        let mut ps = Pseudonymizer::new(Dictionary::new());
        assert!(ps.pseudonymize(&p).is_ok(), "the prompt itself must pass the pseudonymizer");
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
        let p = prepare(&note(), &segs, dict(), 400).unwrap();
        assert_eq!(p.chunks.len(), 2);
        let all: String = p.chunks.iter().map(|c| c.user.get().as_str()).collect();
        assert!(!all.contains("Muster") && !all.contains("Beispiel") && !all.contains("4.3.2025"), "{all}");
        assert!(all.contains("[S1] (Diktat, 00:00)"));
        assert!(all.contains("[S3] (Sitzung, 1:02:05) [PERSON_"), "name found by rule in chunk 1 is tokenized in chunk 2: {all}");
        assert_eq!(p.chunks[1].range, 2..3);
        let again = prepare(&note(), &segs, dict(), 400).unwrap();
        assert_eq!(again.digest, p.digest, "same input, same outgoing text");
        let mut d = dict();
        d.add(EntityKind::Ort, "Streit");
        assert_ne!(prepare(&note(), &segs, d, 400).unwrap().digest, p.digest);
        assert!(prepare(&note(), &[], dict(), 400).is_err());
    }

    #[test]
    fn parse_keeps_valid_facts_and_restores_tokens() {
        let segs = vec![seg(Speaker::Unknown, 0, "Herr Beispiel schläft schlecht."), seg(Speaker::Unknown, 1, "Hausaufgabe: Schlaftagebuch.")];
        let p = prepare(&note(), &segs, dict(), CHUNK_CHARS).unwrap();
        let template = note();
        let sections: Vec<&str> = target_sections(&template).iter().map(|s| s.key.as_str()).collect();
        let answer = r#"<think>hmm</think>```json
        {"facts": [
          {"section": "verlauf", "text": "[PERSON_1]  berichtet\nSchlafstörungen", "segments": [1]},
          {"section": "verlauf", "text": "[PERSON_1] berichtet Schlafstörungen", "segments": ["S1"]},
          {"section": "vereinbarungen", "text": "Schlaftagebuch", "segments": ["[S2]", 2, 1]},
          {"section": "diagnosen", "text": "x", "segments": [1]},
          {"section": "befund", "text": "Verdacht auf F32.1", "segments": [1]},
          {"section": "befund", "text": "gedrückt", "segments": [7]},
          {"section": "befund", "text": "gedrückt", "segments": []},
          {"section": "verlauf", "text": "[PERSON_9] kam mit", "segments": [1]},
          {"section": "verlauf", "text": "", "segments": [1]},
          "kaputt"
        ]}
        ```"#;
        let out = parse(answer, &p.chunks[0].range, &sections, &p.map).unwrap();
        assert_eq!(
            out.facts,
            vec![
                DraftFact { section: "verlauf".into(), text: "Beispiel berichtet Schlafstörungen".into(), segments: vec![0] },
                DraftFact { section: "vereinbarungen".into(), text: "Schlaftagebuch".into(), segments: vec![0, 1] },
            ]
        );
        assert_eq!(out.dropped, 7);
        assert!(parse("[]", &(0..2), &sections, &p.map).unwrap().facts.is_empty());
        assert!(parse("Ich kann das nicht.", &(0..2), &sections, &p.map).is_err());
        assert!(parse(r#"{"answer": 1}"#, &(0..2), &sections, &p.map).is_err());
    }

    #[test]
    fn citations_outside_the_chunk_are_dropped() {
        let out = parse(r#"[{"section":"verlauf","text":"a","segments":[2]},{"section":"verlauf","text":"b","segments":[4]}]"#, &(3..5), &["verlauf"], &TokenMap::default()).unwrap();
        assert_eq!(out.facts.len(), 1);
        assert_eq!(out.facts[0].segments, vec![3]);
    }

    #[test]
    fn clock_formats() {
        assert_eq!(clock(0), "00:00");
        assert_eq!(clock(754_999), "12:34");
        assert_eq!(clock(3_725_000), "1:02:05");
    }
}
