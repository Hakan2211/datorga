//! Style profile and exemplars per section (slice 52; PLAN-AMENDMENTS §3).
//!
//! From her redacted exemplars of one document type the app computes, locally, how she writes:
//! sentence length, Ich-/Wir-/unpersönliche Form, how she names the person, prose or bullet
//! points, her usual length per section and phrases she repeats. With her own notes (per type
//! and for all types) this goes into the system prompt as "Stilprofil".
//!
//! An exemplar is split along the template's headings ([`split_sections`]); a Bericht draft then
//! sends each section only the matching parts ("Stilbeispiele für diesen Abschnitt") instead of
//! whole letters. Exemplars whose headings don't match go along whole, as before (slice 49).
//! All of it is derived from redacted text: no patient data, and never a source.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::style::StyleExemplar;
use crate::templates::{FieldKind, Template};

/// Characters of one exemplar's part per section request.
pub const SECTION_CHARS: usize = 1_500;
/// Exemplars whose parts go along per section.
pub const PER_SECTION: usize = 3;
/// Her notes, per type and for all.
pub const MAX_NOTES_CHARS: usize = 1_500;
/// The `template_key` of notes for every type.
pub const ALL_TYPES: &str = "*";

/// How she refers to herself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Voice {
    /// "ich", "meine Behandlung".
    Ich,
    /// "wir", "unsere Praxis".
    Wir,
    /// Neither: "Die Patientin berichtet …".
    Unpersoenlich,
}

/// Prose or bullet points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Form {
    Fliesstext,
    Teils,
    Stichpunkte,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SectionLength {
    pub key: String,
    pub heading: String,
    /// Median words over the exemplars that have the section.
    pub words: u32,
}

/// What the app read from her exemplars of one type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Computed {
    pub exemplars: u32,
    /// Median words per sentence.
    pub sentence_words: u32,
    pub voice: Voice,
    pub form: Form,
    /// "Pat.", "die Patientin", "Frau [Name]", … (the most frequent), if any.
    pub person_term: Option<String>,
    /// Her usual length per section (exemplars split along the template's headings).
    pub sections: Vec<SectionLength>,
    /// Phrases (3–7 words) found in at least two exemplars.
    pub phrases: Vec<String>,
}

/// A part of an exemplar: the text under one section heading of the template.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub key: String,
    pub text: String,
}

fn norm(s: &str) -> String {
    let s = s.trim();
    // "3.", "3.1", "a)", "IV." numbering is not part of the heading.
    let s = s.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')' || c == ' ');
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with(' ') && !out.is_empty() {
            out.push(' ');
        }
    }
    out.trim().to_string()
}

/// A heading line about diagnoses ("Diagnosen:", "5. Diagnose nach ICD-10"): what follows is
/// hers to fill in each time, never style, whether or not the template has such a section.
fn is_diagnoses_heading(line: &str) -> bool {
    let n = norm(line);
    line.chars().count() <= 80 && n.split_whitespace().count() <= 8 && (n.starts_with("diagnose") || n.starts_with("icd") || n.contains(" icd"))
}

/// Which section of `template` the line is the heading of, if any.
fn heading_of<'a>(line: &str, template: &'a Template, used: &HashSet<&str>) -> Option<&'a str> {
    let line = line.trim();
    if line.is_empty() || line.chars().count() > 120 || line.ends_with(['.', ',', ';']) && !line.ends_with("..") && line.split_whitespace().count() > 12 {
        return None;
    }
    let n = norm(line);
    if n.split_whitespace().count() == 0 {
        return None;
    }
    let number = line.split(|c: char| !(c.is_ascii_digit() || c == '.')).next().unwrap_or("").trim_end_matches('.');
    template.sections.iter().filter(|s| !s.heading.trim().is_empty() && !used.contains(s.key.as_str())).find_map(|s| {
        let h = norm(&s.heading);
        let by_text = n == h
            || (n.starts_with(&h) && h.split_whitespace().count() >= 2)
            || (h.starts_with(&n) && n.split_whitespace().count() >= 2 && n.chars().count() * 10 >= h.chars().count() * 6);
        // "4. Lebensgeschichte …", "5. Diagnosen": the number of a numbered template and the
        // first word's stem are enough.
        let stem = |w: &str| w.chars().take(6).collect::<String>();
        let by_number = !number.is_empty()
            && s.number.as_deref() == Some(number)
            && line.chars().count() <= 80
            && h.split_whitespace().next().zip(n.split_whitespace().next()).is_some_and(|(a, b)| stem(a) == stem(b));
        (by_text || by_number).then_some(s.key.as_str())
    })
}

/// The exemplar split along the template's headings. Text before the first heading belongs to
/// the opening section (a letter's section without heading), if the template has one. Diagnosen
/// are left out (hers to fill in, no style). Empty when fewer than two headings were found in a
/// template with two or more: then the exemplar is used whole.
pub fn split_sections(text: &str, template: &Template) -> Vec<Part> {
    let headed = template.sections.iter().filter(|s| !s.heading.trim().is_empty()).count();
    let opening = template.sections.iter().find(|s| s.heading.trim().is_empty()).map(|s| s.key.as_str());
    let mut used: HashSet<&str> = HashSet::new();
    let mut parts: Vec<(Option<&str>, String)> = vec![(opening, String::new())];
    for line in text.lines() {
        if let Some(key) = heading_of(line, template, &used) {
            used.insert(key);
            parts.push((Some(key), String::new()));
            continue;
        }
        if is_diagnoses_heading(line) {
            parts.push((None, String::new()));
            continue;
        }
        let cur = &mut parts.last_mut().expect("one part").1;
        cur.push_str(line.trim_end());
        cur.push('\n');
    }
    if used.len() < 2.min(headed) || used.is_empty() {
        return Vec::new();
    }
    parts
        .into_iter()
        .filter_map(|(key, text)| {
            let key = key?;
            let def = template.sections.iter().find(|s| s.key == key)?;
            let text = text.trim().to_string();
            (def.field != FieldKind::Diagnoses && !text.is_empty()).then(|| Part { key: key.to_string(), text })
        })
        .collect()
}

// ---------- the computed profile ----------

const ABBREVIATIONS: &[&str] = &["z. B.", "z.B.", "u. a.", "d. h.", "o. g.", "ggf.", "bzw.", "Pat.", "Dr.", "geb.", "ca.", "vgl.", "usw.", "etc.", "evtl.", "inkl.", "Nr.", "med.", "Dipl.-Psych.", "s. o.", "v. a."];

/// Lines that are text (not headings or the letter frame).
fn body_lines<'a>(text: &'a str, template: &Template) -> Vec<&'a str> {
    let none = HashSet::new();
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && heading_of(l, template, &none).is_none())
        .filter(|l| {
            let lower = l.to_lowercase();
            !(lower.starts_with("sehr geehrte") || lower.starts_with("mit freundlichen") || lower.starts_with("betreff") || lower.starts_with("liebe"))
        })
        .collect()
}

fn sentences(lines: &[&str]) -> Vec<String> {
    let mut text = lines.iter().filter(|l| !is_bullet(l)).copied().collect::<Vec<_>>().join(" ");
    for (i, a) in ABBREVIATIONS.iter().enumerate() {
        text = text.replace(a, &format!("\u{1}{i}\u{1}"));
    }
    text.split(['.', '!', '?'])
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| s.split_whitespace().count() >= 3)
        .collect()
}

fn is_bullet(line: &str) -> bool {
    line.starts_with(['-', '•', '*', '–', '·'])
}

fn median(mut v: Vec<u32>) -> u32 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[v.len() / 2]
}

/// Words as written, a trailing comma, semicolon or colon kept (phrases show them).
fn words(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|w| {
            let w = w.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '[' && c != '\u{1}');
            let core = w.trim_end_matches(|c: char| !c.is_alphanumeric() && c != ']' && c != '\u{1}');
            let tail = &w[core.len()..];
            let keep = tail.chars().next().filter(|c| matches!(c, ',' | ';' | ':')).map(String::from).unwrap_or_default();
            format!("{core}{keep}")
        })
        .filter(|w| !w.is_empty())
        .collect()
}

fn voice(text: &str) -> Voice {
    let ws: Vec<String> = text.split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    let total = ws.len().max(1) as f32;
    let ich = ws.iter().filter(|w| matches!(w.as_str(), "ich" | "mir" | "mich" | "mein" | "meine" | "meiner" | "meinem" | "meinen")).count() as f32;
    let wir = ws.iter().filter(|w| matches!(w.as_str(), "wir" | "uns" | "unser" | "unsere" | "unserer" | "unserem" | "unseren")).count() as f32;
    if ich / total * 1000.0 >= 2.0 && ich >= wir {
        Voice::Ich
    } else if wir / total * 1000.0 >= 2.0 {
        Voice::Wir
    } else {
        Voice::Unpersoenlich
    }
}

fn person_term(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let count = |needles: &[&str]| needles.iter().map(|n| lower.matches(n).count()).sum::<usize>();
    let candidates = [
        ("Pat.", count(&["pat. "])),
        ("die Patientin / der Patient", count(&["patientin", "patient ", "patienten "])),
        ("Frau / Herr [Name]", count(&["frau [name]", "herr [name]", "herrn [name]"])),
        ("die Klientin / der Klient", count(&["klientin", "klient "])),
        ("die Antragstellerin / der Antragsteller", count(&["antragstellerin", "antragsteller "])),
    ];
    // The most frequent; a tie goes to the earlier (shorter) form.
    let mut best: Option<(&str, usize)> = None;
    for (t, n) in candidates {
        if n >= 2 && best.is_none_or(|(_, b)| n > b) {
            best = Some((t, n));
        }
    }
    best.map(|(t, _)| t.to_string())
}

/// Phrases of 3–8 words, inside one sentence, that appear in at least two exemplars (no
/// placeholders or numbers), longest and most frequent first, at most `max`, none overlapping
/// another by three words or more.
fn phrases(docs: &[Vec<Vec<String>>], max: usize) -> Vec<String> {
    if docs.len() < 2 {
        return Vec::new();
    }
    let mut seen_in: HashMap<Vec<String>, (usize, String)> = HashMap::new();
    for doc in docs {
        let mut mine: HashSet<Vec<String>> = HashSet::new();
        for ws in doc {
            let lower: Vec<String> = ws.iter().map(|w| w.trim_end_matches([',', ';', ':']).to_lowercase()).collect();
            for n in 3..=8usize {
                for i in 0..(lower.len() + 1).saturating_sub(n) {
                    let gram = &lower[i..i + n];
                    if gram.iter().any(|w| w.contains(['[', ']', '\u{1}']) || w.chars().any(|c| c.is_ascii_digit())) {
                        continue;
                    }
                    if is_stop(&gram[0]) || is_stop(&gram[n - 1]) {
                        continue;
                    }
                    if mine.insert(gram.to_vec()) {
                        let shown = ws[i..i + n].join(" ").trim_end_matches([',', ';', ':']).to_string();
                        seen_in.entry(gram.to_vec()).or_insert((0, shown)).0 += 1;
                    }
                }
            }
        }
    }
    let mut found: Vec<(usize, String)> = seen_in.into_values().filter(|(n, _)| *n >= 2).collect();
    found.sort_by(|a, b| (b.1.split_whitespace().count() * b.0).cmp(&(a.1.split_whitespace().count() * a.0)).then(a.1.cmp(&b.1)));
    let trigrams = |p: &str| -> HashSet<Vec<String>> {
        let w: Vec<String> = p.to_lowercase().split_whitespace().map(|x| x.trim_end_matches([',', ';', ':']).to_string()).collect();
        w.windows(3).map(|x| x.to_vec()).collect()
    };
    let mut out: Vec<String> = Vec::new();
    let mut taken: HashSet<Vec<String>> = HashSet::new();
    for (_, p) in found {
        let t = trigrams(&p);
        if !t.is_disjoint(&taken) {
            continue;
        }
        taken.extend(t);
        out.push(p);
        if out.len() >= max {
            break;
        }
    }
    out
}

fn is_stop(w: &str) -> bool {
    matches!(w, "und" | "oder" | "der" | "die" | "das" | "den" | "dem" | "des" | "ein" | "eine" | "einer" | "einem" | "einen" | "in" | "im" | "zu" | "zur" | "zum" | "mit" | "von" | "vom" | "auf" | "an" | "am" | "bei" | "für" | "ist" | "sind" | "wird" | "werden" | "sich" | "es" | "sie" | "er" | "nicht" | "auch" | "als" | "so" | "da" | "dass")
}

/// The profile of a type from its active exemplars; `None` without any.
pub fn compute(exemplars: &[StyleExemplar], template: &Template) -> Option<Computed> {
    let active: Vec<&StyleExemplar> = exemplars.iter().filter(|e| e.active && e.template_key == template.key).collect();
    if active.is_empty() {
        return None;
    }
    let mut sentence_words = Vec::new();
    let (mut bullets, mut lines_total) = (0usize, 0usize);
    let mut all = String::new();
    let mut by_key: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let mut word_lists = Vec::new();
    for e in &active {
        let lines = body_lines(&e.text, template);
        lines_total += lines.len();
        bullets += lines.iter().filter(|l| is_bullet(l)).count();
        sentence_words.extend(sentences(&lines).iter().map(|s| s.split_whitespace().count() as u32));
        all.push_str(&lines.join("\n"));
        all.push('\n');
        word_lists.push(sentences(&lines).iter().map(|x| words(x)).collect::<Vec<_>>());
        for p in split_sections(&e.text, template) {
            by_key.entry(p.key).or_default().push(p.text.split_whitespace().count() as u32);
        }
    }
    let share = bullets as f32 / lines_total.max(1) as f32;
    let form = if share >= 0.3 {
        Form::Stichpunkte
    } else if share >= 0.1 {
        Form::Teils
    } else {
        Form::Fliesstext
    };
    let sections = template
        .sections
        .iter()
        .filter_map(|s| {
            let v = by_key.get(&s.key)?;
            Some(SectionLength { key: s.key.clone(), heading: s.heading.clone(), words: median(v.clone()) })
        })
        .collect();
    Some(Computed {
        exemplars: active.len() as u32,
        sentence_words: median(sentence_words),
        voice: voice(&all),
        form,
        person_term: person_term(&all),
        sections,
        phrases: phrases(&word_lists, 6),
    })
}

// ---------- prompt ----------

/// The "Stilprofil" block for the system prompt: what the app computed and her notes (for the
/// type, then for all). Empty when there is neither.
pub fn prompt_block(computed: Option<&Computed>, notes_type: Option<&str>, notes_all: Option<&str>) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(c) = computed {
        if c.sentence_words > 0 {
            lines.push(format!("Sätze: im Mittel etwa {} Wörter.", c.sentence_words));
        }
        lines.push(
            match c.voice {
                Voice::Ich => "Perspektive: Ich-Form („ich“, „meine Behandlung“).",
                Voice::Wir => "Perspektive: Wir-Form („wir“, „unsere Praxis“).",
                Voice::Unpersoenlich => "Perspektive: unpersönlich, ohne „ich“ und „wir“.",
            }
            .to_string(),
        );
        if let Some(t) = &c.person_term {
            lines.push(format!("Die Person heißt meist: {t}."));
        }
        lines.push(
            match c.form {
                Form::Fliesstext => "Form: Fließtext, kaum Stichpunkte.",
                Form::Teils => "Form: Fließtext, teils Stichpunkte.",
                Form::Stichpunkte => "Form: überwiegend Stichpunkte.",
            }
            .to_string(),
        );
        if !c.sections.is_empty() {
            let list: Vec<String> = c.sections.iter().map(|s| format!("{} {}", if s.heading.trim().is_empty() { "Einleitung" } else { s.heading.trim() }, s.words)).collect();
            lines.push(format!("Übliche Länge je Abschnitt (Wörter): {}.", list.join(", ")));
        }
        if !c.phrases.is_empty() {
            lines.push(format!("Wiederkehrende Wendungen: {}.", c.phrases.iter().map(|p| format!("„{p}“")).collect::<Vec<_>>().join(", ")));
        }
    }
    for n in [notes_type, notes_all].into_iter().flatten() {
        let n = n.trim();
        if !n.is_empty() {
            lines.push(format!("Hinweise der Therapeutin: {n}"));
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let source = computed.map_or_else(|| "aus ihren Hinweisen".to_string(), |c| format!("aus {} früheren Schreiben dieser Art", c.exemplars));
    format!("Stilprofil der Therapeutin ({source}; nur Stil, keine Inhalte):\n- {}\n", lines.join("\n- "))
}

/// The parts of her exemplars for one section, for that section's request: at most
/// [`PER_SECTION`], each cut at a sentence before [`SECTION_CHARS`].
pub fn section_block(parts: &[(String, String)], usual_words: Option<u32>) -> String {
    if parts.is_empty() && usual_words.is_none() {
        return String::new();
    }
    let mut out = String::from("Stilbeispiele für diesen Abschnitt (frühere Schreiben der Therapeutin über andere Personen; nur Stil, keine Inhalte, keine Quelle):\n");
    for (i, (title, text)) in parts.iter().take(PER_SECTION).enumerate() {
        out.push_str(&format!("[S{}] {}\n\"\"\"\n{}\n\"\"\"\n", i + 1, title.trim(), cut(text, SECTION_CHARS)));
    }
    if let Some(w) = usual_words.filter(|w| *w > 0) {
        out.push_str(&format!("Ihre übliche Länge für diesen Abschnitt: etwa {w} Wörter.\n"));
    }
    out
}

fn cut(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let end = text.char_indices().nth(max).map(|(b, _)| b).unwrap_or(text.len());
    let head = &text[..end];
    let at = head.rfind(". ").map(|i| i + 1).filter(|&i| i > end / 2).unwrap_or(end);
    format!("{} […]", head[..at].trim_end())
}

/// The text without diagnosis lines: a heading about diagnoses and the lines under it up to the
/// next blank line, and any line that starts with an ICD-10 code ("F32.1 G", "F41.0: …").
/// Diagnoses are hers to fill in each time and never style (MDR boundary).
pub fn without_diagnoses(text: &str) -> String {
    let icd = |l: &str| {
        let b = l.trim_start().as_bytes();
        b.len() >= 3 && b[0].is_ascii_uppercase() && b[1].is_ascii_digit() && b[2].is_ascii_digit() && (b.len() == 3 || !b[3].is_ascii_alphanumeric())
    };
    let mut out: Vec<&str> = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        if is_diagnoses_heading(line) {
            skipping = true;
            continue;
        }
        if skipping && line.trim().is_empty() {
            skipping = false;
        }
        if skipping || icd(line) {
            continue;
        }
        out.push(line);
    }
    out.join("\n").trim().to_string()
}

/// Her notes as typed, trimmed and length-checked.
pub fn clean_notes(notes: &str) -> crate::Result<String> {
    let n = notes.trim().to_string();
    if n.chars().count() > MAX_NOTES_CHARS {
        return Err(crate::Error::Invalid(format!("Die Hinweise sind zu lang (höchstens {MAX_NOTES_CHARS} Zeichen)")));
    }
    Ok(n)
}

/// One type's profile for the Vorlagen page (`Store::style_profiles`).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ProfileView {
    /// A template key, or `*` for her notes on all types.
    pub template_key: String,
    pub notes: String,
    pub computed: Option<Computed>,
    /// (exemplar id, sections it was split into; 0 = used whole).
    pub mapped: Vec<(String, u32)>,
}

/// What goes along with a draft or letter of one type (see `Store::style_material`).
#[derive(Debug, Clone, Default)]
pub struct StyleMaterial {
    /// Exemplars that go along whole (in the system prompt): letters always; Bericht drafts
    /// only those whose headings didn't match the template.
    pub whole: Vec<(String, String)>,
    /// Bericht drafts: per section key, (title, part) of the exemplars that matched.
    pub sections: HashMap<String, Vec<(String, String)>>,
    /// Her usual words per section key.
    pub lengths: HashMap<String, u32>,
    /// The "Stilprofil" block ([`prompt_block`]); empty without one.
    pub profile: String,
    /// Active exemplars of the type.
    pub exemplars: usize,
}

impl StyleMaterial {
    pub fn is_empty(&self) -> bool {
        self.whole.is_empty() && self.sections.is_empty() && self.profile.is_empty()
    }

    /// What goes with the request for section `key` (empty without).
    pub fn section(&self, key: &str) -> String {
        section_block(self.sections.get(key).map_or(&[][..], |v| v.as_slice()), self.lengths.get(key).copied())
    }
}

/// Builds the material: newest active exemplars first; with `per_section` (Bericht drafts, one
/// request per section) matched ones per section (up to [`PER_SECTION`] each), the others whole
/// (up to [`crate::style::PER_REQUEST`]); letters (one request) take them all whole.
pub fn material(exemplars: &[StyleExemplar], template: &Template, notes_type: Option<&str>, notes_all: Option<&str>, per_section: bool) -> StyleMaterial {
    let mut active: Vec<&StyleExemplar> = exemplars.iter().filter(|e| e.active && e.template_key == template.key).collect();
    active.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let computed = compute(exemplars, template);
    let mut m = StyleMaterial { profile: prompt_block(computed.as_ref(), notes_type, notes_all), exemplars: active.len(), ..Default::default() };
    if let Some(c) = &computed {
        m.lengths = c.sections.iter().map(|s| (s.key.clone(), s.words)).collect();
    }
    for e in active {
        let parts = if per_section { split_sections(&e.text, template) } else { Vec::new() };
        if parts.is_empty() {
            if m.whole.len() < crate::style::PER_REQUEST {
                m.whole.push((e.title.clone(), crate::style::excerpt(&without_diagnoses(&e.text))));
            }
            continue;
        }
        for p in parts {
            let list = m.sections.entry(p.key).or_default();
            let text = without_diagnoses(&p.text);
            if list.len() < PER_SECTION && !text.is_empty() {
                list.push((e.title.clone(), text));
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(key: &str) -> Template {
        crate::templates::builtin().unwrap().into_iter().find(|t| t.key == key).unwrap()
    }

    fn exemplar(id: &str, key: &str, text: &str) -> StyleExemplar {
        StyleExemplar {
            id: id.into(),
            template_key: key.into(),
            title: format!("Brief {id}"),
            text: text.into(),
            active: true,
            source_sha256: None,
            created_at: format!("2026-01-0{id}"),
            updated_at: String::new(),
        }
    }

    const DRV_A: &str = "Frau [Name] befindet sich seit dem [Datum] in meiner ambulanten psychotherapeutischen Behandlung. Ich berichte wie folgt.\n\n\
Behandlung und Setting\nDie Behandlung findet wöchentlich als Einzeltherapie statt. Pat. kommt zuverlässig zu den Terminen.\n\n\
Diagnosen (ICD-10)\nF32.1 G\n\n\
Psychopathologischer Befund und Symptomatik\nPat. wirkt im Kontakt freundlich, jedoch deutlich erschöpft. Der Antrieb ist vermindert, die Stimmung gedrückt. Pat. berichtet von Ein- und Durchschlafstörungen.\n\n\
Zusammenfassende Empfehlung\nAus meiner Sicht ist eine Fortsetzung der Behandlung dringend angezeigt. Bei Rückfragen stehe ich gerne zur Verfügung.";

    const DRV_B: &str = "Herr [Name] befindet sich seit dem [Datum] in meiner ambulanten psychotherapeutischen Behandlung.\n\n\
1. Behandlung und Setting\nPat. nimmt seit zwei Jahren an Einzelsitzungen teil.\n\n\
Psychopathologischer Befund\nPat. ist wach und orientiert. Die Stimmung ist gedrückt, der Antrieb gemindert.\n\n\
Zusammenfassende Empfehlung\nAus meiner Sicht ist eine Fortsetzung der Behandlung dringend angezeigt.";

    #[test]
    fn exemplars_are_split_along_the_template_headings() {
        let t = template("drv-befund");
        let parts = split_sections(DRV_A, &t);
        let keys: Vec<&str> = parts.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["anlass", "behandlung", "befund", "empfehlung"], "{parts:?}");
        assert!(parts[0].text.starts_with("Frau [Name] befindet sich"));
        assert!(!parts.iter().any(|p| p.text.contains("F32.1")), "no Diagnosen");
        // Numbered and shortened headings still match.
        let b: Vec<String> = split_sections(DRV_B, &t).into_iter().map(|p| p.key).collect();
        assert_eq!(b, ["anlass", "behandlung", "befund", "empfehlung"]);
        // Diagnosen under a heading the template words differently: never in a style part.
        let ptv3 = template("ptv3-erst-umwandlung");
        let x = "1. Relevante soziodemographische Daten\nPat. ist verheiratet.\n\n2. Symptomatik und psychischer Befund\nPat. wirkt erschöpft.\n\nDiagnosen nach ICD-10:\nF32.1 G\n";
        let parts = split_sections(x, &ptv3);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|p| !p.text.contains("F32.1")), "{parts:?}");
        let y = x.replace("Diagnosen nach ICD-10:", "5. Diagnosen");
        assert!(split_sections(&y, &ptv3).iter().all(|p| !p.text.contains("F32.1")));
        // A letter without the template's headings: used whole.
        assert!(split_sections("Sehr geehrte Damen und Herren,\nPat. ist in Behandlung. Es geht ihr besser.", &t).is_empty());
    }

    #[test]
    fn the_profile_reads_how_she_writes() {
        let t = template("drv-befund");
        let ex = vec![exemplar("1", "drv-befund", DRV_A), exemplar("2", "drv-befund", DRV_B), StyleExemplar { active: false, ..exemplar("3", "drv-befund", "- a\n- b\n- c") }];
        let c = compute(&ex, &t).unwrap();
        assert_eq!(c.exemplars, 2, "paused ones don't count");
        assert_eq!(c.voice, Voice::Ich);
        assert_eq!(c.form, Form::Fliesstext);
        assert_eq!(c.person_term.as_deref(), Some("Pat."));
        assert!((6..=14).contains(&c.sentence_words), "{}", c.sentence_words);
        assert!(c.sections.iter().any(|s| s.key == "befund" && s.words > 10), "{:?}", c.sections);
        assert!(c.phrases.iter().any(|p| p.contains("ambulanten psychotherapeutischen Behandlung")), "{:?}", c.phrases);
        assert!(c.phrases.iter().all(|p| !p.contains('[')), "no placeholders");
        let block = prompt_block(Some(&c), Some("kurz halten"), Some("keine Abkürzungen außer Pat."));
        assert!(block.starts_with("Stilprofil der Therapeutin (aus 2 früheren Schreiben"));
        assert!(block.contains("Ich-Form") && block.contains("Pat.") && block.contains("Hinweise der Therapeutin: kurz halten"));
        assert!(block.contains("keine Abkürzungen außer Pat."));
        assert_eq!(prompt_block(None, None, Some("  ")), "");
        assert!(prompt_block(None, Some("knapp"), None).contains("aus ihren Hinweisen"));
        let bullets = vec![exemplar("1", "drv-befund", "Befund\n- müde\n- gedrückt\n- Schlaf gestört\nText.")];
        assert_eq!(compute(&bullets, &t).unwrap().form, Form::Stichpunkte);
        assert!(compute(&[], &t).is_none());
    }

    #[test]
    fn material_goes_per_section_or_whole() {
        let t = template("drv-befund");
        let plain = "Sehr geehrte Damen und Herren,\nich berichte über Frau [Name]. Sie ist seit [Datum] in Behandlung und es geht ihr besser.";
        let ex = vec![exemplar("1", "drv-befund", DRV_A), exemplar("2", "drv-befund", DRV_B), exemplar("3", "drv-befund", plain), exemplar("4", "arztbrief", DRV_A)];
        let m = material(&ex, &t, None, None, true);
        assert_eq!(m.exemplars, 3);
        assert_eq!(m.whole.len(), 1, "only the one without headings goes whole");
        assert_eq!(m.sections.get("befund").map(Vec::len), Some(2));
        assert!(m.section("befund").contains("Stilbeispiele für diesen Abschnitt") && m.section("befund").contains("deutlich erschöpft"));
        assert!(m.section("befund").contains("Ihre übliche Länge für diesen Abschnitt: etwa"));
        assert_eq!(m.section("somatik"), "", "nothing for a section she never wrote");
        assert!(!m.profile.is_empty());
        let letter = material(&ex, &t, None, None, false);
        assert_eq!((letter.whole.len(), letter.sections.len()), (2, 0), "a letter takes the newest two whole");
        let long = "Satz mit einigen Wörtern. ".repeat(200);
        assert!(section_block(&[("x".into(), long)], None).chars().count() < SECTION_CHARS + 400);
        assert!(clean_notes(&"x".repeat(MAX_NOTES_CHARS + 1)).is_err());
        assert_eq!(without_diagnoses("Text.\nDiagnosen:\nF32.1 G\nF41.0 V\n\nWeiter.\nF43.2: Anpassung\nFazit."), "Text.\n\nWeiter.\nFazit.");
        assert_eq!(without_diagnoses("F-Skala unauffällig. Fünf Termine."), "F-Skala unauffällig. Fünf Termine.");
    }
}
