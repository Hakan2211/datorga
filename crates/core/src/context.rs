//! Context about a patient from earlier sessions, for the voice prep (slice 26) and later the
//! agent. Built from signed Sitzungsnotizen and applied facts only (what the therapist approved).
//!
//! The patient is written "Pat."; sentences naming anyone or anything else identifying (a person,
//! place or employer, by the patient record, the per-patient terms or the rules) are dropped, not
//! tokenized, because a voice model would read placeholders aloud. Dates stay as `[DATUM_n]`.
//! The result is pseudonymized as one text and capped in length.

use serde::Serialize;
use ts_rs::TS;

use crate::domain::{EpisodeFact, Patient, Report};
use crate::draft::patient_as_pat;
use crate::extract::{sha256, Preview, TokenView};
use crate::pii::plain_text;
use crate::pseudo::{Dictionary, EntityKind, Pseudonymized, Pseudonymizer, TokenMap};
use crate::Result;

/// Sitzungsnotiz sections that go into the brief, with the label used there.
pub const NOTE_SECTIONS: &[(&str, &str)] = &[("verlauf", "Verlauf"), ("vereinbarungen", "Vereinbarungen und Planung")];
/// How many earlier notes are read.
pub const MAX_NOTES: usize = 3;
/// Newest applied facts of the episode.
pub const MAX_FACTS: usize = 40;
/// Roughly 3–4k tokens: plenty for a prep talk, far below GPT-Live's 16k instruction limit.
pub const MAX_CHARS: usize = 12_000;

/// What a brief was built from, shown with the preview.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct BriefStats {
    pub notes: usize,
    pub facts: usize,
    /// Sentences left out because they name someone or something identifying.
    pub dropped: usize,
    /// Sentences left out for length.
    pub cut: usize,
}

pub struct Brief {
    pub text: Pseudonymized<String>,
    pub map: TokenMap,
    pub stats: BriefStats,
    /// SHA-256 of the text: the start command refuses if it changed since the preview.
    pub digest: String,
}

impl Brief {
    pub fn empty(&self) -> bool {
        self.stats.notes == 0 && self.stats.facts == 0
    }

    pub fn preview(&self) -> Preview {
        Preview {
            messages: vec![self.text.get().clone()],
            tokens: self.map.entries().iter().map(TokenView::from).collect(),
            digest: self.digest.clone(),
        }
    }
}

const ORDINALS: &[&str] = &["Letzte Sitzung", "Vorletzte Sitzung", "Drittletzte Sitzung"];

/// `notes`: newest first (as [`crate::store::Store::prior_session_notes`] returns them).
/// `facts`: in session order (as `episode_facts` returns them).
pub fn prep_brief(patient: &Patient, dict: Dictionary, notes: &[(Option<String>, Report)], facts: &[EpisodeFact]) -> Result<Brief> {
    let mut p = Pseudonymizer::new(dict);
    let mut stats = BriefStats::default();
    let mut out = String::new();
    let mut budget = MAX_CHARS;

    // Adds the sentences of `text` that are safe, as long as the budget lasts.
    let keep = |text: &str, stats: &mut BriefStats, budget: &mut usize| -> String {
        let mut kept = String::new();
        for sentence in sentences(&patient_as_pat(text, patient)) {
            if p.detect(sentence).iter().any(|d| d.kind != EntityKind::Datum) {
                stats.dropped += 1;
            } else if sentence.len() + 1 > *budget {
                stats.cut += 1;
            } else {
                *budget -= sentence.len() + 1;
                kept.push_str(sentence);
                kept.push(' ');
            }
        }
        kept.trim_end().to_string()
    };

    for (i, (_, note)) in notes.iter().take(MAX_NOTES).enumerate() {
        let mut block = String::new();
        for (key, label) in NOTE_SECTIONS {
            let Some(section) = note.sections.iter().find(|s| s.key == *key) else { continue };
            let text = keep(&html_text(&section.body), &mut stats, &mut budget);
            if !text.is_empty() {
                block.push_str(&format!("{label}: {text}\n"));
            }
        }
        if !block.is_empty() {
            stats.notes += 1;
            out.push_str(&format!("{}:\n{block}\n", ORDINALS.get(i).unwrap_or(&"Frühere Sitzung")));
        }
    }

    let recent = &facts[facts.len().saturating_sub(MAX_FACTS)..];
    let mut lines = String::new();
    for f in recent.iter().rev() {
        let text = keep(&f.text, &mut stats, &mut budget);
        if !text.is_empty() {
            stats.facts += 1;
            lines.push_str(&format!("- {text}\n"));
        }
    }
    if !lines.is_empty() {
        out.push_str("Festgehaltene Punkte aus der bisherigen Behandlung (neueste zuerst):\n");
        out.push_str(&lines);
    }

    let raw = out.trim_end().to_string();
    let text = p.pseudonymize(&raw)?;
    let digest = sha256(text.get());
    Ok(Brief { text, map: p.into_map(), stats, digest })
}

/// Text of a TipTap section: paragraphs and list items become lines.
fn html_text(html: &str) -> String {
    let spaced = html.replace("</p>", "</p>\n").replace("</li>", "</li>\n").replace("<br>", "\n");
    plain_text(&spaced)
}

/// Sentences of `text`, cut after `.`, `!`, `?` followed by whitespace, and at line ends.
/// Abbreviations ("z. B.") may split a sentence; both halves are then checked on their own.
fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut start = 0;
        let bytes = line.as_bytes();
        for (i, c) in line.char_indices() {
            if matches!(c, '.' | '!' | '?') && bytes.get(i + 1).is_some_and(|b| b.is_ascii_whitespace()) {
                out.push(line[start..=i].trim());
                start = i + 1;
            }
        }
        out.push(line[start..].trim());
    }
    out.retain(|s| !s.is_empty());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ReportSection, SectionOrigin};
    use crate::pseudo::EntityKind;

    fn patient() -> Patient {
        serde_json::from_value(serde_json::json!({
            "id": "p1", "first_name": "Anna", "last_name": "Beispiel", "birth_date": "1990-03-04",
            "chiffre": "B040390", "pronouns": null, "consent_recording": false, "status": "active",
            "created_at": "2026-01-01T00:00:00Z"
        }))
        .unwrap()
    }

    fn note(verlauf: &str, vereinbarungen: &str) -> (Option<String>, Report) {
        let mut r: Report = serde_json::from_value(serde_json::json!({
            "id": "r1", "patient_id": "p1", "episode_id": null, "application_id": null, "session_id": "s0",
            "template_key": "sitzungsnotiz", "template_version": "2026.1", "kind": "sitzungsnotiz", "status": "signed",
            "created_at": "x", "updated_at": "x", "signed_at": "x", "signed_by": "T", "exported_at": null,
            "export_outdated": false, "sections": [], "addenda": []
        }))
        .unwrap();
        for (key, body) in [("verlauf", verlauf), ("vereinbarungen", vereinbarungen), ("befund", "<p>Geheim.</p>")] {
            r.sections.push(ReportSection { key: key.into(), body: body.into(), origin: SectionOrigin::Human, edited_at: None });
        }
        (Some("2026-10-20T10:00:00Z".into()), r)
    }

    fn fact(text: &str) -> EpisodeFact {
        EpisodeFact {
            fact_id: "f".into(), session_id: "s0".into(), note_id: "r1".into(), session_at: None, session_no: 1,
            section_key: "verlauf".into(), text: text.into(),
        }
    }

    #[test]
    fn brief_writes_pat_drops_third_parties_and_skips_other_sections() {
        let mut dict = Dictionary::from_patient(&patient());
        dict.add(EntityKind::Person, "Karl");
        let notes = [note(
            "<p>Frau Beispiel berichtet von besserem Schlaf. Ihr Mann Karl war krank.</p><p>Streit mit Frau Müller.</p>",
            "<ul><li>Schlaftagebuch weiterführen.</li></ul>",
        )];
        let b = prep_brief(&patient(), dict, &notes, &[fact("Anna hat Angst vor der Prüfung am 4. März 2026."), fact("Arbeitet bei der Firma Acme.")]).unwrap();
        let t = b.text.get();
        assert!(t.starts_with("Letzte Sitzung:\nVerlauf: Pat. berichtet von besserem Schlaf."), "{t}");
        assert!(t.contains("Vereinbarungen und Planung: Schlaftagebuch weiterführen."));
        assert!(!t.contains("Karl") && !t.contains("Müller") && !t.contains("Acme") && !t.contains("Geheim"), "{t}");
        assert!(t.contains("- Pat. hat Angst vor der Prüfung am [DATUM_1]."), "{t}");
        assert_eq!(b.stats, BriefStats { notes: 1, facts: 1, dropped: 3, cut: 0 });
        assert_eq!(b.map.restore(t).unknown.len(), 0);
        assert_eq!(b.preview().digest, b.digest);
    }

    #[test]
    fn brief_is_capped_and_can_be_empty() {
        let long = format!("<p>{}</p>", "Pat. übt täglich Entspannung. ".repeat(1000));
        let b = prep_brief(&patient(), Dictionary::from_patient(&patient()), &[note(&long, "")], &[]).unwrap();
        assert!(b.text.get().len() <= MAX_CHARS + 200);
        assert!(b.stats.cut > 0);
        let e = prep_brief(&patient(), Dictionary::new(), &[], &[]).unwrap();
        assert!(e.empty() && e.text.get().is_empty());
    }

    #[test]
    fn sentence_split() {
        assert_eq!(sentences("Eins. Zwei! Drei?\nVier"), vec!["Eins.", "Zwei!", "Drei?", "Vier"]);
    }
}
