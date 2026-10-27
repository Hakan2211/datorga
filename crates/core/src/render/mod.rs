//! Report export: one neutral document model, rendered to PDF (typst) and DOCX (docx-rs).

pub mod richtext;

use serde::Serialize;
use typst::foundations::{Dict, IntoValue};
use typst_as_lib::typst_kit_options::TypstKitFontOptions;
use typst_as_lib::TypstEngine;

use crate::domain::{Application, ApplicationKind, Diagnosis, Episode, Patient, Report, Session};
use crate::pii::DiagnosesBody;
use crate::templates::{FieldKind, Template};
use crate::{Error, Result};
use richtext::Block;

const BERICHT_TYP: &str = include_str!("bericht.typ");
const NOTFALL_TYP: &str = include_str!("notfall.typ");

#[derive(Debug, Clone, Serialize)]
pub struct RenderDoc {
    pub title: String,
    pub subtitle: String,
    /// The Gutachter only ever sees the Chiffre, never the name.
    pub chiffre: String,
    pub method: Option<String>,
    pub report_no: Option<i64>,
    pub date: String,
    pub signed_by: Option<String>,
    pub practice: Option<String>,
    pub sections: Vec<RenderSection>,
    /// Nachträge, printed after the original and its signature.
    pub addenda: Vec<RenderAddendum>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderAddendum {
    /// "Nachtrag vom 27.10.2026, 14:05 – <signer>"
    pub heading: String,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderSection {
    pub number: Option<String>,
    pub heading: String,
    pub diagnoses: Vec<Diagnosis>,
    pub blocks: Vec<Block>,
}

/// `template` must be resolved for the episode context.
pub fn build_doc(
    report: &Report,
    template: &Template,
    patient: &Patient,
    episode: Option<&Episode>,
    application: Option<&Application>,
    session: Option<&Session>,
    practice: Option<String>,
) -> RenderDoc {
    let body_of = |key: &str| report.sections.iter().find(|s| s.key == key).map(|s| s.body.as_str()).unwrap_or("");
    let sections = template
        .sections
        .iter()
        // Optional sections left empty are omitted (Sitzungsnotiz); PTV 3 sections always print.
        .filter(|def| def.required || !crate::pii::section_text(def.field, body_of(&def.key)).is_empty())
        .map(|def| {
            let body = body_of(&def.key);
            let (diagnoses, html) = match def.field {
                FieldKind::Prose => (vec![], body.to_string()),
                FieldKind::Diagnoses => {
                    let d: DiagnosesBody = serde_json::from_str(body).unwrap_or_default();
                    (d.diagnoses, d.notes)
                }
            };
            RenderSection { number: def.number.clone(), heading: def.heading.clone(), diagnoses, blocks: richtext::parse(&html) }
        })
        .collect();

    let (title, subtitle) = if let Some(s) = session {
        let when = s.scheduled_at.as_deref().or(s.started_at.as_deref()).unwrap_or(&report.created_at);
        (template.title.clone(), format!("Sitzung vom {}", german_date(when)))
    } else {
        let subtitle = match application.map(|a| (a.kind, a.seq)) {
            Some((ApplicationKind::Erstantrag, _)) => "Bericht zum Erstantrag".to_string(),
            Some((ApplicationKind::Umwandlung, _)) => "Bericht zum Umwandlungsantrag".to_string(),
            Some((ApplicationKind::Fortfuehrung, n)) => format!("Bericht Nr. {n} zum Fortführungsantrag"),
            None => template.title.clone(),
        };
        ("Bericht an die Gutachterin / den Gutachter".to_string(), subtitle)
    };
    let date_src = report.signed_at.as_deref().unwrap_or(&report.updated_at);

    RenderDoc {
        title,
        subtitle,
        chiffre: patient.chiffre.clone().unwrap_or_else(|| "—".into()),
        method: episode.map(|e| e.method.as_str().to_string()),
        report_no: application.filter(|a| a.kind == ApplicationKind::Fortfuehrung).map(|a| a.seq),
        date: german_date(date_src),
        signed_by: report.signed_by.clone(),
        practice,
        sections,
        addenda: report
            .addenda
            .iter()
            .map(|a| RenderAddendum {
                heading: format!("Nachtrag vom {} – {}", german_datetime(&a.created_at), a.signed_by),
                blocks: richtext::parse(&a.body),
            })
            .collect(),
    }
}

pub fn pdf(doc: &RenderDoc) -> Result<Vec<u8>> {
    typst_pdf_from(BERICHT_TYP, doc)
}

/// The printable "Notfall-Kit". Holds the recovery key in clear text: written only where the
/// therapist chooses, never logged.
#[derive(Serialize)]
pub struct RecoveryKit {
    /// The 16 groups of [`crate::crypto::RecoveryKey::display`].
    pub groups: Vec<String>,
    /// German date, e.g. "27.10.2026, 14:05".
    pub created: String,
    pub reason: String,
    pub device: String,
    pub app_version: String,
    pub data_dir: String,
}

impl RecoveryKit {
    /// `created_at` is RFC 3339 (UTC); `rotated` = replaces an earlier key.
    pub fn new(key: &crate::crypto::RecoveryKey, created_at: &str, rotated: bool, device: String, app_version: String, data_dir: String) -> Self {
        Self {
            groups: key.display().split('-').map(str::to_string).collect(),
            created: german_datetime(created_at),
            reason: if rotated {
                "Neuer Schlüssel; ersetzt alle früheren Notfall-Kits".into()
            } else {
                "Ersteinrichtung".into()
            },
            device,
            app_version,
            data_dir,
        }
    }
}

pub fn recovery_kit_pdf(kit: &RecoveryKit) -> Result<Vec<u8>> {
    typst_pdf_from(NOTFALL_TYP, kit)
}

fn typst_pdf_from(main: &str, data: &impl Serialize) -> Result<Vec<u8>> {
    let engine = TypstEngine::builder()
        .main_file(main)
        .search_fonts_with(TypstKitFontOptions::default().include_system_fonts(false).include_embedded_fonts(true))
        .build();
    let mut input = Dict::new();
    input.insert("data".into(), serde_json::to_string(data)?.into_value());
    let compiled = engine
        .compile_with_input(input)
        .output
        .map_err(|e| Error::Render(format!("typst: {e:?}")))?;
    typst_pdf::pdf(&compiled, &Default::default()).map_err(|e| Error::Render(format!("pdf: {e:?}")))
}

pub fn docx(doc: &RenderDoc) -> Result<Vec<u8>> {
    use docx_rs::{AbstractNumbering, Docx, IndentLevel, Level, LevelJc, LevelText, NumberFormat, Numbering, NumberingId, Paragraph, Run, Start, Table, TableCell, TableRow};

    fn runs_para(runs: &[richtext::Run]) -> Paragraph {
        runs.iter().fold(Paragraph::new(), |p, r| {
            let mut run = Run::new().size(22);
            for (i, part) in r.text.split('\n').enumerate() {
                if i > 0 {
                    run = run.add_break(docx_rs::BreakType::TextWrapping);
                }
                run = run.add_text(part);
            }
            if r.bold {
                run = run.bold();
            }
            if r.italic {
                run = run.italic();
            }
            p.add_run(run)
        })
    }

    fn add_blocks(mut d: Docx, blocks: &[Block]) -> Docx {
        for b in blocks {
            match b {
                Block::P { runs } => d = d.add_paragraph(runs_para(runs)),
                Block::Ul { items } | Block::Ol { items } => {
                    let num = if matches!(b, Block::Ul { .. }) { 1 } else { 2 };
                    for it in items {
                        d = d.add_paragraph(runs_para(it).numbering(NumberingId::new(num), IndentLevel::new(0)));
                    }
                }
            }
        }
        d
    }

    let bullet = |id: usize, fmt: &str, text: &str| {
        AbstractNumbering::new(id).add_level(
            Level::new(0, Start::new(1), NumberFormat::new(fmt), LevelText::new(text), LevelJc::new("left"))
                .indent(Some(560), Some(docx_rs::SpecialIndentType::Hanging(280)), None, None),
        )
    };

    let mut d = Docx::new()
        .add_abstract_numbering(bullet(1, "bullet", "•"))
        .add_numbering(Numbering::new(1, 1))
        .add_abstract_numbering(bullet(2, "decimal", "%1."))
        .add_numbering(Numbering::new(2, 2));

    let header = format!(
        "Chiffre: {}{}{}",
        doc.chiffre,
        doc.method.as_ref().map(|m| format!("   Verfahren: {m}")).unwrap_or_default(),
        doc.report_no.map(|n| format!("   Bericht Nr. {n}")).unwrap_or_default()
    );
    d = d.header(docx_rs::Header::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text(header).size(17))));
    if let Some(p) = &doc.practice {
        d = d.add_paragraph(Paragraph::new().align(docx_rs::AlignmentType::Right).add_run(Run::new().add_text(p).size(18)));
    }
    d = d
        .add_paragraph(Paragraph::new().add_run(Run::new().add_text(&doc.title).bold().size(28)))
        .add_paragraph(Paragraph::new().add_run(Run::new().add_text(&doc.subtitle).size(21)));

    for s in &doc.sections {
        let heading = match &s.number {
            Some(n) => format!("{n}. {}", s.heading),
            None => s.heading.clone(),
        };
        d = d.add_paragraph(Paragraph::new().keep_next(true).add_run(Run::new().add_text(heading).bold().size(22)));
        if !s.diagnoses.is_empty() {
            let cell = |t: &str, bold: bool| {
                let r = Run::new().add_text(t).size(20);
                TableCell::new().add_paragraph(Paragraph::new().add_run(if bold { r.bold() } else { r }))
            };
            let mut rows = vec![TableRow::new(vec![cell("ICD-10", true), cell("Diagnose", true), cell("Sicherheit", true)])];
            rows.extend(s.diagnoses.iter().map(|x| TableRow::new(vec![cell(&x.code, false), cell(&x.label, false), cell(&x.certainty, false)])));
            d = d.add_table(Table::new(rows));
        }
        d = add_blocks(d, &s.blocks);
    }
    d = d.add_paragraph(Paragraph::new()).add_paragraph(Paragraph::new().add_run(
        Run::new()
            .add_text(format!(
                "Datum: {}        Unterschrift: {}",
                doc.date,
                doc.signed_by.clone().unwrap_or_else(|| "____________________".into())
            ))
            .size(20),
    ));
    for a in &doc.addenda {
        d = d.add_paragraph(Paragraph::new()).add_paragraph(
            Paragraph::new().keep_next(true).add_run(Run::new().add_text(&a.heading).bold().size(22)),
        );
        d = add_blocks(d, &a.blocks);
    }

    let mut buf = std::io::Cursor::new(Vec::new());
    d.build().pack(&mut buf).map_err(|e| Error::Render(format!("docx: {e}")))?;
    Ok(buf.into_inner())
}

fn german_date(rfc3339: &str) -> String {
    match rfc3339.get(..10).map(|s| s.split('-').collect::<Vec<_>>()) {
        Some(p) if p.len() == 3 => format!("{}.{}.{}", p[2], p[1], p[0]),
        _ => rfc3339.to_string(),
    }
}

/// Wall-clock time in Germany (CET/CEST, EU switch at 01:00 UTC on the last Sundays of March
/// and October), so a Nachtrag shows the time it was signed regardless of the machine's zone.
fn german_datetime(rfc3339: &str) -> String {
    let Some(t) = german_time(rfc3339) else { return rfc3339.to_string() };
    format!("{:02}.{:02}.{}, {:02}:{:02}", t.day(), u8::from(t.month()), t.year(), t.hour(), t.minute())
}

/// German local time as `20261027-143621`, for file and folder names.
pub fn file_stamp(rfc3339: &str) -> String {
    match german_time(rfc3339) {
        Some(t) => format!("{}{:02}{:02}-{:02}{:02}{:02}", t.year(), u8::from(t.month()), t.day(), t.hour(), t.minute(), t.second()),
        None => rfc3339.chars().filter(char::is_ascii_digit).collect(),
    }
}

fn german_time(rfc3339: &str) -> Option<time::OffsetDateTime> {
    use time::{format_description::well_known::Rfc3339, Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset, Weekday};
    let t = OffsetDateTime::parse(rfc3339, &Rfc3339).ok()?.to_offset(UtcOffset::UTC);
    let switch = |m: Month| {
        let mut d = Date::from_calendar_date(t.year(), m, 31).expect("March and October have 31 days");
        while d.weekday() != Weekday::Sunday {
            d = d.previous_day().expect("not the first day of the calendar");
        }
        PrimitiveDateTime::new(d, Time::from_hms(1, 0, 0).expect("valid time")).assume_utc()
    };
    let summer = t >= switch(Month::March) && t < switch(Month::October);
    Some(t.to_offset(UtcOffset::from_hms(if summer { 2 } else { 1 }, 0, 0).expect("valid offset")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::*;
    use crate::store::Store;
    use crate::templates::Context;

    fn signed_fixture() -> (RenderDoc, Report) {
        let store = Store::open_in_memory().unwrap();
        let p = store
            .create_patient(PatientInput {
                first_name: "Anna".into(),
                last_name: "Beispiel".into(),
                chiffre: Some("B040385".into()),
                ..Default::default()
            })
            .unwrap();
        let ep = store
            .create_episode(EpisodeInput { patient_id: p.id.clone(), method: Method::Vt, age_group: AgeGroup::Adult, setting: Setting::Einzel })
            .unwrap();
        let app = store.create_application(&ep.id, ApplicationKind::Erstantrag).unwrap();
        let rid = app.report_id.clone().unwrap();
        store.save_section(&rid, "soziodemographie", "<p>Bürokauffrau, verheiratet, <strong>2 Kinder</strong></p>").unwrap();
        store.save_section(&rid, "symptomatik_befund", "<ul><li><p>Panikattacken seit 2024</p></li><li><p>Vermeidung</p></li></ul>").unwrap();
        let diag = serde_json::to_string(&crate::pii::DiagnosesBody {
            diagnoses: vec![Diagnosis { code: "F41.0".into(), label: "Panikstörung".into(), certainty: "G".into() }],
            notes: "<p>DD: Agoraphobie ausgeschlossen</p>".into(),
        })
        .unwrap();
        store.save_section(&rid, "diagnose", &diag).unwrap();
        let report = store.sign_report(&rid, "Dipl.-Psych. M. Muster").unwrap();
        let t = store.template(&report.template_key).unwrap().resolve(&Context {
            method: ep.method,
            age_group: ep.age_group,
            kind: Some(app.kind),
        });
        (build_doc(&report, &t, &p, Some(&ep), Some(&app), None, Some("Praxis Muster · Musterstr. 1".into())), report)
    }

    #[test]
    fn renders_pdf_with_chiffre_and_without_name() {
        let (doc, _) = signed_fixture();
        assert_eq!(doc.chiffre, "B040385");
        assert!(!serde_json::to_string(&doc).unwrap().contains("Beispiel"));
        let pdf = pdf(&doc).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        if let Ok(dir) = std::env::var("THERA_RENDER_OUT") {
            std::fs::write(format!("{dir}/bericht-test.pdf"), &pdf).unwrap();
        }
    }

    #[test]
    fn session_note_has_own_title_and_skips_empty_optional_sections() {
        let store = Store::open_in_memory().unwrap();
        let p = store
            .create_patient(PatientInput { first_name: "Anna".into(), last_name: "Beispiel".into(), ..Default::default() })
            .unwrap();
        let s = store
            .create_session(SessionInput { patient_id: p.id.clone(), episode_id: None, scheduled_at: Some("2026-10-27T09:00:00Z".into()), modality: None })
            .unwrap();
        let note = store.create_session_note(&s.id).unwrap();
        store.save_section(&note.id, "risiko", "<p>Keine Suizidalität</p>").unwrap();
        let note = store.report(&note.id).unwrap();
        let t = store.template(&note.template_key).unwrap();
        let doc = build_doc(&note, &t, &p, None, None, Some(&store.session(&s.id).unwrap()), None);
        assert_eq!((doc.title.as_str(), doc.subtitle.as_str()), ("Sitzungsnotiz", "Sitzung vom 27.10.2026"));
        let headings: Vec<_> = doc.sections.iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(headings, ["Inhalt und Verlauf der Sitzung", "Risikoeinschätzung"]);
        assert!(pdf(&doc).unwrap().starts_with(b"%PDF"));
    }

    #[test]
    fn note_export_keeps_original_and_appends_addenda() {
        let store = Store::open_in_memory().unwrap();
        let p = store
            .create_patient(PatientInput { first_name: "Anna".into(), last_name: "Beispiel".into(), ..Default::default() })
            .unwrap();
        let s = store
            .create_session(SessionInput { patient_id: p.id.clone(), episode_id: None, scheduled_at: None, modality: None })
            .unwrap();
        let note = store.create_session_note(&s.id).unwrap();
        store.save_section(&note.id, "verlauf", "<p>Original</p>").unwrap();
        store.sign_report(&note.id, "M. Muster").unwrap();
        store.add_addendum(&note.id, "<p>Korrektur: <strong>Dienstag</strong></p>", "M. Muster").unwrap();
        let note = store.report(&note.id).unwrap();
        let t = store.template(&note.template_key).unwrap();
        let doc = build_doc(&note, &t, &p, None, None, Some(&store.session(&s.id).unwrap()), None);

        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].blocks, richtext::parse("<p>Original</p>"), "original as signed");
        assert_eq!(doc.signed_by.as_deref(), Some("M. Muster"));
        assert_eq!(doc.addenda.len(), 1);
        let h = &doc.addenda[0].heading;
        assert!(h.starts_with("Nachtrag vom ") && h.ends_with(" – M. Muster"), "{h}");
        assert!(serde_json::to_string(&doc.addenda).unwrap().contains("Dienstag"));
        assert!(pdf(&doc).unwrap().starts_with(b"%PDF"));
        assert!(docx(&doc).unwrap().starts_with(b"PK"));
    }

    #[test]
    fn recovery_kit_pdf_renders_key_groups() {
        let key = crate::crypto::RecoveryKey::generate().unwrap();
        let kit = RecoveryKit::new(&key, "2026-10-27T13:05:00Z", true, "PRAXIS-PC".into(), "0.1.0".into(), "C:/Users/x/AppData/Roaming/de.datorga.thera".into());
        assert_eq!(kit.groups.len(), 16);
        assert_eq!(kit.groups.join("-"), key.display());
        assert_eq!(kit.created, "27.10.2026, 14:05");
        let pdf = recovery_kit_pdf(&kit).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        if let Ok(dir) = std::env::var("THERA_RENDER_OUT") {
            std::fs::write(format!("{dir}/notfall-kit-test.pdf"), &pdf).unwrap();
        }
    }

    #[test]
    fn german_datetime_follows_cet_and_cest() {
        assert_eq!(german_datetime("2026-10-27T13:05:00Z"), "27.10.2026, 14:05");
        assert_eq!(german_datetime("2026-07-01T12:00:00Z"), "01.07.2026, 14:00");
        // EU switches: 2026-03-29 and 2026-10-25, both at 01:00 UTC.
        assert_eq!(german_datetime("2026-03-29T00:59:00Z"), "29.03.2026, 01:59");
        assert_eq!(german_datetime("2026-03-29T01:00:00Z"), "29.03.2026, 03:00");
        assert_eq!(german_datetime("2026-10-25T00:59:00Z"), "25.10.2026, 02:59");
        assert_eq!(german_datetime("2026-10-25T01:00:00Z"), "25.10.2026, 02:00");
        assert_eq!(file_stamp("2026-10-27T13:36:21.5Z"), "20261027-143621");
    }

    #[test]
    fn renders_docx() {
        let (doc, _) = signed_fixture();
        let bytes = docx(&doc).unwrap();
        assert!(bytes.starts_with(b"PK"), "docx is a zip");
        if let Ok(dir) = std::env::var("THERA_RENDER_OUT") {
            std::fs::write(format!("{dir}/bericht-test.docx"), &bytes).unwrap();
        }
    }
}
