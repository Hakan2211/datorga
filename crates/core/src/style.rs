//! Her own old letters and Berichte as style exemplars (Phase 6, slice 49; PLAN-AMENDMENTS §3).
//!
//! An upload is read locally ([`crate::ingest`]), then **redacted before it is stored**: names of
//! her patients and her pseudonymization terms, the pseudonymizer's rules (dates, `Frau/Herr X`,
//! postcodes, streets, employers) and a few rules for letters (full names after `Frau/Herr`, the
//! name line of an address block, reference numbers, phone numbers and e-mail addresses). She
//! checks every mark in the review, unmarks what is not identifying, marks what was missed, and
//! may trim the text. Only the redacted text is kept, with the file's SHA-256; the file itself is
//! not.
//!
//! At draft time the newest one or two active exemplars of the document type go along as
//! "Stilbeispiele" `[S n]`: wording only, never a source (v1: whole text per type, no section
//! mapping).

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::compose::reference_spans;
use crate::pseudo::{Dictionary, EntityKind, Pseudonymizer};
use crate::{Error, Result};

/// Longest exemplar kept.
pub const MAX_CHARS: usize = 40_000;
/// Shortest exemplar worth keeping.
pub const MIN_CHARS: usize = 200;
pub const MAX_TITLE_CHARS: usize = 120;
/// Exemplars per request.
pub const PER_REQUEST: usize = 2;
/// Characters of one exemplar that go along.
pub const PROMPT_CHARS: usize = 4_500;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StyleExemplar {
    pub id: String,
    /// The document type (template key) it is an example of.
    pub template_key: String,
    pub title: String,
    /// The redacted text (the only text kept).
    pub text: String,
    /// Used in drafts (she can pause one).
    pub active: bool,
    pub source_sha256: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct StyleExemplarInput {
    pub template_key: String,
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub source_sha256: Option<String>,
}

impl StyleExemplarInput {
    pub fn clean(self) -> Result<Self> {
        let title = self.title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() {
            return Err(Error::Invalid("Bitte einen Titel angeben".into()));
        }
        if title.chars().count() > MAX_TITLE_CHARS {
            return Err(Error::Invalid(format!("Der Titel ist zu lang (höchstens {MAX_TITLE_CHARS} Zeichen)")));
        }
        let text = crate::ingest::tidy(&self.text);
        let n = text.chars().count();
        if n < MIN_CHARS {
            return Err(Error::Invalid(format!("Das Beispiel ist zu kurz für den Schreibstil (mindestens {MIN_CHARS} Zeichen)")));
        }
        if n > MAX_CHARS {
            return Err(Error::Invalid(format!("Das Beispiel ist zu lang ({n} Zeichen, höchstens {MAX_CHARS}). Bitte auf einen Brief kürzen.")));
        }
        let key = self.template_key.trim().to_string();
        if key.is_empty() {
            return Err(Error::Invalid("Bitte die Art des Schreibens wählen".into()));
        }
        Ok(Self { template_key: key, title, text, source_sha256: self.source_sha256.filter(|s| s.len() == 64) })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum MarkKind {
    Person,
    Ort,
    Arbeitgeber,
    Datum,
    /// Aktenzeichen, Versicherungsnummer and the like.
    Zeichen,
    /// Phone, fax, e-mail.
    Kontakt,
}

impl MarkKind {
    pub fn placeholder(self) -> &'static str {
        match self {
            MarkKind::Person => "[Name]",
            MarkKind::Ort => "[Ort]",
            MarkKind::Arbeitgeber => "[Arbeitgeber]",
            MarkKind::Datum => "[Datum]",
            MarkKind::Zeichen => "[Zeichen]",
            MarkKind::Kontakt => "[Kontakt]",
        }
    }
}

impl From<EntityKind> for MarkKind {
    fn from(k: EntityKind) -> Self {
        match k {
            EntityKind::Person => MarkKind::Person,
            EntityKind::Ort => MarkKind::Ort,
            EntityKind::Arbeitgeber => MarkKind::Arbeitgeber,
            EntityKind::Datum => MarkKind::Datum,
        }
    }
}

/// A span the review shows as identifying. Offsets in chars (JS string indices for German text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct StyleMark {
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub kind: MarkKind,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct StyleReview {
    pub marks: Vec<StyleMark>,
    /// The text with every mark not kept replaced by its placeholder.
    pub redacted: String,
}

// "Frau Erika Mustermann," / "Herrn Max Muster geb." : the second name too.
static FULL_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:Frau|Herrn?)\s+(?:(?:Dr|Prof)\.\s+(?:med\.\s+)?)?\p{Lu}[\p{L}'-]+\s+(\p{Lu}[\p{L}'-]+)(?:\s*(?:,|\(|geb\.|\n|$))")
        .expect("valid regex")
});
// The name line of an address block: two or three capitalized words above a street line.
static ADDRESS_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^((?:Frau|Herrn?)\s+)?(\p{Lu}[\p{L}'-]+(?:\s+\p{Lu}[\p{L}'-]+){1,2})\s*\n\p{Lu}[\p{L}-]*(?:straße|strasse|str\.|weg|platz|allee|gasse|ring|damm)\s+\d")
        .expect("valid regex")
});
static EMAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\w.+-]+@[\w-]+(?:\.[\w-]+)+").expect("valid regex"));
static PHONE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:Tel(?:efon)?|Fax|Mobil|Handy)\.?\s*:?\s*(\+?\d[\d /().-]{5,}\d)").expect("valid regex")
});

/// Words that start a sentence or a heading, never a name (address-block rule).
const NOT_NAMES: &[&str] = &["Deutsche", "Sehr", "Mit", "Betreff", "Bericht", "Befundbericht", "Landratsamt", "Versorgungsamt", "Pflegekasse", "Rentenversicherung", "Medizinischer", "Praxis"];

/// The marks in `text`: `dict` holds her patients and terms, `extra` what she marked by hand.
pub fn review(text: &str, dict: &Dictionary, extra: &[(EntityKind, String)], keep: &[String]) -> StyleReview {
    let mut dict = dict.clone();
    for (kind, t) in extra {
        dict.add(*kind, t);
    }
    for c in FULL_NAME.captures_iter(text) {
        dict.add(EntityKind::Person, &c[1]);
    }
    for c in ADDRESS_NAME.captures_iter(text) {
        let words: Vec<&str> = c[2].split_whitespace().collect();
        if c.get(1).is_some() || !words.iter().any(|w| NOT_NAMES.contains(w)) {
            for w in words {
                dict.add(EntityKind::Person, w);
            }
        }
    }
    let byte_to_char = |b: usize| text[..b].chars().count();
    let mut marks: Vec<StyleMark> = Pseudonymizer::new(dict)
        .detect(text)
        .into_iter()
        .map(|d| {
            // A street rule may take the line break after the house number: keep it outside.
            let text = d.text.trim_end().to_string();
            StyleMark { start: d.start, end: d.start + text.chars().count(), text, kind: d.kind.into() }
        })
        .collect();
    let mut more = |start: usize, end: usize, kind: MarkKind| {
        marks.push(StyleMark { start: byte_to_char(start), end: byte_to_char(end), text: text[start..end].to_string(), kind });
    };
    for m in EMAIL.find_iter(text) {
        more(m.start(), m.end(), MarkKind::Kontakt);
    }
    for c in PHONE.captures_iter(text) {
        let m = c.get(1).expect("group 1");
        more(m.start(), m.end(), MarkKind::Kontakt);
    }
    // Labelled reference numbers, found as in a request (compose::redact_references).
    for (start, end) in reference_spans(text) {
        more(start, end, MarkKind::Zeichen);
    }
    marks.sort_by_key(|m| (m.start, std::cmp::Reverse(m.end)));
    let mut kept: Vec<StyleMark> = Vec::new();
    for m in marks {
        if kept.last().is_none_or(|k| m.start >= k.end) {
            kept.push(m);
        }
    }
    let redacted = redact(text, &kept, keep);
    StyleReview { marks: kept, redacted }
}

/// `text` with each mark replaced by its placeholder, except marks whose text she kept.
pub fn redact(text: &str, marks: &[StyleMark], keep: &[String]) -> String {
    let keep: Vec<String> = keep.iter().map(|k| k.trim().to_lowercase()).collect();
    let chars: Vec<char> = text.chars().collect();
    let (mut out, mut last) = (String::with_capacity(text.len()), 0);
    for m in marks {
        if m.start < last || m.end > chars.len() || keep.contains(&m.text.trim().to_lowercase()) {
            continue;
        }
        out.extend(&chars[last..m.start]);
        out.push_str(m.kind.placeholder());
        last = m.end;
    }
    out.extend(&chars[last..]);
    out
}

/// The part of an exemplar that goes along: whole if short, else cut at a paragraph (or sentence)
/// before [`PROMPT_CHARS`].
pub fn excerpt(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= PROMPT_CHARS {
        return text.to_string();
    }
    let cut = text.char_indices().nth(PROMPT_CHARS).map(|(b, _)| b).unwrap_or(text.len());
    let head = &text[..cut];
    let at = head.rfind("\n\n").filter(|&i| i > cut / 2).or_else(|| head.rfind(". ").map(|i| i + 1)).unwrap_or(cut);
    format!("{}\n[…]", head[..at].trim_end())
}

/// The exemplars that go along with a request: active ones of the type, newest first.
pub fn for_request(all: &[StyleExemplar], template_key: &str) -> Vec<(String, String)> {
    let mut fit: Vec<&StyleExemplar> = all.iter().filter(|e| e.active && e.template_key == template_key).collect();
    fit.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    fit.into_iter().take(PER_REQUEST).map(|e| (e.title.clone(), excerpt(&e.text))).collect()
}

/// The rules for using exemplars (part of the system prompt when some go along).
pub const PROMPT_RULES: &str = "Unter \"Stilbeispiele\" [S1] … stehen frühere Schreiben der Therapeutin an andere \
     Empfänger über andere Personen. Sie zeigen nur, wie sie schreibt: Aufbau, Ton, Satzlänge, Fachbegriffe und \
     typische Wendungen. Schreibe in diesem Stil, aber übernimm keinen Inhalt daraus: keine Fakten, Beschwerden, \
     Diagnosen, Daten oder Namen. Ein Stilbeispiel ist nie eine Quelle. Angaben in eckigen Klammern wie [Name] oder \
     [Datum] sind dort geschwärzt.";

/// The exemplars as a block for the prompt (empty without any).
pub fn prompt_block(exemplars: &[(String, String)]) -> String {
    if exemplars.is_empty() {
        return String::new();
    }
    let mut out = String::from("Stilbeispiele (nur Stil, keine Inhalte, keine Quellen):\n");
    for (i, (title, text)) in exemplars.iter().enumerate() {
        out.push_str(&format!("[S{}] {}\n\"\"\"\n{}\n\"\"\"\n", i + 1, title.trim(), text.trim()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Gender, Patient};

    fn patient() -> Patient {
        Patient {
            id: "p".into(),
            external_ref: None,
            first_name: "Anna".into(),
            last_name: "Beispiel".into(),
            birth_date: Some("1985-03-14".into()),
            chiffre: None,
            pronouns: None,
            gender: Some(Gender::W),
            consent_recording: false,
            status: "active".into(),
            created_at: String::new(),
            archived_at: None,
            contact: Default::default(),
        }
    }

    const LETTER: &str = "Praxis für Psychotherapie\nTel.: 07071 / 123456 · praxis@example.de\n\nDeutsche Rentenversicherung\nRuhrstraße 2\n10709 Berlin\n\nVersicherungsnummer: 12 140385 B 123\n\nBeurteilung psychischer Erkrankung von Frau Erika Mustermann, geb. 14.03.1985\n\nSehr geehrte Damen und Herren,\nFrau Mustermann befindet sich seit dem 02.05.2019 in meiner Behandlung. Ihre Schwester Jonas begleitet sie. Auch Frau Beispiel kennt sie.\nMit freundlichen Grüßen";

    #[test]
    fn review_marks_names_dates_references_and_contacts() {
        let dict = Dictionary::from_patient(&patient());
        let r = review(LETTER, &dict, &[(EntityKind::Person, "Jonas".into())], &[]);
        let kinds = |k: MarkKind| r.marks.iter().filter(|m| m.kind == k).map(|m| m.text.as_str()).collect::<Vec<_>>();
        assert_eq!(kinds(MarkKind::Person), ["Erika", "Mustermann", "Mustermann", "Jonas", "Beispiel"]);
        assert_eq!(kinds(MarkKind::Datum), ["14.03.1985", "02.05.2019"]);
        assert_eq!(kinds(MarkKind::Zeichen), ["12 140385 B 123"]);
        assert_eq!(kinds(MarkKind::Kontakt), ["07071 / 123456", "praxis@example.de"]);
        assert!(kinds(MarkKind::Ort).contains(&"10709 Berlin") && kinds(MarkKind::Ort).contains(&"Ruhrstraße 2"));
        for secret in ["Erika", "Mustermann", "Jonas", "Beispiel", "14.03.1985", "140385", "123456", "praxis@", "Ruhrstraße"] {
            assert!(!r.redacted.contains(secret), "{secret} in {}", r.redacted);
        }
        assert!(r.redacted.contains("von Frau [Name] [Name], geb. [Datum]") && r.redacted.contains("Versicherungsnummer: [Zeichen]"), "{}", r.redacted);
        assert!(r.redacted.contains("Deutsche Rentenversicherung"), "an institution is not a name");
        // Offsets are chars: the marks cut the text where they say.
        let chars: Vec<char> = LETTER.chars().collect();
        for m in &r.marks {
            assert_eq!(chars[m.start..m.end].iter().collect::<String>(), m.text);
        }
    }

    #[test]
    fn kept_marks_stay_and_address_names_are_found() {
        let text = "Max Muster\nHauptstraße 5\n72070 Tübingen\n\nSehr geehrter Herr Muster, die Behandlung läuft.";
        let r = review(text, &Dictionary::new(), &[], &["72070 Tübingen".into()]);
        assert!(r.redacted.starts_with("[Name] [Name]\n[Ort]\n72070 Tübingen"), "{}", r.redacted);
        assert!(r.redacted.contains("Herr [Name],"));
        let institution = "Deutsche Rentenversicherung Bund\nRuhrstraße 2\n10709 Berlin";
        assert!(review(institution, &Dictionary::new(), &[], &[]).redacted.starts_with("Deutsche Rentenversicherung Bund\n"));
    }

    #[test]
    fn input_is_checked() {
        let ok = StyleExemplarInput { template_key: "drv-befund".into(), title: "  DRV  2023 ".into(), text: "Wort ".repeat(60), source_sha256: Some("x".into()) };
        let c = ok.clone().clean().unwrap();
        assert_eq!(c.title, "DRV 2023");
        assert_eq!(c.source_sha256, None, "not a SHA-256");
        assert!(StyleExemplarInput { text: "zu kurz".into(), ..ok.clone() }.clean().is_err());
        assert!(StyleExemplarInput { title: " ".into(), ..ok.clone() }.clean().is_err());
        assert!(StyleExemplarInput { template_key: "".into(), ..ok }.clean().is_err());
    }

    #[test]
    fn excerpts_cut_at_paragraphs_and_pick_the_newest() {
        let long = format!("{}\n\n{}", "Satz eins. ".repeat(300), "Satz zwei. ".repeat(300));
        let e = excerpt(&long);
        assert!(e.ends_with("\n[…]") && e.chars().count() <= PROMPT_CHARS + 4 && !e.contains("zwei"), "{}", e.len());
        assert_eq!(excerpt(" kurz "), "kurz");
        let ex = |id: &str, key: &str, at: &str, active: bool| StyleExemplar {
            id: id.into(),
            template_key: key.into(),
            title: id.into(),
            text: id.into(),
            active,
            source_sha256: None,
            created_at: at.into(),
            updated_at: at.into(),
        };
        let all = vec![ex("a", "drv-befund", "1", true), ex("b", "drv-befund", "3", true), ex("c", "drv-befund", "2", true), ex("d", "drv-befund", "4", false), ex("e", "arztbrief", "5", true)];
        let picked: Vec<String> = for_request(&all, "drv-befund").into_iter().map(|(t, _)| t).collect();
        assert_eq!(picked, ["b", "c"]);
        let block = prompt_block(&for_request(&all, "drv-befund"));
        assert!(block.starts_with("Stilbeispiele") && block.contains("[S1] b\n\"\"\"\nb\n\"\"\"") && block.contains("[S2] c"));
        assert_eq!(prompt_block(&[]), "");
    }
}
