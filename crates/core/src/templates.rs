//! Report templates are data (YAML), never code. Built-in packs are compiled in. User packs
//! (uploaded forms → template builder) use the same schema and live in the `template` table.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::{AgeGroup, ApplicationKind, Method};
use crate::{Error, Result};

const BUILTIN: &[&str] = &[
    include_str!("../../../templates/ptv3/erst-umwandlung.yaml"),
    include_str!("../../../templates/ptv3/fortfuehrung.yaml"),
    include_str!("../../../templates/notes/sitzungsnotiz.yaml"),
    include_str!("../../../templates/briefe/bescheinigung.yaml"),
    include_str!("../../../templates/briefe/freier-brief.yaml"),
    include_str!("../../../templates/briefe/drv-befund.yaml"),
    include_str!("../../../templates/briefe/pflege-stellungnahme.yaml"),
    include_str!("../../../templates/briefe/versorgungsamt-befund.yaml"),
    include_str!("../../../templates/briefe/arztbrief.yaml"),
];

/// Template of the per-session note (Verlaufsdokumentation).
pub const SESSION_NOTE_KEY: &str = "sitzungsnotiz";

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Template {
    pub key: String,
    pub version: String,
    pub title: String,
    pub source: Option<String>,
    /// Application kinds a Bericht template serves; empty for the Sitzungsnotiz.
    #[serde(default)]
    pub report_kinds: Vec<ApplicationKind>,
    /// Which list offers it: the Berichte board (`ptv3`, `bericht`) or Briefe (`bericht`, `brief`).
    #[serde(default)]
    pub category: Category,
    /// `letter`: printed as a DIN 5008 letter with letterhead, address window and Betreff.
    #[serde(default)]
    pub layout: Layout,
    /// One line under the title in the type picker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Letters: Betreff with placeholders (see [`crate::letter::fill`]), filled in locally.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Letters: reference lines under the Betreff (Versicherungsnummer, Aktenzeichen, ...).
    #[serde(default)]
    pub refs: Vec<RefDef>,
    /// Letters: the sentence before "Mit freundlichen Grüßen".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closing: Option<String>,
    /// Recipient kinds this type is recommended for in the "Neuer Brief" dialog.
    #[serde(default)]
    pub recipients: Vec<crate::domain::RecipientKind>,
    /// Letters drafted by the AI: who reads them (picks the prompt's wording). Required for the
    /// `bericht` category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience: Option<Audience>,
    /// Letters: a Schweigepflichtentbindung for the recipient is expected (checked before signing).
    #[serde(default)]
    pub release: bool,
    pub rules: Rules,
    #[serde(default)]
    pub intro: String,
    pub sections: Vec<SectionDef>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Category {
    /// Bericht an den Gutachter (PTV 3).
    #[default]
    Ptv3,
    /// Other Berichte (Rentenversicherung, Pflegekasse, Versorgungsamt, Ärzt:in), slice 46.
    Bericht,
    /// Short letters: Bescheinigung, freier Brief.
    Brief,
    /// The Sitzungsnotiz.
    Note,
}

/// Who reads a letter: the AI's wording and what it stresses follow from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Audience {
    /// Rentenversicherung, Pflegekasse / MD, Versorgungsamt: function and participation count.
    Behoerde,
    /// A treating doctor: findings, course, recommendation.
    Arzt,
    /// The patient or someone they pass it on to (Bescheinigung).
    Patient,
    /// Anyone (freier Brief).
    Frei,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Layout {
    #[default]
    Bericht,
    Letter,
}

/// A reference line under a letter's Betreff.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RefDef {
    pub label: String,
    /// Prefilled from the patient record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<PatientRef>,
    /// Missing value → warning before signing.
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PatientRef {
    InsuranceNo,
    PensionNo,
    Insurer,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Rules {
    pub target_pages: Option<u32>,
    #[serde(default)]
    pub keyword_style_allowed: bool,
    #[serde(default)]
    pub signature_required: bool,
    /// Report goes to a third party under a Chiffre: no name, address or birth date allowed.
    #[serde(default)]
    pub pseudonymized: bool,
    /// Reports of this template are numbered consecutively per episode.
    #[serde(default)]
    pub numbered: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FieldKind {
    /// Rich text (TipTap HTML). AI-draftable unless `ai_allowed: false`.
    Prose,
    /// ICD-10 list (therapist-entered only) plus free-text notes.
    Diagnoses,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SectionDef {
    pub key: String,
    pub number: Option<String>,
    pub heading: String,
    pub field: FieldKind,
    #[serde(default = "yes")]
    pub ai_allowed: bool,
    /// An empty required section blocks signing.
    #[serde(default = "yes")]
    pub required: bool,
    /// May be marked "nicht relevant" in a report (PTV 3: "Unterpunkte nur bei Relevanz").
    #[serde(default)]
    pub optional: bool,
    /// Receives the therapist's confirmed dictation when the document is created.
    #[serde(default)]
    pub dictation: bool,
    /// Letters: text the section starts with, placeholders filled from the patient record
    /// (paragraphs separated by a blank line).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starter: Option<String>,
    #[serde(default)]
    pub when: When,
    #[serde(default)]
    pub hints: Vec<Hint>,
    /// Questions the Bericht-Lotse asks when the sessions don't answer them (slice 38).
    #[serde(default)]
    pub asks: Vec<Ask>,
    /// Sections of the Vorbericht a draft of this one sees besides the one with the same key
    /// (slice 40): the earlier goals for the Verlauf of a Fortführung.
    #[serde(default)]
    pub prior: Vec<String>,
}

/// A question for the "Fragen" step. Her answer counts as her own statement in the draft.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Ask {
    pub key: String,
    pub question: String,
    /// Quick answers ("keine", "nicht bekannt"); a chip fills or extends the answer.
    #[serde(default)]
    pub chips: Vec<String>,
    /// Answered from the patient record: offered as a chip, never sent to the coverage check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<RecordField>,
    #[serde(default)]
    pub when: When,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RecordField {
    /// "41 Jahre, weiblich" from the birth date and Geschlecht.
    AgeGender,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Hint {
    pub text: String,
    #[serde(default)]
    pub when: When,
}

/// Empty list / absent = applies to all.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct When {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<Vec<Method>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age: Option<Vec<AgeGroup>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Vec<ApplicationKind>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Context {
    pub method: Method,
    pub age_group: AgeGroup,
    /// `None` outside an application (Sitzungsnotiz): kind-restricted entries are dropped.
    pub kind: Option<ApplicationKind>,
}

impl When {
    pub fn matches(&self, ctx: &Context) -> bool {
        fn ok<T: PartialEq>(list: &Option<Vec<T>>, v: &T) -> bool {
            list.as_ref().is_none_or(|l| l.is_empty() || l.contains(v))
        }
        let kind_ok = match ctx.kind {
            Some(k) => ok(&self.kind, &k),
            None => self.kind.as_ref().is_none_or(|l| l.is_empty()),
        };
        ok(&self.method, &ctx.method) && ok(&self.age, &ctx.age_group) && kind_ok
    }
}

impl Template {
    pub fn from_yaml(src: &str) -> Result<Self> {
        let t: Template = serde_yaml_ng::from_str(src).map_err(|e| Error::Template(e.to_string()))?;
        t.validate()?;
        Ok(t)
    }

    pub fn validate(&self) -> Result<()> {
        if self.key.trim().is_empty() || self.sections.is_empty() {
            return Err(Error::Template("template needs a key and at least one section".into()));
        }
        let letter = self.layout == Layout::Letter;
        if letter != matches!(self.category, Category::Bericht | Category::Brief) {
            return Err(Error::Template("letter layout goes with the categories bericht and brief".into()));
        }
        if !letter && (self.subject.is_some() || !self.refs.is_empty() || self.closing.is_some() || self.audience.is_some() || self.release) {
            return Err(Error::Template("subject, refs, closing, audience and release are for letters".into()));
        }
        if self.category == Category::Bericht && self.audience.is_none() {
            return Err(Error::Template("a bericht template needs an audience".into()));
        }
        if self.category == Category::Bericht && !self.report_kinds.is_empty() {
            return Err(Error::Template("report_kinds are for PTV 3 templates".into()));
        }
        let mut seen = std::collections::HashSet::new();
        for s in &self.sections {
            // A letter section may go without a heading (a freier Brief is just text).
            if s.heading.trim().is_empty() && !letter {
                return Err(Error::Template(format!("section {} needs a heading", s.key)));
            }
            if s.starter.is_some() && (!letter || s.field != FieldKind::Prose) {
                return Err(Error::Template(format!("section {}: starter text only in letter prose sections", s.key)));
            }
            if !seen.insert(s.key.as_str()) {
                return Err(Error::Template(format!("duplicate section key {}", s.key)));
            }
            if s.field == FieldKind::Diagnoses && s.ai_allowed {
                return Err(Error::Template(format!(
                    "section {}: diagnoses must be ai_allowed: false (MDR boundary)",
                    s.key
                )));
            }
            // No questions about diagnoses (MDR boundary); answers feed AI drafts only.
            if !s.asks.is_empty() && (s.field != FieldKind::Prose || !s.ai_allowed) {
                return Err(Error::Template(format!("section {}: questions only on AI-draftable prose sections", s.key)));
            }
            let mut asks = std::collections::HashSet::new();
            if let Some(a) = s.asks.iter().find(|a| !asks.insert(a.key.as_str()) || a.question.trim().is_empty()) {
                return Err(Error::Template(format!("section {}: question {} is empty or twice", s.key, a.key)));
            }
        }
        Ok(())
    }

    /// The template as it applies to one patient/episode/application: sections and hints filtered.
    pub fn resolve(&self, ctx: &Context) -> Template {
        let mut t = self.clone();
        t.sections.retain(|s| s.when.matches(ctx));
        for s in &mut t.sections {
            s.hints.retain(|h| h.when.matches(ctx));
            s.asks.retain(|a| a.when.matches(ctx));
        }
        t
    }
}

pub fn builtin() -> Result<Vec<Template>> {
    BUILTIN.iter().map(|s| Template::from_yaml(s)).collect()
}

pub fn template_key_for(kind: ApplicationKind) -> &'static str {
    match kind {
        ApplicationKind::Erstantrag | ApplicationKind::Umwandlung => "ptv3-erst-umwandlung",
        ApplicationKind::Fortfuehrung => "ptv3-fortfuehrung",
    }
}

fn yes() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn erst() -> Template {
        builtin().unwrap().into_iter().find(|t| t.key == "ptv3-erst-umwandlung").unwrap()
    }

    fn hints(t: &Template, key: &str) -> Vec<String> {
        t.sections.iter().find(|s| s.key == key).unwrap().hints.iter().map(|h| h.text.clone()).collect()
    }

    #[test]
    fn vt_adult_erstantrag() {
        let t = erst().resolve(&Context { method: Method::Vt, age_group: AgeGroup::Adult, kind: Some(ApplicationKind::Erstantrag) });
        let keys: Vec<_> = t.sections.iter().map(|s| s.number.clone().unwrap()).collect();
        assert_eq!(keys, ["1", "2", "3", "4", "5", "6"]);
        let h4 = hints(&t, "lebensgeschichte_modell");
        assert!(h4.iter().any(|h| h.contains("Makroanalyse")));
        assert!(!h4.iter().any(|h| h.contains("Psychodynamik")));
        assert!(hints(&t, "soziodemographie")[0].contains("Familienstand"));
        assert!(!hints(&t, "diagnose").iter().any(|h| h.contains("neurosenpsychologische")));
    }

    #[test]
    fn tp_kjp_umwandlung_has_section_7_and_bezugspersonen() {
        let t = erst().resolve(&Context { method: Method::Tp, age_group: AgeGroup::Kjp, kind: Some(ApplicationKind::Umwandlung) });
        assert_eq!(t.sections.len(), 7);
        assert!(hints(&t, "lebensgeschichte_modell").iter().any(|h| h.contains("Abwehrmechanismen")));
        assert!(hints(&t, "soziodemographie")[0].contains("Geschwisterzahl"));
        assert!(hints(&t, "diagnose").iter().any(|h| h.contains("neurosenpsychologische")));
        assert!(hints(&t, "behandlungsplan_prognose").iter().any(|h| h.contains("Einbeziehung der Bezugspersonen")));
    }

    #[test]
    fn session_note_hints_follow_method_and_age() {
        let note = builtin().unwrap().into_iter().find(|t| t.key == SESSION_NOTE_KEY).unwrap();
        let vt = note.resolve(&Context { method: Method::Vt, age_group: AgeGroup::Adult, kind: None });
        assert_eq!(vt.sections.len(), 5);
        let required: Vec<_> = vt.sections.iter().filter(|s| s.required).map(|s| s.key.as_str()).collect();
        assert_eq!(required, ["verlauf"]);
        assert!(vt.sections.iter().find(|s| s.key == "verlauf").unwrap().dictation);
        assert!(hints(&vt, "interventionen").iter().any(|h| h.contains("Exposition")));
        assert!(!hints(&vt, "risiko").iter().any(|h| h.contains("Kindeswohl")));
        let tp = note.resolve(&Context { method: Method::Tp, age_group: AgeGroup::Kjp, kind: None });
        assert!(hints(&tp, "interventionen").iter().any(|h| h.contains("Übertragung")));
        assert!(hints(&tp, "risiko").iter().any(|h| h.contains("Kindeswohl")));
    }

    fn asks(t: &Template, key: &str) -> Vec<String> {
        t.sections.iter().find(|s| s.key == key).unwrap().asks.iter().map(|a| a.key.clone()).collect()
    }

    #[test]
    fn questions_follow_method_age_and_kind() {
        let vt = erst().resolve(&Context { method: Method::Vt, age_group: AgeGroup::Adult, kind: Some(ApplicationKind::Erstantrag) });
        assert!(asks(&vt, "soziodemographie").contains(&"beruf".to_string()));
        assert!(asks(&vt, "lebensgeschichte_modell").contains(&"bedingungsmodell".to_string()));
        assert!(!asks(&vt, "lebensgeschichte_modell").contains(&"psychodynamik".to_string()));
        assert!(asks(&vt, "somatik").contains(&"medikation".to_string()));
        assert!(vt.sections.iter().all(|s| s.field == FieldKind::Prose || s.asks.is_empty()), "no questions about diagnoses");
        let tp = erst().resolve(&Context { method: Method::Tp, age_group: AgeGroup::Kjp, kind: Some(ApplicationKind::Umwandlung) });
        assert!(asks(&tp, "lebensgeschichte_modell").contains(&"psychodynamik".to_string()));
        assert!(asks(&tp, "soziodemographie").contains(&"schule".to_string()));
        assert!(!asks(&tp, "soziodemographie").contains(&"beruf".to_string()));
        assert!(asks(&tp, "umwandlung").contains(&"begruendung".to_string()));
        let fort = builtin().unwrap().into_iter().find(|t| t.key == "ptv3-fortfuehrung").unwrap();
        let fort = fort.resolve(&Context { method: Method::Vt, age_group: AgeGroup::Adult, kind: Some(ApplicationKind::Fortfuehrung) });
        assert!(asks(&fort, "begruendung_planung").contains(&"begruendung".to_string()));
        let record: Vec<_> = vt.sections.iter().flat_map(|s| &s.asks).filter(|a| a.record.is_some()).collect();
        assert_eq!(record.len(), 1);
    }

    #[test]
    fn no_questions_on_the_diagnoses() {
        let src = include_str!("../../../templates/ptv3/fortfuehrung.yaml").replace(
            "    ai_allowed: false
",
            "    ai_allowed: false
    asks:
      - key: icd
        question: Welche Diagnose?
",
        );
        assert!(src.contains("key: icd"));
        assert!(Template::from_yaml(&src).is_err());
    }

    #[test]
    fn diagnoses_cannot_be_ai_drafted() {
        let src = include_str!("../../../templates/ptv3/fortfuehrung.yaml").replace("ai_allowed: false", "ai_allowed: true");
        assert!(Template::from_yaml(&src).is_err());
    }

    #[test]
    fn letter_berichte_are_complete_and_safe() {
        let all = builtin().unwrap();
        for key in ["drv-befund", "pflege-stellungnahme", "versorgungsamt-befund", "arztbrief"] {
            let t = all.iter().find(|t| t.key == key).unwrap();
            assert_eq!((t.category, t.layout), (Category::Bericht, Layout::Letter), "{key}");
            assert!(t.audience.is_some() && t.release && t.subject.is_some() && !t.recipients.is_empty(), "{key}");
            assert!(!t.rules.pseudonymized, "{key}: named letters");
            let r = t.resolve(&Context { method: Method::Tp, age_group: AgeGroup::Adult, kind: None });
            assert_eq!(r.sections.len(), t.sections.len(), "{key}: no section depends on the Antragsart");
            assert!(r.sections.iter().any(|s| s.field == FieldKind::Diagnoses && !s.ai_allowed), "{key}: diagnoses by her");
            assert!(r.sections.iter().map(|s| s.asks.len()).sum::<usize>() >= 4, "{key}: questions for the Fragen step");
            // The opening paragraph has text, so its questions would never count as open.
            assert!(r.sections.iter().all(|s| s.starter.is_none() || s.asks.is_empty()), "{key}");
        }
        let drv = all.iter().find(|t| t.key == "drv-befund").unwrap();
        assert!(drv.refs.iter().any(|r| r.required && r.from == Some(PatientRef::PensionNo)));
        assert!(!all.iter().find(|t| t.key == "bescheinigung").unwrap().release);
    }

    #[test]
    fn a_bericht_letter_needs_an_audience_and_no_antragsart() {
        let src = include_str!("../../../templates/briefe/arztbrief.yaml").replace("\r\n", "\n");
        assert!(Template::from_yaml(&src).is_ok());
        assert!(Template::from_yaml(&src.replace("audience: arzt\n", "")).is_err());
        assert!(Template::from_yaml(&src.replace("audience: arzt\n", "audience: arzt\nreport_kinds: [erstantrag]\n")).is_err());
        let ptv3 = include_str!("../../../templates/ptv3/fortfuehrung.yaml");
        assert!(Template::from_yaml(&format!("{ptv3}\nrelease: true\n")).is_err(), "release is for letters");
    }

    #[test]
    fn umwandlung_asks_for_the_family_history() {
        let t = erst().resolve(&Context { method: Method::Vt, age_group: AgeGroup::Adult, kind: Some(ApplicationKind::Umwandlung) });
        assert!(asks(&t, "symptomatik_befund").contains(&"familienanamnese".to_string()));
        assert!(hints(&t, "symptomatik_befund").iter().any(|h| h.contains("Familienanamnese")));
    }
}
