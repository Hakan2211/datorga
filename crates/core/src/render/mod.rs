//! Report export: one neutral document model, rendered to PDF (typst) and DOCX (docx-rs).

pub mod richtext;

use serde::Serialize;
use typst::foundations::{Dict, IntoValue};
use typst_as_lib::typst_kit_options::TypstKitFontOptions;
use typst_as_lib::TypstEngine;

use crate::domain::{Application, ApplicationKind, Diagnosis, Episode, Patient, Report};
use crate::pii::DiagnosesBody;
use crate::templates::{FieldKind, Template};
use crate::{Error, Result};
use richtext::Block;

const BERICHT_TYP: &str = include_str!("bericht.typ");

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
    practice: Option<String>,
) -> RenderDoc {
    let sections = template
        .sections
        .iter()
        .map(|def| {
            let body = report.sections.iter().find(|s| s.key == def.key).map(|s| s.body.as_str()).unwrap_or("");
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

    let subtitle = match application.map(|a| (a.kind, a.seq)) {
        Some((ApplicationKind::Erstantrag, _)) => "Bericht zum Erstantrag".to_string(),
        Some((ApplicationKind::Umwandlung, _)) => "Bericht zum Umwandlungsantrag".to_string(),
        Some((ApplicationKind::Fortfuehrung, n)) => format!("Bericht Nr. {n} zum Fortführungsantrag"),
        None => template.title.clone(),
    };
    let date_src = report.signed_at.as_deref().unwrap_or(&report.updated_at);

    RenderDoc {
        title: "Bericht an die Gutachterin / den Gutachter".into(),
        subtitle,
        chiffre: patient.chiffre.clone().unwrap_or_else(|| "—".into()),
        method: episode.map(|e| e.method.as_str().to_string()),
        report_no: application.filter(|a| a.kind == ApplicationKind::Fortfuehrung).map(|a| a.seq),
        date: german_date(date_src),
        signed_by: report.signed_by.clone(),
        practice,
        sections,
    }
}

pub fn pdf(doc: &RenderDoc) -> Result<Vec<u8>> {
    let engine = TypstEngine::builder()
        .main_file(BERICHT_TYP)
        .search_fonts_with(TypstKitFontOptions::default().include_system_fonts(false).include_embedded_fonts(true))
        .build();
    let mut input = Dict::new();
    input.insert("data".into(), serde_json::to_string(doc)?.into_value());
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
        for b in &s.blocks {
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
            kind: app.kind,
        });
        (build_doc(&report, &t, &p, Some(&ep), Some(&app), Some("Praxis Muster · Musterstr. 1".into())), report)
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
    fn renders_docx() {
        let (doc, _) = signed_fixture();
        let bytes = docx(&doc).unwrap();
        assert!(bytes.starts_with(b"PK"), "docx is a zip");
        if let Ok(dir) = std::env::var("THERA_RENDER_OUT") {
            std::fs::write(format!("{dir}/bericht-test.docx"), &bytes).unwrap();
        }
    }
}
