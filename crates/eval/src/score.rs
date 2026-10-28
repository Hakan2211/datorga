//! Scoring against the golden set. Deterministic (regex matching), no model as judge, so a score
//! change means the model or prompt changed, not the grader.

use std::collections::BTreeSet;

use serde::Serialize;
use thera_core::extract::DraftFact;

use crate::suite::{Case, Episode, Suite};

// ---------- extraction ----------

#[derive(Debug, Default, Clone, Serialize)]
pub struct ExtractScore {
    /// Required expected facts.
    pub expected: usize,
    /// Required expected facts that some proposal matches.
    pub found: usize,
    /// Of `found`: filed under an accepted section.
    pub section_ok: usize,
    /// Of `found`: citing at least one of the expected segments.
    pub citation_ok: usize,
    pub proposed: usize,
    /// Proposals that match no expected fact (required or not). Not necessarily wrong: listed for review.
    pub extra: Vec<String>,
    /// Proposals hitting a `forbid` pattern (invented or off-limits content).
    pub forbidden: Vec<String>,
    pub missed: Vec<String>,
    /// `fact id → section it was filed under`.
    pub misfiled: Vec<String>,
    /// Entries the parser dropped.
    pub dropped: usize,
    /// Repeated proposals folded by `extract::merge` before scoring.
    pub merged: usize,
    /// Expected facts (required or not) that more than one proposal matches: duplicates the
    /// merge missed.
    pub duplicates: Vec<String>,
    /// Bericht tags on the best match of each found fact, and how many of them are expected or
    /// acceptable for it (tag precision).
    pub tags: usize,
    pub tags_ok: usize,
    /// Expected Bericht sections of the found facts, and how many their best match names (tag recall).
    pub tags_expected: usize,
    pub tags_recalled: usize,
    /// `fact id → tag` for tags outside the expected and acceptable sections.
    pub wrong_tags: Vec<String>,
    /// Per expected fact of the case: the index of its best-placed proposal.
    #[serde(skip)]
    pub matched: Vec<Option<usize>>,
}

impl ExtractScore {
    pub fn add(&mut self, o: &ExtractScore) {
        self.expected += o.expected;
        self.found += o.found;
        self.section_ok += o.section_ok;
        self.citation_ok += o.citation_ok;
        self.proposed += o.proposed;
        self.dropped += o.dropped;
        self.merged += o.merged;
        self.tags += o.tags;
        self.tags_ok += o.tags_ok;
        self.tags_expected += o.tags_expected;
        self.tags_recalled += o.tags_recalled;
        self.duplicates.extend(o.duplicates.iter().cloned());
        self.wrong_tags.extend(o.wrong_tags.iter().cloned());
        self.extra.extend(o.extra.iter().cloned());
        self.forbidden.extend(o.forbidden.iter().cloned());
        self.missed.extend(o.missed.iter().cloned());
        self.misfiled.extend(o.misfiled.iter().cloned());
    }
}

/// One proposal may cover several expected facts (a model that merges "gedrückt, angespannt"
/// into one line still found both); each expected fact takes its best-placed match.
pub fn extraction(case: &Case, proposed: &[DraftFact], dropped: usize, merged: usize) -> ExtractScore {
    let mut s = ExtractScore { proposed: proposed.len(), dropped, merged, ..Default::default() };
    let mut used = vec![false; proposed.len()];
    let matches = |f: &crate::suite::Fact, i: usize| f.patterns.iter().all(|p| p.is_match(&proposed[i].text));
    // Which expected facts each proposal matches: two proposals with the same set, filed under the
    // same note section, say the same point twice ("Klassenlehrerin …" also matches the "Klasse"
    // fact, so it is no duplicate of "vierte Klasse").
    let sig: Vec<Vec<usize>> = (0..proposed.len()).map(|i| (0..case.facts.len()).filter(|&k| matches(&case.facts[k], i)).collect()).collect();
    for f in &case.facts {
        let hits: Vec<usize> = (0..proposed.len()).filter(|&i| matches(f, i)).collect();
        for &i in &hits {
            used[i] = true;
        }
        let twice = |n: usize, i: usize| {
            hits[..n].iter().any(|&j| proposed[j].section == proposed[i].section && sig[j] == sig[i] && similar(&proposed[j].text, &proposed[i].text))
        };
        if hits.iter().enumerate().any(|(n, &i)| twice(n, i)) {
            s.duplicates.push(f.def.id.clone());
        }
        let score = |i: &usize| {
            let p = &proposed[*i];
            (f.sections.contains(&p.section), p.segments.iter().any(|x| f.segments.contains(x)))
        };
        let best = hits.iter().max_by_key(|i| score(i)).copied();
        s.matched.push(best);
        if !f.def.required {
            continue;
        }
        s.expected += 1;
        let Some(best) = best.as_ref() else {
            s.missed.push(f.def.id.clone());
            continue;
        };
        s.found += 1;
        let tags = &proposed[*best].bericht;
        s.tags += tags.len();
        for t in tags {
            if f.def.bericht.contains(t) || f.def.bericht_ok.contains(t) {
                s.tags_ok += 1;
            } else {
                s.wrong_tags.push(format!("{} → {t}", f.def.id));
            }
        }
        s.tags_expected += f.def.bericht.len();
        s.tags_recalled += f.def.bericht.iter().filter(|b| tags.contains(b)).count();
        let (section, citation) = score(best);
        if section {
            s.section_ok += 1;
        } else {
            s.misfiled.push(format!("{} → {}", f.def.id, proposed[*best].section));
        }
        s.citation_ok += citation as usize;
    }
    for (i, p) in proposed.iter().enumerate() {
        if !used[i] {
            s.extra.push(format!("[{}] {}", p.section, p.text));
        }
        if case.forbid.iter().any(|r| r.is_match(&p.text)) {
            s.forbidden.push(format!("[{}] {}", p.section, p.text));
        }
    }
    s
}

/// Half or more of the shorter text's longer words recur in the other: the same statement, not
/// two halves of one fact ("lebt mit Mutter und Schwester" / "Eltern getrennt").
fn similar(a: &str, b: &str) -> bool {
    let words = |t: &str| -> BTreeSet<String> {
        t.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() > 3).map(|w| w.chars().take(5).collect()).collect()
    };
    let (a, b) = (words(a), words(b));
    let small = a.len().min(b.len());
    small > 0 && a.intersection(&b).count() * 2 >= small
}

// ---------- drafting ----------

/// An input fact of an episode: which case and fact it is.
#[derive(Debug, Clone)]
pub struct EpisodeFact {
    pub case: usize,
    pub fact: usize,
    pub session_no: usize,
}

pub fn episode_facts(suite: &Suite, ep: &Episode) -> Vec<EpisodeFact> {
    let mut out = Vec::new();
    for (n, &c) in ep.sessions.iter().enumerate() {
        for f in 0..suite.cases[c].facts.len() {
            out.push(EpisodeFact { case: c, fact: f, session_no: n + 1 });
        }
    }
    out
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct SectionScore {
    pub section: String,
    /// Facts expected in this section.
    pub relevant: usize,
    /// Distinct facts the kept sentences cite.
    pub cited: usize,
    /// Cited facts that belong here (expected or acceptable).
    pub correct: usize,
    /// Expected facts that are cited.
    pub recalled: usize,
    pub sentences: usize,
    /// Sentences in a section with no relevant fact at all (padding).
    pub padding: usize,
    pub wrong: Vec<String>,
    pub missed: Vec<String>,
    pub forbidden: Vec<String>,
    pub dropped: usize,
    pub text: Vec<String>,
}

impl SectionScore {
    pub fn add(&mut self, o: &SectionScore) {
        self.relevant += o.relevant;
        self.cited += o.cited;
        self.correct += o.correct;
        self.recalled += o.recalled;
        self.sentences += o.sentences;
        self.padding += o.padding;
        self.dropped += o.dropped;
        self.wrong.extend(o.wrong.iter().cloned());
        self.missed.extend(o.missed.iter().cloned());
        self.forbidden.extend(o.forbidden.iter().cloned());
    }
}

fn fact_id(suite: &Suite, f: &EpisodeFact) -> String {
    format!("{}/{}", suite.cases[f.case].id, suite.cases[f.case].facts[f.fact].def.id)
}

/// `sentences`: (text, cited indices into `input`). `input` is what the model saw for this section.
pub fn section(
    suite: &Suite,
    ep: &Episode,
    key: &str,
    all: &[EpisodeFact],
    input: &[usize],
    sentences: &[(String, Vec<usize>)],
    dropped: usize,
) -> SectionScore {
    let fact = |f: &EpisodeFact| &suite.cases[f.case].facts[f.fact].def;
    let relevant: Vec<usize> = (0..all.len()).filter(|&i| fact(&all[i]).bericht.iter().any(|b| b == key)).collect();
    let cited: BTreeSet<usize> = sentences.iter().flat_map(|(_, c)| c.iter().map(|&i| input[i])).collect();
    let mut s = SectionScore {
        section: key.to_string(),
        relevant: relevant.len(),
        cited: cited.len(),
        sentences: sentences.len(),
        dropped,
        text: sentences.iter().map(|(t, _)| t.clone()).collect(),
        ..Default::default()
    };
    for &i in &cited {
        let d = fact(&all[i]);
        if d.bericht.iter().chain(&d.bericht_ok).any(|b| b == key) {
            s.correct += 1;
        } else {
            s.wrong.push(fact_id(suite, &all[i]));
        }
    }
    for &i in &relevant {
        if cited.contains(&i) {
            s.recalled += 1;
        } else {
            s.missed.push(fact_id(suite, &all[i]));
        }
    }
    if relevant.is_empty() {
        s.padding = sentences.len();
    }
    for (t, _) in sentences {
        if ep.forbid.iter().any(|r| r.is_match(t)) {
            s.forbidden.push(t.clone());
        }
    }
    s
}

/// What a source mapping would do before any model runs: relevant facts it keeps per section
/// and unrelated facts it removes. A mapping that drops relevant facts caps recall.
#[derive(Debug, Default, Clone, Serialize)]
pub struct FilterScore {
    pub section: String,
    pub relevant: usize,
    pub relevant_kept: usize,
    pub unrelated: usize,
    pub unrelated_removed: usize,
}

pub fn filter(suite: &Suite, key: &str, all: &[EpisodeFact], kept: &[usize]) -> FilterScore {
    let mut s = FilterScore { section: key.to_string(), ..Default::default() };
    for (i, f) in all.iter().enumerate() {
        let d = &suite.cases[f.case].facts[f.fact].def;
        let keep = kept.contains(&i);
        if d.bericht.iter().any(|b| b == key) {
            s.relevant += 1;
            s.relevant_kept += keep as usize;
        } else if !d.bericht_ok.iter().any(|b| b == key) {
            s.unrelated += 1;
            s.unrelated_removed += !keep as usize;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suite;
    use std::path::Path;

    fn suite() -> Suite {
        suite::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("suites/de-therapy-v1")).unwrap()
    }

    fn gold(case: &Case) -> Vec<DraftFact> {
        case.facts
            .iter()
            .map(|f| DraftFact {
                section: f.sections[0].clone(),
                text: f.def.text.clone(),
                segments: f.segments.clone(),
                bericht: f.def.bericht.clone(),
            })
            .collect()
    }

    #[test]
    fn the_expected_facts_score_perfectly() {
        let s = suite();
        for c in &s.cases {
            let r = extraction(c, &gold(c), 0, 0);
            assert_eq!((r.found, r.section_ok, r.citation_ok), (r.expected, r.expected, r.expected), "{}", c.id);
            assert!(r.extra.is_empty() && r.forbidden.is_empty(), "{}: {:?} {:?}", c.id, r.extra, r.forbidden);
            assert_eq!((r.tags_ok, r.tags_recalled), (r.tags, r.tags_expected), "{}", c.id);
            assert!(r.duplicates.is_empty(), "{}: {:?}", c.id, r.duplicates);
        }
    }

    #[test]
    fn misses_misfiles_and_extras_are_counted() {
        let s = suite();
        let c = &s.cases[0];
        let mut facts = gold(c);
        let first = c.facts.iter().position(|f| f.def.required).unwrap();
        let other = c.note.sections.iter().map(|d| d.key.clone()).find(|k| !c.facts[first].sections.contains(k)).unwrap();
        facts[first].section = other;
        facts[first].segments = vec![c.segments.len() + 5];
        let second = c.facts.iter().enumerate().skip(first + 1).find(|(_, f)| f.def.required).unwrap().0;
        facts.remove(second);
        facts.push(DraftFact { section: "verlauf".into(), text: "Wetter war schön".into(), segments: vec![0], bericht: vec![] });
        // The same fact once more, and a wrong tag on another.
        let third = c.facts.iter().enumerate().skip(second + 1).find(|(_, f)| f.def.required && !f.def.bericht.is_empty()).unwrap().0;
        let at = |facts: &[DraftFact], k: usize| facts.iter().position(|f| f.text == c.facts[k].def.text).unwrap();
        let again = facts[at(&facts, third)].clone();
        facts.push(again);
        let fourth = c.facts.iter().enumerate().skip(third + 1).find(|(_, f)| f.def.required).unwrap().0;
        let i = at(&facts, fourth);
        facts[i].bericht.push("somatik_nope".into());
        let r = extraction(c, &facts, 2, 0);
        assert_eq!(r.duplicates, vec![c.facts[third].def.id.clone()]);
        assert_eq!(r.tags, r.tags_ok + 1);
        assert_eq!(r.wrong_tags, vec![format!("{} → somatik_nope", c.facts[fourth].def.id)]);
        assert_eq!(r.found, r.expected - 1);
        assert_eq!(r.section_ok, r.found - 1);
        assert_eq!(r.citation_ok, r.found - 1);
        assert_eq!(r.missed, vec![c.facts[second].def.id.clone()]);
        assert_eq!(r.extra.len(), 1);
        assert_eq!(r.dropped, 2);
    }

    #[test]
    fn section_scores_use_expected_and_acceptable_sections() {
        let s = suite();
        let ep = &s.episodes[0];
        let all = episode_facts(&s, ep);
        let input: Vec<usize> = (0..all.len()).collect();
        let def = |i: usize| &s.cases[all[i].case].facts[all[i].fact].def;
        let key = "somatik";
        let rel: Vec<usize> = input.iter().copied().filter(|&i| def(i).bericht.iter().any(|b| b == key)).collect();
        let off = input.iter().copied().find(|&i| !def(i).bericht.iter().chain(&def(i).bericht_ok).any(|b| b == key)).unwrap();

        let perfect = section(&s, ep, key, &all, &input, &[("Somatisch …".into(), rel.clone())], 0);
        assert_eq!((perfect.correct, perfect.recalled, perfect.cited), (rel.len(), rel.len(), rel.len()));

        let mut cited = vec![rel[0], off];
        cited.sort();
        let r = section(&s, ep, key, &all, &input, &[("a".into(), cited)], 1);
        assert_eq!((r.cited, r.correct, r.recalled), (2, 1, 1));
        assert_eq!(r.wrong.len(), 1);
        assert_eq!(r.missed.len(), rel.len() - 1);
    }
}
