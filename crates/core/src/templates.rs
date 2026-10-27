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
    pub rules: Rules,
    #[serde(default)]
    pub intro: String,
    pub sections: Vec<SectionDef>,
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
    /// Receives the therapist's confirmed dictation when the document is created.
    #[serde(default)]
    pub dictation: bool,
    #[serde(default)]
    pub when: When,
    #[serde(default)]
    pub hints: Vec<Hint>,
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
        let mut seen = std::collections::HashSet::new();
        for s in &self.sections {
            if !seen.insert(s.key.as_str()) {
                return Err(Error::Template(format!("duplicate section key {}", s.key)));
            }
            if s.field == FieldKind::Diagnoses && s.ai_allowed {
                return Err(Error::Template(format!(
                    "section {}: diagnoses must be ai_allowed: false (MDR boundary)",
                    s.key
                )));
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

    #[test]
    fn diagnoses_cannot_be_ai_drafted() {
        let src = include_str!("../../../templates/ptv3/fortfuehrung.yaml").replace("ai_allowed: false", "ai_allowed: true");
        assert!(Template::from_yaml(&src).is_err());
    }
}
