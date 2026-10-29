//! Identity leakage checks for reports that go to a third party under a Chiffre (PTV 3),
//! plus the pre-sign checklist. The full token-based pseudonymizer for cloud calls
//! (`Pseudonymized<T>`) builds on `plain_text` and the patient identifiers here.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::{Diagnosis, Patient, Report};
use crate::templates::{FieldKind, Template};

/// Body of a `diagnoses` section, stored as JSON in `report_section.body`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiagnosesBody {
    #[serde(default)]
    pub diagnoses: Vec<Diagnosis>,
    /// TipTap HTML: psychodynamic diagnosis, differential diagnosis, etc.
    #[serde(default)]
    pub notes: String,
}

/// Strips tags and decodes the few entities TipTap emits. Good enough for scanning and counting.
pub fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Plain text of a section regardless of field kind.
pub fn section_text(field: FieldKind, body: &str) -> String {
    match field {
        FieldKind::Prose => plain_text(body),
        FieldKind::Diagnoses => {
            let d: DiagnosesBody = serde_json::from_str(body).unwrap_or_default();
            let list = d
                .diagnoses
                .iter()
                .map(|d| format!("{} {} ({})", d.code, d.label, d.certainty))
                .collect::<Vec<_>>()
                .join("; ");
            format!("{list} {}", plain_text(&d.notes)).trim().to_string()
        }
    }
}

/// Identifiers of the patient that must not appear in a pseudonymized report.
pub fn identifiers(p: &Patient) -> Vec<String> {
    let mut ids = vec![p.last_name.clone()];
    if p.first_name.chars().count() >= 3 {
        ids.push(p.first_name.clone());
    }
    if let Some(d) = &p.birth_date {
        if let [y, m, day] = d.split('-').collect::<Vec<_>>()[..] {
            let (mi, di) = (m.trim_start_matches('0'), day.trim_start_matches('0'));
            ids.extend([d.clone(), format!("{day}.{m}.{y}"), format!("{di}.{mi}.{y}"), format!("{day}.{m}.{}", &y[2..])]);
        }
    }
    ids.retain(|s| !s.trim().is_empty());
    ids
}

/// Whole-word, case-insensitive occurrences of patient identifiers.
pub fn find_identifiers(text: &str, ids: &[String]) -> Vec<String> {
    let lower = text.to_lowercase();
    ids.iter()
        .filter(|id| {
            let needle = id.to_lowercase();
            lower.match_indices(&needle).any(|(i, _)| {
                let before = lower[..i].chars().next_back();
                let after = lower[i + needle.len()..].chars().next();
                !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
            })
        })
        .cloned()
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CheckItem {
    pub key: String,
    pub ok: bool,
    /// "block" prevents signing; "warn" is advisory.
    pub severity: String,
    pub message: String,
    pub section_key: Option<String>,
}

/// Roughly one A4 page of 11pt running text with headings.
pub const CHARS_PER_PAGE: usize = 3300;

/// Pre-sign checklist. `template` must already be resolved for the episode context.
pub fn checklist(report: &Report, template: &Template, patient: &Patient) -> Vec<CheckItem> {
    let mut items = Vec::new();
    let ids = identifiers(patient);
    let mut total = 0usize;

    for def in &template.sections {
        let section = report.sections.iter().find(|s| s.key == def.key);
        // Left out of the export, so nothing to check.
        if def.optional && section.is_some_and(|s| s.not_relevant) {
            continue;
        }
        let body = section.map(|s| s.body.as_str()).unwrap_or("");
        let text = section_text(def.field, body);
        total += text.chars().count();
        let label = def.number.clone().unwrap_or_else(|| def.key.clone());

        if def.required {
            items.push(CheckItem {
                key: format!("filled:{}", def.key),
                ok: !text.is_empty(),
                severity: "block".into(),
                message: match &def.number {
                    Some(n) => format!("Abschnitt {n} ausgefüllt"),
                    None => format!("{} ausgefüllt", def.heading),
                },
                section_key: Some(def.key.clone()),
            });
        }
        if template.rules.pseudonymized {
            let hits = find_identifiers(&text, &ids);
            items.push(CheckItem {
                key: format!("pseudonym:{}", def.key),
                ok: hits.is_empty(),
                severity: "block".into(),
                message: if hits.is_empty() {
                    format!("Abschnitt {label}: keine Klarnamen/Geburtsdaten")
                } else {
                    format!("Abschnitt {label} enthält identifizierende Angaben: {}", hits.join(", "))
                },
                section_key: Some(def.key.clone()),
            });
        }
    }

    if template.rules.pseudonymized {
        let has = patient.chiffre.as_deref().is_some_and(|c| !c.trim().is_empty());
        items.push(CheckItem {
            key: "chiffre".into(),
            ok: has,
            severity: "block".into(),
            message: "Chiffre für den Gutachter hinterlegt".into(),
            section_key: None,
        });
    }
    if let Some(pages) = template.rules.target_pages {
        let est = total as f32 / CHARS_PER_PAGE as f32;
        items.push(CheckItem {
            key: "length".into(),
            ok: est <= pages as f32 * 1.25,
            severity: "warn".into(),
            message: format!("Umfang ca. {est:.1} Seiten (Richtwert: {pages})"),
            section_key: None,
        });
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patient() -> Patient {
        Patient {
            id: "p".into(),
            external_ref: None,
            first_name: "Anna".into(),
            last_name: "Beispiel".into(),
            birth_date: Some("1985-03-04".into()),
            chiffre: Some("B040385".into()),
            pronouns: None,
            gender: None,
            consent_recording: false,
            status: "active".into(),
            created_at: String::new(),
            archived_at: None,
        }
    }

    #[test]
    fn finds_names_and_dates_in_html() {
        let ids = identifiers(&patient());
        let text = plain_text("<p>Frau <strong>Beispiel</strong>, geb. 4.3.1985, berichtet…</p>");
        let hits = find_identifiers(&text, &ids);
        assert!(hits.contains(&"Beispiel".to_string()));
        assert!(hits.contains(&"4.3.1985".to_string()));
        // Whole words only: "Annahme" must not match "Anna".
        assert!(find_identifiers("Annahme bestätigt", &ids).is_empty());
    }
}
