//! Local pseudonymizer: identifying spans become tokens (`[PERSON_1]`, `[ORT_2]`…) before any
//! text leaves the machine, and are restored in the response.
//!
//! The only way to get a [`Pseudonymized`] value is [`Pseudonymizer::pseudonymize`], so a cloud
//! provider that takes `Pseudonymized<T>` can't be handed raw text:
//!
//! ```compile_fail
//! let raw = thera_core::pseudo::Pseudonymized("Frau Beispiel".to_string());
//! ```
//!
//! Sources, longest match wins on overlap:
//! - the patient record (names, birth date variants),
//! - therapist-confirmed dictionary entries (places, employers, relatives…),
//! - rules: dates with a day, `Frau/Herr X`, company suffixes (`GmbH`, `AG`…), `Firma X`,
//!   postcode + city, street + house number. Names found by rule are then matched everywhere.
//!
//! The [`TokenMap`] holds the raw identifiers. It stays local (encrypted store only).

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::Patient;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Person,
    Ort,
    Arbeitgeber,
    Datum,
}

impl EntityKind {
    pub fn label(self) -> &'static str {
        match self {
            EntityKind::Person => "PERSON",
            EntityKind::Ort => "ORT",
            EntityKind::Arbeitgeber => "ARBEITGEBER",
            EntityKind::Datum => "DATUM",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum DetectionSource {
    Patient,
    Dictionary,
    Rule,
}

/// A span to be tokenized. Offsets are in chars (equal to JS string indices for German text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Detection {
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub kind: EntityKind,
    pub source: DetectionSource,
}

/// Known identifiers, matched whole-word and case-insensitively (a trailing genitive `s` stays
/// outside the token, so "Beispiels" becomes "[PERSON_1]s"). Each entry's regex is built once:
/// the assistant's dictionary holds every patient and is matched against many texts.
#[derive(Debug, Clone, Default)]
pub struct Dictionary {
    entries: Vec<(EntityKind, String, DetectionSource, Regex)>,
}

impl Dictionary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_patient(p: &Patient) -> Self {
        let mut d = Self::new();
        d.push(EntityKind::Person, &p.last_name, DetectionSource::Patient);
        if p.first_name.trim().chars().count() >= 2 {
            d.push(EntityKind::Person, &p.first_name, DetectionSource::Patient);
        }
        if let Some(b) = &p.birth_date {
            if let [y, m, day] = b.split('-').collect::<Vec<_>>()[..] {
                let (mi, di) = (m.trim_start_matches('0'), day.trim_start_matches('0'));
                for v in [b.clone(), format!("{day}.{m}.{y}"), format!("{di}.{mi}.{y}")] {
                    d.push(EntityKind::Datum, &v, DetectionSource::Patient);
                }
            }
        }
        d
    }

    /// Therapist-confirmed entity.
    pub fn add(&mut self, kind: EntityKind, text: &str) {
        self.push(kind, text, DetectionSource::Dictionary);
    }

    /// Everything another dictionary knows (the assistant's covers all patients).
    pub fn extend(&mut self, other: &Dictionary) {
        for (kind, text, source, re) in &other.entries {
            if !self.contains(text) {
                self.entries.push((*kind, text.clone(), *source, re.clone()));
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn contains(&self, text: &str) -> bool {
        let text = text.to_lowercase();
        self.entries.iter().any(|(_, t, _, _)| t.to_lowercase() == text)
    }

    fn push(&mut self, kind: EntityKind, text: &str, source: DetectionSource) {
        let text = text.trim();
        if !text.is_empty() && !self.contains(text) {
            let words: Vec<_> = text.split_whitespace().map(regex::escape).collect();
            let re = Regex::new(&format!(r"(?i)\b({})s?\b", words.join(r"\s+"))).expect("escaped literal");
            self.entries.push((kind, text.to_string(), source, re));
        }
    }

    fn find(&self, text: &str, out: &mut Vec<Span>) {
        for (kind, _, source, re) in &self.entries {
            for c in re.captures_iter(text) {
                let m = c.get(1).expect("group 1");
                out.push(Span { start: m.start(), end: m.end(), kind: *kind, source: *source });
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
    kind: EntityKind,
    source: DetectionSource,
}

const MONTHS: &str = "Januar|Februar|März|Maerz|April|Mai|Juni|Juli|August|September|Oktober|November|Dezember|Jan|Feb|Mär|Apr|Jun|Jul|Aug|Sept|Sep|Okt|Nov|Dez";

/// (regex, capture group holding the entity, kind)
static RULES: LazyLock<Vec<(Regex, usize, EntityKind)>> = LazyLock::new(|| {
    let r = |p: &str| Regex::new(p).expect("valid rule");
    vec![
        // 04.03.1985, 4.3.85, 4. 3. 1985 (no yearless dates: "4.3." is a section number)
        (r(r"\b(\d{1,2}\.\s?\d{1,2}\.\s?(?:\d{4}|\d{2}))\b"), 1, EntityKind::Datum),
        (r(r"\b(\d{4}-\d{2}-\d{2})\b"), 1, EntityKind::Datum),
        // 4. März 1985, 4. Mai
        (r(&format!(r"\b(\d{{1,2}}\.\s?(?:{MONTHS})\b\.?(?:\s\d{{4}}\b)?)")), 1, EntityKind::Datum),
        (r(r"\b(?:Frau|Herrn?|Fr\.|Hr\.)\s+(?:(?:Dr|Prof)\.\s+)?(\p{Lu}[\p{L}'-]+)"), 1, EntityKind::Person),
        (
            r(r"\b((?:\p{Lu}[\p{L}&.-]*\s+){0,3}\p{Lu}[\p{L}&.-]*\s+(?:(?:gGmbH|GmbH|AG|KG|OHG|SE|UG)\b(?:\s?&\s?Co\.\s?KG\b)?|e\.\s?V\.))"),
            1,
            EntityKind::Arbeitgeber,
        ),
        (r(r"\bFirma\s+(\p{Lu}[\p{L}&-]*)"), 1, EntityKind::Arbeitgeber),
        (r(r"\b(\d{5}\s+\p{Lu}[\p{L}-]+)"), 1, EntityKind::Ort),
        (
            r(r"\b(\p{Lu}[\p{L}-]*(?:straße|strasse|str\.|weg|platz|allee|gasse|ring|damm)\s+\d+\s?[a-z]?)\b"),
            1,
            EntityKind::Ort,
        ),
    ]
});

static TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(PERSON|ORT|ARBEITGEBER|DATUM)_(\d+)\]").expect("valid token regex"));

/// Token-shaped text that isn't from a map ("[PERSON_1]" typed by hand, or invented by a model)
/// put in round brackets, so [`Pseudonymizer::pseudonymize`] takes it and nothing reads it as a
/// token.
pub fn defuse_tokens(text: &str) -> std::borrow::Cow<'_, str> {
    TOKEN.replace_all(text, |c: &regex::Captures| format!("({}_{})", &c[1], &c[2]))
}

fn valid_date(s: &str) -> bool {
    let nums: Vec<u32> = s.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse().ok()).collect();
    match nums[..] {
        [_, m, d] if s.contains('-') => (1..=12).contains(&m) && (1..=31).contains(&d),
        [d, m, _] => (1..=31).contains(&d) && (1..=12).contains(&m),
        [d, ..] => (1..=31).contains(&d), // "4. März 2024"
        [] => false,
    }
}

/// Text that has been through the pseudonymizer. Constructible only by [`Pseudonymizer`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Pseudonymized<T>(T);

static TOKEN_RUN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(PERSON|ORT|ARBEITGEBER|DATUM)_\d+\](?:\s+\[(?:PERSON|ORT|ARBEITGEBER|DATUM)_\d+\])*").expect("valid token run regex")
});

impl Pseudonymized<String> {
    /// For reading aloud: a placeholder, or a run of them ("[PERSON_1] [PERSON_2]"), becomes one
    /// neutral word, so the voice doesn't spell out "PERSON 1". Taking tokens out can't add
    /// anything identifying.
    pub fn speakable(&self) -> Pseudonymized<String> {
        Pseudonymized(
            TOKEN_RUN
                .replace_all(&self.0, |c: &regex::Captures| match &c[1] {
                    "PERSON" => "Person",
                    "ORT" => "Ort",
                    "ARBEITGEBER" => "Arbeitgeber",
                    _ => "Datum",
                })
                .into_owned(),
        )
    }
}

impl<T> Pseudonymized<T> {
    pub fn get(&self) -> &T {
        &self.0
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenEntry {
    pub token: String,
    pub kind: EntityKind,
    pub original: String,
}

/// Token ↔ original surface text. Same surface text always gets the same token.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenMap {
    entries: Vec<TokenEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restored {
    pub text: String,
    /// Token-shaped strings in the input that aren't in the map (e.g. invented by the model).
    pub unknown: Vec<String>,
}

impl TokenMap {
    pub fn entries(&self) -> &[TokenEntry] {
        &self.entries
    }

    fn token_for(&mut self, kind: EntityKind, original: &str) -> String {
        if let Some(e) = self.entries.iter().find(|e| e.kind == kind && e.original == original) {
            return e.token.clone();
        }
        let n = self.entries.iter().filter(|e| e.kind == kind).count() + 1;
        let token = format!("[{}_{n}]", kind.label());
        self.entries.push(TokenEntry { token: token.clone(), kind, original: original.to_string() });
        token
    }

    pub fn restore(&self, text: &str) -> Restored {
        let mut unknown = Vec::new();
        let out = TOKEN.replace_all(text, |c: &regex::Captures| {
            let tok = &c[0];
            match self.entries.iter().find(|e| e.token == tok) {
                Some(e) => e.original.clone(),
                None => {
                    unknown.push(tok.to_string());
                    tok.to_string()
                }
            }
        });
        Restored { text: out.into_owned(), unknown }
    }
}

/// Holds the dictionary and a token map shared across all texts of one request, so the same
/// person keeps the same token in every segment.
#[derive(Debug, Clone)]
pub struct Pseudonymizer {
    dict: Dictionary,
    map: TokenMap,
}

impl Pseudonymizer {
    pub fn new(dict: Dictionary) -> Self {
        Self { dict, map: TokenMap::default() }
    }

    pub fn with_map(dict: Dictionary, map: TokenMap) -> Self {
        Self { dict, map }
    }

    pub fn map(&self) -> &TokenMap {
        &self.map
    }

    pub fn into_map(self) -> TokenMap {
        self.map
    }

    fn spans(&self, text: &str) -> Vec<Span> {
        let mut rules = Vec::new();
        for (re, group, kind) in RULES.iter() {
            for c in re.captures_iter(text) {
                let m = c.get(*group).expect("rule group");
                if *kind == EntityKind::Datum && !valid_date(m.as_str()) {
                    continue;
                }
                let mut end = m.end();
                // "Frau Beispiels" → known stem + genitive s.
                if *kind == EntityKind::Person && m.as_str().ends_with('s') && self.dict.contains(&m.as_str()[..m.len() - 1]) {
                    end -= 1;
                }
                rules.push(Span { start: m.start(), end, kind: *kind, source: DetectionSource::Rule });
            }
        }
        // A name seen once as "Frau X" is identifying everywhere in the text.
        let mut dict = self.dict.clone();
        for s in rules.iter().filter(|s| s.kind == EntityKind::Person) {
            dict.push(EntityKind::Person, &text[s.start..s.end], DetectionSource::Rule);
        }
        // Dictionary first so it wins ties (stable sort).
        let mut spans = Vec::new();
        dict.find(text, &mut spans);
        spans.extend(rules);

        spans.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
        let mut kept: Vec<Span> = Vec::new();
        for s in spans {
            if kept.last().is_none_or(|k| s.start >= k.end) {
                kept.push(s);
            }
        }
        kept
    }

    /// Spans that would be tokenized, for the redaction review.
    pub fn detect(&self, text: &str) -> Vec<Detection> {
        let chars = |b: usize| text[..b].chars().count();
        self.spans(text)
            .into_iter()
            .map(|s| Detection {
                start: chars(s.start),
                end: chars(s.end),
                text: text[s.start..s.end].to_string(),
                kind: s.kind,
                source: s.source,
            })
            .collect()
    }

    pub fn pseudonymize(&mut self, text: &str) -> Result<Pseudonymized<String>> {
        if let Some(m) = TOKEN.find(text) {
            return Err(Error::Invalid(format!("Text enthält bereits ein Token: {}", m.as_str())));
        }
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        for s in self.spans(text) {
            out.push_str(&text[last..s.start]);
            out.push_str(&self.map.token_for(s.kind, &text[s.start..s.end]));
            last = s.end;
        }
        out.push_str(&text[last..]);

        // Fail closed if a known identifier survived (e.g. lost in an overlap).
        let mut residual = Vec::new();
        self.dict.find(&out, &mut residual);
        if let Some(s) = residual.first() {
            return Err(Error::Invalid(format!("Pseudonymisierung unvollständig: {}", &out[s.start..s.end])));
        }
        Ok(Pseudonymized(out))
    }

    pub fn pseudonymize_all<S: AsRef<str>>(&mut self, texts: &[S]) -> Result<Pseudonymized<Vec<String>>> {
        texts.iter().map(|t| self.pseudonymize(t.as_ref()).map(Pseudonymized::into_inner)).collect::<Result<_>>().map(Pseudonymized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speakable_text_has_no_placeholders() {
        let mut p = Pseudonymizer::new(Dictionary::from_patient(&patient()));
        let t = p.pseudonymize("Termin mit Anna Beispiel in 12345 Musterstadt.").unwrap();
        assert_eq!(t.speakable().get(), "Termin mit Person in Ort.");
    }

    fn patient() -> Patient {
        Patient {
            id: "p".into(),
            external_ref: None,
            first_name: "Anna".into(),
            last_name: "Beispiel".into(),
            birth_date: Some("1985-03-04".into()),
            chiffre: Some("B040385".into()),
            pronouns: None,
            consent_recording: false,
            status: "active".into(),
            created_at: String::new(),
            archived_at: None,
        }
    }

    fn pseudonymizer() -> Pseudonymizer {
        let mut d = Dictionary::from_patient(&patient());
        d.add(EntityKind::Ort, "Gelsenkirchen");
        d.add(EntityKind::Person, "Jonas");
        Pseudonymizer::new(d)
    }

    const SAMPLE: &str = "Frau Anna Beispiel (geb. 04.03.1985) lebt seit 2019 in Gelsenkirchen, \
        Hauptstraße 12a, 45879 Gelsenkirchen. Sie arbeitet als Sachbearbeiterin bei der Müller Bau GmbH; \
        ihr Vorgesetzter Herr Dr. Krause setze sie unter Druck. Am 4. März 2024 kam es zu einem \
        Zusammenbruch, seit dem 12.4.24 ist sie krankgeschrieben. Frau Beispiels Sohn Jonas (8) \
        erlebt die Situation mit. Krause habe sich nicht gemeldet. In Abschnitt 4.3. folgt die Diagnose.";

    #[test]
    fn round_trip_restores_identical_text() {
        let mut p = pseudonymizer();
        let out = p.pseudonymize(SAMPLE).unwrap();
        let s = out.get();
        for raw in ["Anna", "Beispiel", "04.03.1985", "Gelsenkirchen", "Hauptstraße", "45879", "Müller Bau", "Krause", "März 2024", "12.4.24", "Jonas"] {
            assert!(!s.contains(raw), "{raw} leaked: {s}");
        }
        // Kept: the genitive s, non-identifying numbers and section numbers.
        assert!(s.contains("[PERSON_2]s Sohn"), "{s}");
        assert!(s.contains("seit 2019") && s.contains("(8)") && s.contains("Abschnitt 4.3."), "{s}");
        assert_eq!(p.map().restore(s).text, SAMPLE);
    }

    #[test]
    fn tokens_are_stable_across_texts() {
        let mut p = pseudonymizer();
        let out = p.pseudonymize_all(&["Frau Beispiel kommt.", "Beispiel sagt ab."]).unwrap().into_inner();
        assert_eq!(out, ["Frau [PERSON_1] kommt.", "[PERSON_1] sagt ab."]);
        // A rule hit ("Herr Krause") makes later bare "Krause" a token too.
        let s = p.pseudonymize("Krause schrieb. Herr Krause rief an.").unwrap().into_inner();
        assert_eq!(s, "[PERSON_2] schrieb. Herr [PERSON_2] rief an.");
    }

    #[test]
    fn restore_reports_unknown_tokens() {
        let mut p = pseudonymizer();
        p.pseudonymize("Frau Beispiel").unwrap();
        let r = p.map().restore("[PERSON_1] und [ORT_9]");
        assert_eq!(r.text, "Beispiel und [ORT_9]");
        assert_eq!(r.unknown, ["[ORT_9]"]);
    }

    #[test]
    fn rejects_token_shaped_input_and_bad_dates() {
        let mut p = pseudonymizer();
        assert!(p.pseudonymize("siehe [PERSON_1]").is_err());
        // 45.13.2020 isn't a date; whole words only ("Annahme" ≠ "Anna").
        assert_eq!(p.pseudonymize("Wert 45.13.2020, Annahme").unwrap().into_inner(), "Wert 45.13.2020, Annahme");
    }

    #[test]
    fn detect_uses_char_offsets() {
        let p = pseudonymizer();
        let d = p.detect("Ärger mit Frau Beispiel");
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].start, d[0].end, d[0].kind), (15, 23, EntityKind::Person));
        assert_eq!(d[0].source, DetectionSource::Patient);
    }
}
