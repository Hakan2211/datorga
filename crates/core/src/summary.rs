//! Session summary (Phase 4, slice 30): 3–6 key points per session for the patient timeline,
//! each citing the transcript segments (`[S3]`) or note sections (`[N1]`) it rests on.
//!
//! Built like the fact extraction: the confirmed transcript and, if there is one, the
//! Sitzungsnotiz are pseudonymized with one token map and split into chunks; the answer is
//! checked, never trusted. A point without a citation, with an ICD code or a dose, with a token
//! the map doesn't know, or stating prognosis/motivation/goals that nothing it cites states
//! ([`crate::draft`]'s grounding check) is dropped. The patient is written "Pat.".

use std::ops::Range;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::domain::{Patient, Report, SourceSegment};
use crate::draft::{patient_as_pat, plain_text, unsupported};
use crate::extract::{self, json_list, sha256, Prepared, PreparedChunk, ICD};
use crate::llm::{ChatOptions, Effort};
use crate::pseudo::{DetectionSource, Dictionary, EntityKind, Pseudonymizer, TokenMap};
use crate::templates::Template;
use crate::{Error, Result};

/// Bump on any change to the prompt or the answer format.
pub const PROMPT_VERSION: &str = "summary-2026.1";

/// Most key points per session (the prompt asks for 3 to this many).
pub const MAX_POINTS: usize = 6;

/// Longest point kept; a key point is a line, not a paragraph.
pub const MAX_POINT_CHARS: usize = 300;

/// Request options for one summary chunk: a short answer, low effort.
pub fn chat_options() -> ChatOptions {
    ChatOptions { temperature: 0.1, max_tokens: 4096, timeout: Duration::from_secs(600), reasoning: Some(Effort::Low) }
}

/// A key point as proposed by the model, tokens restored, citations as 0-based indices into the
/// segments and the note sections the run saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftPoint {
    pub text: String,
    pub segments: Vec<usize>,
    pub note: Vec<usize>,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub points: Vec<DraftPoint>,
    /// Entries that failed a check (counted for the audit log, contents discarded).
    pub dropped: usize,
}

/// A note section the summary may read: its key, heading and plain text.
#[derive(Debug, Clone)]
pub struct NoteSection {
    pub key: String,
    pub heading: String,
    pub text: String,
}

/// The note's sections that go in: prose sections that allow AI text (never Diagnosen), not empty.
pub fn note_sections(template: &Template, note: &Report) -> Vec<NoteSection> {
    extract::target_sections(template)
        .into_iter()
        .filter_map(|def| {
            let body = note.sections.iter().find(|s| s.key == def.key)?;
            let text = plain_text(&body.body);
            (!text.is_empty()).then(|| NoteSection { key: def.key.clone(), heading: def.heading.trim().to_string(), text })
        })
        .collect()
}

/// Points asked per chunk, so a long session still ends with at most [`MAX_POINTS`].
pub fn quota(chunks: usize) -> usize {
    (MAX_POINTS / chunks.max(1)).max(1)
}

/// The system prompt for a chunk that may hold `quota` points. Holds no patient data.
pub fn system_prompt(quota: usize) -> String {
    let range = if quota >= 3 { format!("3 bis {quota}") } else { format!("höchstens {quota}") };
    format!(
        "Du unterstützt eine Psychotherapeutin. Fasse eine Therapiesitzung in Kernpunkten für ihren \
         Behandlungsverlauf zusammen.\n\
         Du erhältst die Abschrift der Sitzung als nummerierte Abschnitte ([S1], [S2] …) und, falls es \
         schon eine gibt, die Sitzungsnotiz der Therapeutin ([N1], [N2] …). Diktate der Therapeutin sind \
         mit \"Diktat\" markiert, Sitzungsaufnahmen mit \"Sitzung\"; \"T:\" spricht die Therapeutin, \
         \"P:\" Pat.\n\n\
         Aufgabe: {range} Kernpunkte: worum es ging, was sich verändert hat, was vereinbart wurde.\n\
         Regeln:\n\
         - Nur was in der Abschrift oder der Notiz steht. Nichts ergänzen, nicht deuten, keine Prognose.\n\
         - Jeder Kernpunkt nennt die Nummern, auf die er sich stützt (mindestens eine, S oder N).\n\
         - Ein Satz pro Punkt, sachlich, auf Deutsch, Patient:in als \"Pat.\".\n\
         - Keine Diagnosen, keine ICD-Codes, keine Medikamente oder Dosierungen.\n\
         - Platzhalter in eckigen Klammern wie [PERSON_n] unverändert übernehmen, keine neuen erfinden.\n\
         - Gibt der Text nichts Relevantes her, antworte mit leerer Liste.\n\n\
         Antworte ausschließlich mit JSON in dieser Form, ohne weiteren Text:\n\
         {{\"points\": [{{\"text\": \"…\", \"sources\": [\"S3\", \"N1\"]}}]}}\n"
    )
}

/// Pseudonymizes transcript and note with one token map. The note goes with the first chunk.
pub fn prepare(patient: &Patient, segments: &[SourceSegment], note: &[NoteSection], mut dict: Dictionary, budget: usize) -> Result<Prepared> {
    if segments.is_empty() && note.is_empty() {
        return Err(Error::Invalid("Keine bestätigte Abschrift und keine Notiz für diese Sitzung".into()));
    }
    let as_pat: Vec<SourceSegment> =
        segments.iter().map(|s| SourceSegment { text: patient_as_pat(&s.text, patient), ..s.clone() }).collect();
    let note_lines: Vec<String> =
        note.iter().enumerate().map(|(i, n)| format!("[N{}] ({}) {}", i + 1, n.heading, patient_as_pat(&n.text, patient).replace('\n', " / "))).collect();
    let all: String = as_pat.iter().map(|s| s.text.as_str()).chain(note_lines.iter().map(String::as_str)).collect::<Vec<_>>().join("\n");
    for d in Pseudonymizer::new(dict.clone()).detect(&all) {
        if d.source == DetectionSource::Rule && d.kind == EntityKind::Person {
            dict.add(EntityKind::Person, &d.text);
        }
    }
    let mut ranges = extract::chunks(&as_pat, budget);
    if ranges.is_empty() {
        ranges.push(0..0);
    }
    let system_raw = system_prompt(quota(ranges.len()));
    let prompt_hash = sha256(&format!("{PROMPT_VERSION}\n{system_raw}"));
    let mut p = Pseudonymizer::new(dict);
    let system = p.pseudonymize(&system_raw)?;
    let mut prepared = Vec::new();
    for (k, range) in ranges.into_iter().enumerate() {
        let mut text = String::new();
        if k == 0 && !note_lines.is_empty() {
            text.push_str(&format!("Sitzungsnotiz:\n\n{}\n\n", note_lines.join("\n")));
        }
        if !range.is_empty() {
            let lines: Vec<String> = range.clone().map(|i| extract::line(i, &as_pat[i])).collect();
            text.push_str(&format!("Abschrift:\n\n{}", lines.join("\n")));
        }
        let user = p.pseudonymize(text.trim_end())?;
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

// Doses and dosing talk: a point about medication is the prescriber's, not the summary's.
static DOSE: LazyLock<Regex> = LazyLock::new(|| {
    regex::RegexBuilder::new(r"\b\d+([.,]\d+)?\s?(mg|µg|ug|ml|tropfen|tbl)\b|dosis|dosier|aufdosier|eindosier")
        .case_insensitive(true)
        .build()
        .expect("valid regex")
});

#[derive(Deserialize)]
struct RawPoint {
    #[serde(default)]
    text: String,
    #[serde(default, alias = "segments", alias = "quellen")]
    sources: Vec<Value>,
}

/// What a chunk's answer may cite: its segments and, for the first chunk, the note sections.
pub struct Scope<'a> {
    pub range: &'a Range<usize>,
    /// Note sections shown in this chunk (0 for every chunk but the first).
    pub notes: usize,
    /// All segments and note sections, for the grounding check.
    pub segments: &'a [SourceSegment],
    pub note: &'a [NoteSection],
}

/// Checks a model answer for one chunk, keeping at most `quota` points. Fails only if the answer
/// has no JSON.
pub fn parse(answer: &str, scope: &Scope, quota: usize, map: &TokenMap, patient: &Patient) -> Result<Parsed> {
    let mut out = Parsed::default();
    for item in json_list(answer, "points")? {
        let Ok(raw) = serde_json::from_value::<RawPoint>(item) else {
            out.dropped += 1;
            continue;
        };
        match check(raw, scope, map, patient) {
            Some(p) if out.points.iter().any(|q| q.text == p.text) => {}
            Some(p) if out.points.len() < quota => out.points.push(p),
            _ => out.dropped += 1,
        }
    }
    Ok(out)
}

/// `"S3"`, `"[N1]"`, `3` (a bare number is a segment) → which list and the 0-based index.
fn source(v: &Value) -> Option<(char, usize)> {
    let (kind, n) = match v {
        Value::Number(n) => ('S', n.as_u64()? as usize),
        Value::String(t) => {
            let t = t.trim().trim_matches(['[', ']']).trim();
            let kind = t.chars().next()?.to_ascii_uppercase();
            match kind {
                'S' | 'N' => (kind, t[1..].trim().parse().ok()?),
                _ => ('S', t.parse().ok()?),
            }
        }
        _ => return None,
    };
    n.checked_sub(1).map(|i| (kind, i))
}

fn check(raw: RawPoint, scope: &Scope, map: &TokenMap, patient: &Patient) -> Option<DraftPoint> {
    let text = raw.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = text.trim_start_matches(['-', '•', '*', ' ']).to_string();
    if text.is_empty() || text.chars().count() > MAX_POINT_CHARS || ICD.is_match(&text) || DOSE.is_match(&text) {
        return None;
    }
    let (mut segments, mut note) = (Vec::new(), Vec::new());
    for v in &raw.sources {
        match source(v)? {
            ('S', i) if scope.range.contains(&i) => segments.push(i),
            ('N', i) if i < scope.notes => note.push(i),
            _ => return None,
        }
    }
    segments.sort_unstable();
    segments.dedup();
    note.sort_unstable();
    note.dedup();
    if segments.is_empty() && note.is_empty() {
        return None;
    }
    let restored = map.restore(&text);
    if !restored.unknown.is_empty() {
        return None;
    }
    let cited: Vec<&str> = segments
        .iter()
        .filter_map(|&i| scope.segments.get(i).map(|s| s.text.as_str()))
        .chain(note.iter().filter_map(|&i| scope.note.get(i).map(|n| n.text.as_str())))
        .collect();
    if unsupported(&restored.text, &cited) {
        return None;
    }
    Some(DraftPoint { text: patient_as_pat(&restored.text, patient), segments, note })
}

/// A stored key point: citations by segment id and note section key, so they survive a re-run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SummaryPoint {
    pub text: String,
    #[serde(default)]
    pub segments: Vec<String>,
    #[serde(default)]
    pub note: Vec<String>,
}

/// A run's checked points as stored: citations by segment id and note section key.
pub fn to_stored(points: &[DraftPoint], segments: &[SourceSegment], note: &[NoteSection]) -> Vec<SummaryPoint> {
    points
        .iter()
        .map(|p| SummaryPoint {
            text: p.text.clone(),
            segments: p.segments.iter().filter_map(|&i| segments.get(i).map(|s| s.segment_id.clone())).collect(),
            note: p.note.iter().filter_map(|&i| note.get(i).map(|n| n.key.clone())).collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{SourceKind, Speaker};

    fn patient() -> Patient {
        serde_json::from_value(serde_json::json!({
            "id": "p1", "first_name": "Anna", "last_name": "Beispiel", "birth_date": "1990-03-04",
            "chiffre": "B040390", "pronouns": null, "consent_recording": true, "status": "active",
            "created_at": "2026-01-01T00:00:00Z", "gender": "w"
        }))
        .unwrap()
    }

    fn seg(i: usize, role: Speaker, text: &str) -> SourceSegment {
        SourceSegment {
            segment_id: format!("seg{i}"),
            source_id: "src".into(),
            kind: SourceKind::SessionAudio,
            role,
            start_ms: i as i64 * 5000,
            end_ms: i as i64 * 5000 + 4000,
            text: text.into(),
        }
    }

    fn segs() -> Vec<SourceSegment> {
        vec![
            seg(0, Speaker::Patient, "Frau Beispiel sagt, ich schlafe seit zwei Wochen besser."),
            seg(1, Speaker::Patient, "Mit meiner Schwester Karla gab es wieder Streit."),
            seg(2, Speaker::Therapist, "Wir vereinbaren, dass Sie das Schlaftagebuch weiterführen."),
        ]
    }

    fn note() -> Vec<NoteSection> {
        vec![NoteSection { key: "verlauf".into(), heading: "Verlauf".into(), text: "Pat. wirkt entlastet.".into() }]
    }

    fn dict() -> Dictionary {
        let mut d = Dictionary::from_patient(&patient());
        d.add(EntityKind::Person, "Karla");
        d
    }

    #[test]
    fn prompt_passes_the_pseudonymizer_and_asks_for_the_quota() {
        let p = system_prompt(6);
        assert!(p.contains("3 bis 6 Kernpunkte") && p.contains("ICD") && p.contains("\"sources\""));
        assert!(system_prompt(2).contains("höchstens 2"));
        assert!(Pseudonymizer::new(Dictionary::new()).pseudonymize(&p).is_ok());
        assert_eq!((quota(1), quota(2), quota(4), quota(9)), (6, 3, 1, 1));
    }

    #[test]
    fn prepare_writes_pat_tokenizes_others_and_puts_the_note_first() {
        let p = prepare(&patient(), &segs(), &note(), dict(), extract::CHUNK_CHARS).unwrap();
        assert_eq!(p.chunks.len(), 1);
        let m = p.chunks[0].user.get();
        assert!(m.starts_with("Sitzungsnotiz:\n\n[N1] (Verlauf) Pat. wirkt entlastet."), "{m}");
        assert!(m.contains("[S1] (Sitzung, 00:00) P: Pat. sagt"), "{m}");
        assert!(!m.contains("Beispiel") && !m.contains("Karla") && m.contains("[PERSON_"), "{m}");
        // A dictation-only or note-only session works too; nothing at all does not.
        assert!(prepare(&patient(), &[], &note(), dict(), 100).unwrap().chunks[0].user.get().starts_with("Sitzungsnotiz:"));
        assert!(prepare(&patient(), &[], &[], dict(), 100).is_err());
        let long = prepare(&patient(), &segs(), &note(), dict(), 60).unwrap();
        assert_eq!(long.chunks.len(), 3);
        assert!(long.system.get().contains("höchstens 2"));
        assert!(!long.chunks[1].user.get().contains("[N1]"), "the note goes with the first chunk only");
    }

    #[test]
    fn parse_keeps_cited_points_and_drops_the_rest() {
        let (s, n) = (segs(), note());
        let p = prepare(&patient(), &s, &n, dict(), extract::CHUNK_CHARS).unwrap();
        let karla = p.map.entries().iter().find(|e| e.original == "Karla").unwrap().token.clone();
        let answer = format!(
            r#"<think>…</think>{{"points": [
              {{"text": "Pat. schläft seit zwei Wochen besser.", "sources": ["S1", "N1"]}},
              {{"text": "Erneut Streit mit der Schwester {karla}.", "sources": [2]}},
              {{"text": "Erneut Streit mit der Schwester {karla}.", "sources": ["[S2]"]}},
              {{"text": "- Schlaftagebuch wird weitergeführt.", "segments": ["S3"]}},
              {{"text": "Ohne Quelle.", "sources": []}},
              {{"text": "Quelle außerhalb.", "sources": ["S9"]}},
              {{"text": "Verdacht auf F32.1.", "sources": ["S1"]}},
              {{"text": "Sertralin auf 50 mg erhöhen.", "sources": ["S1"]}},
              {{"text": "Prognose günstig.", "sources": ["S1"]}},
              {{"text": "[PERSON_9] kam mit.", "sources": ["S1"]}},
              "kaputt"
            ]}}"#
        );
        let scope = Scope { range: &p.chunks[0].range, notes: n.len(), segments: &s, note: &n };
        let out = parse(&answer, &scope, 6, &p.map, &patient()).unwrap();
        assert_eq!(
            out.points,
            vec![
                DraftPoint { text: "Pat. schläft seit zwei Wochen besser.".into(), segments: vec![0], note: vec![0] },
                DraftPoint { text: "Erneut Streit mit der Schwester Karla.".into(), segments: vec![1], note: vec![] },
                DraftPoint { text: "Schlaftagebuch wird weitergeführt.".into(), segments: vec![2], note: vec![] },
            ]
        );
        assert_eq!(out.dropped, 7);
        assert_eq!(parse(&answer, &scope, 2, &p.map, &patient()).unwrap().points.len(), 2, "quota");
        let no_note = Scope { notes: 0, ..scope };
        assert_eq!(parse(&answer, &no_note, 6, &p.map, &patient()).unwrap().points.len(), 2, "N1 not shown in this chunk");
        assert!(parse("Kann ich nicht.", &no_note, 6, &p.map, &patient()).is_err());
        let stored = to_stored(&out.points, &s, &n);
        assert_eq!(stored[0], SummaryPoint { text: out.points[0].text.clone(), segments: vec!["seg0".into()], note: vec!["verlauf".into()] });
    }

    #[test]
    fn a_grounded_claim_needs_a_source_that_says_so() {
        let s = vec![seg(0, Speaker::Patient, "Ich bin sehr motiviert, weiterzumachen.")];
        let p = prepare(&patient(), &s, &[], Dictionary::new(), extract::CHUNK_CHARS).unwrap();
        let scope = Scope { range: &p.chunks[0].range, notes: 0, segments: &s, note: &[] };
        let out = parse(r#"{"points":[{"text":"Pat. ist motiviert.","sources":["S1"]}]}"#, &scope, 6, &p.map, &patient()).unwrap();
        assert_eq!(out.points.len(), 1);
    }
}
