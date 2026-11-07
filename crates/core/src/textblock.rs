//! Textbausteine (Phase 6, slice 48): paragraphs the therapist reuses nearly word for word.
//!
//! A block is rich text (TipTap HTML) with placeholders (`{Name}`, `{Frau/Herr}`, …, see
//! [`crate::letter::PLACEHOLDERS`]) that are filled from the patient record when it is inserted
//! into a letter. A block fits some letter types (or all) and may name the section it is meant
//! for; the insert menu offers those first, ordered by use.
//!
//! The built-in set (`templates/textbausteine.yaml`) is our own generic wording. Editing a
//! built-in block makes it hers (origin `user`, never overwritten); deleting one hides it.
//!
//! "Mit KI schreiben" sends the letter type's blocks as "bevorzugte Formulierungen" `[T n]`: the
//! model may take their wording where the material backs the content. Citing only a block does
//! not back a sentence (a block is wording, not a fact about the patient).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::Gender;
use crate::letter::{self, FillContext};
use crate::{Error, Result};

const BUILTIN_YAML: &str = include_str!("../../../templates/textbausteine.yaml");

/// Longest title.
pub const MAX_TITLE_CHARS: usize = 120;
/// Longest body (HTML).
pub const MAX_BODY_CHARS: usize = 20_000;
/// Most blocks sent with one letter request, and their plain-text size together.
pub const MAX_AI_BLOCKS: usize = 12;
pub const MAX_AI_CHARS: usize = 6_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "TextBlockOrigin")]
pub enum Origin {
    Builtin,
    User,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Builtin => "builtin",
            Origin::User => "user",
        }
    }
}

impl std::str::FromStr for Origin {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "builtin" => Ok(Origin::Builtin),
            "user" => Ok(Origin::User),
            _ => Err(Error::Invalid(format!("unknown text block origin {s}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TextBlock {
    pub id: String,
    pub title: String,
    /// TipTap HTML with placeholders.
    pub body: String,
    /// Letter types it fits; empty = all.
    pub template_keys: Vec<String>,
    /// The section it is meant for (offered first there).
    pub section_key: Option<String>,
    pub origin: Origin,
    #[ts(type = "number")]
    pub use_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl TextBlock {
    /// Fits a letter of this type.
    pub fn fits(&self, template_key: &str) -> bool {
        self.template_keys.is_empty() || self.template_keys.iter().any(|k| k == template_key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TextBlockInput {
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub template_keys: Vec<String>,
    #[serde(default)]
    pub section_key: Option<String>,
}

impl TextBlockInput {
    /// Trimmed and checked; an empty body (no text) is refused.
    pub fn clean(self) -> Result<Self> {
        let title = self.title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() {
            return Err(Error::Invalid("Der Textbaustein braucht einen Titel".into()));
        }
        if title.chars().count() > MAX_TITLE_CHARS {
            return Err(Error::Invalid(format!("Der Titel ist zu lang (höchstens {MAX_TITLE_CHARS} Zeichen)")));
        }
        let body = self.body.trim().to_string();
        if crate::draft::plain_text(&body).trim().is_empty() && !body.contains("<li") {
            return Err(Error::Invalid("Der Textbaustein ist leer".into()));
        }
        if body.chars().count() > MAX_BODY_CHARS {
            return Err(Error::Invalid(format!("Der Textbaustein ist zu lang (höchstens {MAX_BODY_CHARS} Zeichen)")));
        }
        let mut template_keys: Vec<String> =
            self.template_keys.into_iter().map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect();
        template_keys.sort();
        template_keys.dedup();
        let section_key = self.section_key.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        Ok(Self { title, body, template_keys, section_key })
    }
}

/// A built-in block as written in the YAML.
#[derive(Debug, Clone, Deserialize)]
pub struct Builtin {
    pub id: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub templates: Vec<String>,
    #[serde(default)]
    pub section: Option<String>,
}

pub fn builtin() -> Result<Vec<Builtin>> {
    serde_yaml_ng::from_str(BUILTIN_YAML).map_err(|e| Error::Invalid(format!("textbausteine.yaml: {e}")))
}

/// The block's HTML with its placeholders filled for this patient (values HTML-escaped).
/// What the record doesn't have stays as `{…}` for her to fill in (the checklist warns).
pub fn fill_html(body: &str, ctx: &FillContext<'_>) -> String {
    letter::fill_with(body, ctx, |v| v.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;"))
}

/// The block's plain text as the model sees it: the patient is "Pat.", gender pairs follow the
/// record ("die Patientin"), and identifying placeholders (birth date, numbers, diagnoses) are
/// left out, as is anything the record doesn't fill.
pub fn for_ai(body: &str, gender: Option<Gender>) -> String {
    let text = crate::draft::plain_text(body);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find(['}', '{', '\n']) {
            Some(end) if after.as_bytes()[end] == b'}' => {
                let token = after[..end].trim();
                let value = if let Some((f, m)) = token.split_once('/') {
                    match gender {
                        Some(Gender::W) => f.trim().to_string(),
                        Some(Gender::M) => m.trim().to_string(),
                        _ => format!("{}/{}", f.trim(), m.trim()),
                    }
                } else {
                    match token {
                        "Name" | "Nachname" | "Vorname" => "Pat.".to_string(),
                        "Anrede" => String::new(),
                        _ => "…".to_string(),
                    }
                };
                out.push_str(&value);
                rest = &after[end + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    // "Frau Pat." / "Herr Pat." → "Pat."; doubled spaces from an empty {Anrede}.
    let out = out.replace("Frau Pat.", "Pat.").replace("Herr Pat.", "Pat.");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The blocks that go along with a letter request as preferred wording: those fitting the type,
/// meant for no section or for one being written; type-specific and most used first, capped.
pub fn for_request<'a>(blocks: &'a [TextBlock], template_key: &str, sections: &[String]) -> Vec<&'a TextBlock> {
    let mut fit: Vec<&TextBlock> = blocks
        .iter()
        .filter(|b| b.fits(template_key) && b.section_key.as_ref().is_none_or(|s| sections.contains(s)))
        .collect();
    fit.sort_by_key(|b| (b.template_keys.is_empty(), -b.use_count));
    let mut chars = 0;
    fit.into_iter()
        .take(MAX_AI_BLOCKS)
        .take_while(|b| {
            chars += crate::draft::plain_text(&b.body).chars().count();
            chars <= MAX_AI_CHARS
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Patient;

    fn patient(gender: Option<Gender>) -> Patient {
        Patient {
            id: "p".into(),
            external_ref: None,
            first_name: "Lena".into(),
            last_name: "Berg".into(),
            birth_date: Some("1990-03-04".into()),
            chiffre: None,
            pronouns: None,
            gender,
            consent_recording: false,
            status: "active".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            archived_at: None,
            contact: Default::default(),
        }
    }

    #[test]
    fn builtins_parse_with_unique_ids_and_known_types() {
        let all = builtin().unwrap();
        assert!(all.len() >= 10);
        let mut ids: Vec<&str> = all.iter().map(|b| b.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all.len());
        let keys: Vec<String> = crate::templates::builtin().unwrap().into_iter().map(|t| t.key).collect();
        for b in &all {
            assert!(b.id.starts_with("builtin-"), "{}", b.id);
            for t in &b.templates {
                assert!(keys.contains(t), "{}: unknown type {t}", b.id);
            }
            let input = TextBlockInput { title: b.title.clone(), body: b.body.clone(), template_keys: b.templates.clone(), section_key: b.section.clone() };
            input.clean().unwrap();
            // Every placeholder is one the resolver knows or a gender pair.
            let filled = fill_html(&b.body, &FillContext { patient: &patient(Some(Gender::W)), episode: None });
            if let Some(open) = letter::open_placeholder(&filled) {
                assert_eq!(open, "{Behandlung seit}", "{}: {open}", b.id);
            }
        }
    }

    #[test]
    fn fill_html_escapes_and_follows_gender() {
        let mut p = patient(Some(Gender::M));
        p.last_name = "Berg & Söhne".into();
        let html = fill_html("<p>{Frau/Herr} {Nachname}, {seine/ihre}… {die Patientin/den Patienten}</p>", &FillContext { patient: &p, episode: None });
        // Pairs are written {weiblich/männlich}; "{seine/ihre}" would be wrong order, so it is taken literally.
        assert_eq!(html, "<p>Herr Berg &amp; Söhne, ihre… den Patienten</p>");
        let open = fill_html("<p>{Krankenkasse}</p>", &FillContext { patient: &p, episode: None });
        assert_eq!(open, "<p>{Krankenkasse}</p>");
    }

    #[test]
    fn for_ai_hides_identity() {
        let body = "<p>{Frau/Herr} {Nachname}, geb. {Geburtsdatum}, ist seit dem {Behandlung seit} bei mir; {Diagnosen}. {Die Patientin/Der Patient} kommt gern.</p>";
        let t = for_ai(body, Some(Gender::W));
        assert_eq!(t, "Pat., geb. …, ist seit dem … bei mir; …. Die Patientin kommt gern.");
        assert!(!t.contains('{'));
        let d = for_ai("<p>{Die Patientin/Der Patient}</p>", None);
        assert_eq!(d, "Die Patientin/Der Patient");
    }

    #[test]
    fn request_blocks_fit_type_and_section() {
        let b = |id: &str, keys: &[&str], section: Option<&str>, uses: i64| TextBlock {
            id: id.into(),
            title: id.into(),
            body: "<p>Text</p>".into(),
            template_keys: keys.iter().map(|k| k.to_string()).collect(),
            section_key: section.map(str::to_string),
            origin: Origin::User,
            use_count: uses,
            created_at: String::new(),
            updated_at: String::new(),
        };
        let all = vec![
            b("all", &[], None, 9),
            b("drv", &["drv-befund"], None, 1),
            b("drv-befund-sec", &["drv-befund"], Some("befund"), 5),
            b("pflege", &["pflege-stellungnahme"], None, 50),
        ];
        let ids = |v: Vec<&TextBlock>| v.into_iter().map(|b| b.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(for_request(&all, "drv-befund", &["anlass".into()])), ["drv", "all"]);
        assert_eq!(ids(for_request(&all, "drv-befund", &["befund".into()])), ["drv-befund-sec", "drv", "all"]);
    }
}
