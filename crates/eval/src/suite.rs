//! The golden set: synthetic German sessions (`cases/*.yaml`) with the facts a careful therapist
//! would note, and episodes (`episodes.yaml`) that group sessions into one Bericht. Each expected
//! fact also says which Bericht sections it belongs to, so the same file scores both stages.

use std::collections::BTreeMap;
use std::path::Path;

use regex::{Regex, RegexBuilder};
use serde::Deserialize;
use thera_core::domain::{AgeGroup, ApplicationKind, Method, Patient, SourceKind, SourceSegment, Speaker};
use thera_core::pseudo::{Dictionary, EntityKind};
use thera_core::templates::{self, Context, Template, SESSION_NOTE_KEY};

pub type Res<T> = Result<T, String>;

/// One value or a list of them (`section: befund` or `section: [befund, verlauf]`).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            OneOrMany::One(s) => vec![s.clone()],
            OneOrMany::Many(v) => v.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatientDef {
    pub first_name: String,
    pub last_name: String,
    pub birth_date: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TermDef {
    pub kind: EntityKind,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentDef {
    pub role: Speaker,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactDef {
    pub id: String,
    /// Sitzungsnotiz section(s) where the fact is correctly filed.
    pub section: OneOrMany,
    /// The fact as the therapist would apply it; the drafting input.
    pub text: String,
    /// Regexes (case-insensitive), all of which a proposed fact must match to count as this one.
    #[serde(rename = "match")]
    pub patterns: Vec<String>,
    /// 1-based segment numbers the fact comes from (any one cited is enough).
    pub segments: Vec<usize>,
    /// Missing it isn't counted against recall (minor detail).
    #[serde(default = "yes")]
    pub required: bool,
    /// Bericht sections that should use this fact (recall and precision).
    #[serde(default)]
    pub bericht: Vec<String>,
    /// Bericht sections where using it is acceptable but not expected (precision only).
    #[serde(default)]
    pub bericht_ok: Vec<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseFile {
    id: String,
    title: String,
    method: Method,
    age_group: AgeGroup,
    patient: PatientDef,
    #[serde(default)]
    terms: Vec<TermDef>,
    /// Smaller chunks than the app's, to exercise multi-chunk extraction on a short session.
    chunk_chars: Option<usize>,
    /// A session recording with speakers separated: every segment is session audio and `role`
    /// says who speaks. Otherwise `therapist` marks a dictation and `unknown` session audio.
    #[serde(default)]
    diarized: bool,
    segments: Vec<SegmentDef>,
    facts: Vec<FactDef>,
    /// Regexes that must not appear in any proposed fact (invented or off-limits content).
    #[serde(default)]
    forbid: Vec<String>,
}

pub struct Fact {
    pub def: FactDef,
    pub patterns: Vec<Regex>,
    pub sections: Vec<String>,
    /// 0-based.
    pub segments: Vec<usize>,
}

pub struct Case {
    pub id: String,
    pub title: String,
    pub method: Method,
    pub age_group: AgeGroup,
    pub patient: Patient,
    pub terms: Vec<TermDef>,
    pub chunk_chars: Option<usize>,
    pub segments: Vec<SourceSegment>,
    pub facts: Vec<Fact>,
    pub forbid: Vec<Regex>,
    /// The Sitzungsnotiz template resolved for this case.
    pub note: Template,
    /// The Bericht template facts are tagged with, resolved for this case (Erstantrag).
    pub tag_template: Template,
}

impl Case {
    pub fn dictionary(&self) -> Dictionary {
        let mut d = Dictionary::from_patient(&self.patient);
        for t in &self.terms {
            d.add(t.kind, &t.text);
        }
        d
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EpisodeDef {
    id: String,
    kind: ApplicationKind,
    sessions: Vec<String>,
    #[serde(default)]
    forbid: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EpisodesFile {
    episodes: Vec<EpisodeDef>,
}

pub struct Episode {
    pub id: String,
    pub kind: ApplicationKind,
    /// Indices into [`Suite::cases`], in session order.
    pub sessions: Vec<usize>,
    pub forbid: Vec<Regex>,
    pub bericht: Template,
}

pub struct Suite {
    pub name: String,
    pub cases: Vec<Case>,
    pub episodes: Vec<Episode>,
}

/// Candidate per-section source mapping: Bericht section → Sitzungsnotiz sections it may draw on.
pub type Sources = BTreeMap<String, Vec<String>>;

pub fn regex(p: &str) -> Res<Regex> {
    RegexBuilder::new(p).case_insensitive(true).build().map_err(|e| format!("regex {p:?}: {e}"))
}

fn regexes(list: &[String]) -> Res<Vec<Regex>> {
    list.iter().map(|p| regex(p)).collect()
}

fn template(all: &[Template], key: &str, ctx: &Context) -> Res<Template> {
    Ok(all.iter().find(|t| t.key == key).ok_or_else(|| format!("no template {key}"))?.resolve(ctx))
}

pub fn load(dir: &Path) -> Res<Suite> {
    let all = templates::builtin().map_err(|e| e.to_string())?;
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();

    let mut paths: Vec<_> = std::fs::read_dir(dir.join("cases"))
        .map_err(|e| format!("{}: {e}", dir.join("cases").display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "yaml"))
        .collect();
    paths.sort();
    let mut cases = Vec::new();
    for path in paths {
        let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file: CaseFile = serde_yaml_ng::from_str(&src).map_err(|e| format!("{}: {e}", path.display()))?;
        let case = case(file, &all).map_err(|e| format!("{}: {e}", path.display()))?;
        if cases.iter().any(|c: &Case| c.id == case.id) {
            return Err(format!("duplicate case id {}", case.id));
        }
        cases.push(case);
    }

    let path = dir.join("episodes.yaml");
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file: EpisodesFile = serde_yaml_ng::from_str(&src).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut episodes = Vec::new();
    for e in file.episodes {
        let ep = episode(e, &cases, &all)?;
        if episodes.iter().any(|x: &Episode| x.id == ep.id) {
            return Err(format!("duplicate episode id {}", ep.id));
        }
        episodes.push(ep);
    }
    Ok(Suite { name, cases, episodes })
}

fn case(f: CaseFile, all: &[Template]) -> Res<Case> {
    let note = template(all, SESSION_NOTE_KEY, &Context { method: f.method, age_group: f.age_group, kind: None })?;
    let targets: Vec<&str> = thera_core::extract::target_sections(&note).iter().map(|s| s.key.as_str()).collect();
    let tag_ctx = Context { method: f.method, age_group: f.age_group, kind: Some(ApplicationKind::Erstantrag) };
    let tag_template = template(all, thera_core::extract::TAG_TEMPLATE_KEY, &tag_ctx)?;
    let tags: Vec<&str> = thera_core::extract::tag_sections(&tag_template).iter().map(|s| s.key.as_str()).collect();
    if f.segments.is_empty() || f.facts.is_empty() {
        return Err("needs segments and facts".into());
    }
    let mut facts = Vec::new();
    for def in f.facts {
        let sections = def.section.to_vec();
        if let Some(s) = sections.iter().find(|s| !targets.contains(&s.as_str())) {
            return Err(format!("fact {}: unknown note section {s}", def.id));
        }
        if def.patterns.is_empty() {
            return Err(format!("fact {}: needs at least one match pattern", def.id));
        }
        if def.segments.is_empty() || def.segments.iter().any(|&n| n == 0 || n > f.segments.len()) {
            return Err(format!("fact {}: segments must be 1..={}", def.id, f.segments.len()));
        }
        if let Some(k) = def.bericht.iter().chain(&def.bericht_ok).find(|k| !tags.contains(&k.as_str())) {
            return Err(format!("fact {}: {k} is not a Bericht section facts are tagged with", def.id));
        }
        if facts.iter().any(|x: &Fact| x.def.id == def.id) {
            return Err(format!("duplicate fact id {}", def.id));
        }
        let patterns = regexes(&def.patterns)?;
        let segments = def.segments.iter().map(|n| n - 1).collect();
        facts.push(Fact { patterns, sections, segments, def });
    }
    let segments = f
        .segments
        .iter()
        .enumerate()
        .map(|(i, s)| SourceSegment {
            segment_id: format!("{}-{}", f.id, i + 1),
            source_id: f.id.clone(),
            kind: if f.diarized || s.role != Speaker::Therapist { SourceKind::SessionAudio } else { SourceKind::Dictation },
            role: s.role,
            start_ms: i as i64 * 8_000,
            end_ms: (i as i64 + 1) * 8_000,
            text: s.text.clone(),
        })
        .collect();
    let patient = Patient {
        id: format!("eval-{}", f.id),
        external_ref: None,
        first_name: f.patient.first_name,
        last_name: f.patient.last_name,
        birth_date: f.patient.birth_date,
        chiffre: None,
        pronouns: None,
        consent_recording: true,
        status: "active".into(),
        created_at: String::new(),
        archived_at: None,
    };
    Ok(Case {
        id: f.id,
        title: f.title,
        method: f.method,
        age_group: f.age_group,
        patient,
        terms: f.terms,
        chunk_chars: f.chunk_chars,
        segments,
        facts,
        forbid: regexes(&f.forbid)?,
        note,
        tag_template,
    })
}

fn episode(e: EpisodeDef, cases: &[Case], all: &[Template]) -> Res<Episode> {
    let mut sessions = Vec::new();
    for id in &e.sessions {
        sessions.push(cases.iter().position(|c| &c.id == id).ok_or_else(|| format!("episode {}: no case {id}", e.id))?);
    }
    let first = &cases[*sessions.first().ok_or_else(|| format!("episode {}: no sessions", e.id))?];
    for &i in &sessions {
        let c = &cases[i];
        if c.method != first.method || c.age_group != first.age_group || c.patient.last_name != first.patient.last_name {
            return Err(format!("episode {}: case {} has another patient, method or age group", e.id, c.id));
        }
    }
    let ctx = Context { method: first.method, age_group: first.age_group, kind: Some(e.kind) };
    let bericht = template(all, templates::template_key_for(e.kind), &ctx)?;
    let targets: Vec<&str> = thera_core::draft::target_sections(&bericht).iter().map(|s| s.key.as_str()).collect();
    for &i in &sessions {
        for f in &cases[i].facts {
            if let Some(k) = f.def.bericht.iter().chain(&f.def.bericht_ok).find(|k| !targets.contains(&k.as_str())) {
                return Err(format!("case {} fact {}: {k} is not a draftable section of {}", cases[i].id, f.def.id, bericht.key));
            }
        }
    }
    Ok(Episode { id: e.id, kind: e.kind, sessions, forbid: regexes(&e.forbid)?, bericht })
}

pub fn load_sources(path: &Path, suite: &Suite) -> Res<Sources> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let map: Sources = serde_yaml_ng::from_str(&src).map_err(|e| format!("{}: {e}", path.display()))?;
    let note_keys: Vec<String> = suite.cases.iter().flat_map(|c| c.note.sections.iter().map(|s| s.key.clone())).collect();
    let bericht_keys: Vec<String> = suite.episodes.iter().flat_map(|e| e.bericht.sections.iter().map(|s| s.key.clone())).collect();
    for (k, v) in &map {
        if !bericht_keys.contains(k) {
            return Err(format!("{}: unknown Bericht section {k}", path.display()));
        }
        if let Some(n) = v.iter().find(|n| !note_keys.contains(n)) {
            return Err(format!("{}: unknown Sitzungsnotiz section {n}", path.display()));
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_suite_loads_and_is_consistent() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("suites/de-therapy-v1");
        let suite = load(&dir).unwrap();
        assert!(suite.cases.len() >= 6);
        assert!(!suite.episodes.is_empty());
        load_sources(&dir.join("sources.yaml"), &suite).unwrap();
        for c in &suite.cases {
            // Every expected fact's patterns must hit its own canonical text and cited segments,
            // or the case could never score a match.
            for f in &c.facts {
                for p in &f.patterns {
                    assert!(p.is_match(&f.def.text), "{} {}: {} misses its own text", c.id, f.def.id, p);
                    let cited: String = f.segments.iter().map(|&i| c.segments[i].text.as_str()).collect::<Vec<_>>().join(" ");
                    assert!(p.is_match(&cited), "{} {}: {} misses the cited segments", c.id, f.def.id, p);
                }
                for p in &c.forbid {
                    assert!(!p.is_match(&f.def.text), "{} {}: forbidden {} in the expected text", c.id, f.def.id, p);
                }
            }
            // The canonical facts are what a careful extraction looks like: they pass the parser.
            let tags = thera_core::extract::tag_sections(&c.tag_template);
            thera_core::extract::prepare(&c.note, &tags, &c.segments, c.dictionary(), thera_core::extract::CHUNK_CHARS).unwrap();
        }
    }
}
