//! Letters (slice 45): DIN 5008 Form B with the practice letterhead, address window, Betreff,
//! salutation, the sections, closing and signature block; the practice footer on every page.

use serde::Serialize;

use super::richtext;
use super::{german_date, german_datetime, RenderAddendum, RenderSection};
use crate::domain::Report;
use crate::letter::{Letter, Practice, RefLine};
use crate::templates::Template;
use crate::{Error, Result};

const BRIEF_TYP: &str = include_str!("brief.typ");

#[derive(Debug, Clone, Serialize)]
pub struct LetterDoc {
    pub subject: String,
    /// Header line of pages 2 and following.
    pub short_subject: String,
    pub practice_name: String,
    pub practice_subtitle: String,
    pub sender_line: String,
    pub footer: Vec<Vec<String>>,
    pub recipient: Vec<String>,
    /// "Musterstadt, den 07.11.2026"
    pub place_date: String,
    pub refs: Vec<RefLine>,
    pub salutation: String,
    pub sections: Vec<RenderSection>,
    pub closing: Option<String>,
    pub signer: Option<String>,
    pub titles: Vec<String>,
    pub signed: bool,
    pub addenda: Vec<RenderAddendum>,
    /// Images, not serialized into the JSON: handed to typst as bytes.
    #[serde(skip)]
    pub logo: Option<Vec<u8>>,
    #[serde(skip)]
    pub signature: Option<Vec<u8>>,
}

/// Everything around the letter that isn't in the report: the practice, its images, and the
/// name to print while the letter is still unsigned.
pub struct Letterhead {
    pub practice: Practice,
    pub logo: Option<Vec<u8>>,
    pub signature: Option<Vec<u8>>,
    /// Settings: "Ihr Name" (the signature name before signing).
    pub therapist: Option<String>,
}

/// `template` must be resolved for the episode context. `today` (RFC 3339) dates an undated draft.
pub fn build_letter(report: &Report, template: &Template, letter: &Letter, head: Letterhead, today: &str) -> LetterDoc {
    let date = letter
        .letter_date
        .clone()
        .or_else(|| report.signed_at.clone())
        .unwrap_or_else(|| today.to_string());
    let date = german_date(&date);
    let place_date = match letter.place.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(place) => format!("{place}, den {date}"),
        None => date,
    };
    let signed = report.signed_by.is_some();
    let p = &head.practice;
    LetterDoc {
        subject: letter.subject.clone(),
        short_subject: letter.subject.chars().take(90).collect(),
        practice_name: p.name.trim().to_string(),
        practice_subtitle: p.subtitle.trim().to_string(),
        sender_line: p.sender_line(),
        footer: p.footer_columns(),
        recipient: letter.recipient.address_lines(),
        place_date,
        refs: letter.refs.iter().filter(|r| !r.value.trim().is_empty()).cloned().collect(),
        salutation: letter.salutation.clone(),
        sections: super::render_sections(report, template),
        closing: template.closing.clone(),
        signer: report.signed_by.clone().or(head.therapist.filter(|t| !t.trim().is_empty())),
        titles: p.titles(),
        signed,
        addenda: report
            .addenda
            .iter()
            .map(|a| RenderAddendum {
                heading: format!("Nachtrag vom {} – {}", german_datetime(&a.created_at), a.signed_by),
                blocks: richtext::parse(&a.body),
            })
            .collect(),
        logo: head.logo,
        // The signature image only on a signed letter, and only when she switched it on.
        signature: head.signature.filter(|_| signed && p.signature_image),
    }
}

fn images(doc: &LetterDoc) -> Vec<(&'static str, Vec<u8>)> {
    let mut out = Vec::new();
    if let Some(l) = &doc.logo {
        out.push(("logo", l.clone()));
    }
    if let Some(s) = &doc.signature {
        out.push(("signature", s.clone()));
    }
    out
}

pub fn letter_pdf(doc: &LetterDoc) -> Result<Vec<u8>> {
    let compiled = super::compile_with(BRIEF_TYP, doc, images(doc))?;
    typst_pdf::pdf(&compiled, &Default::default()).map_err(|e| Error::Render(format!("pdf: {e:?}")))
}

pub fn letter_preview_pngs(doc: &LetterDoc, scale: f32) -> Result<Vec<Vec<u8>>> {
    let opts = typst_render::RenderOptions { pixel_per_pt: f64::from(scale).into(), ..Default::default() };
    super::compile_with(BRIEF_TYP, doc, images(doc))?
        .pages()
        .iter()
        .map(|page| typst_render::render(page, &opts).encode_png().map_err(|e| Error::Render(format!("png: {e}"))))
        .collect()
}

/// A sample letter for the letterhead preview in the settings.
pub fn sample(head: Letterhead, today: &str) -> LetterDoc {
    let mut doc = LetterDoc {
        subject: "Beurteilung psychischer Erkrankung von Frau Erika Mustermann, geb. 01.01.1980".into(),
        short_subject: String::new(),
        practice_name: head.practice.name.trim().to_string(),
        practice_subtitle: head.practice.subtitle.trim().to_string(),
        sender_line: head.practice.sender_line(),
        footer: head.practice.footer_columns(),
        recipient: vec!["Deutsche Rentenversicherung".into(), "Musterstraße 1".into(), "12345 Musterstadt".into()],
        place_date: match head.practice.place_or_city() {
            Some(place) => format!("{place}, den {}", german_date(today)),
            None => german_date(today),
        },
        refs: vec![RefLine { label: "Versicherungsnummer".into(), value: "12 010180 M 123".into() }],
        salutation: crate::letter::DEFAULT_SALUTATION.into(),
        sections: vec![RenderSection {
            number: None,
            heading: String::new(),
            diagnoses: vec![],
            blocks: richtext::parse("<p>hier steht der Text des Briefes. Diese Vorschau zeigt nur den Briefkopf, die Anschrift und die Fußzeile mit Ihren Angaben.</p>"),
        }],
        closing: Some("Bei Fragen stehe ich Ihnen gerne zur Verfügung.".into()),
        signer: head.therapist.filter(|t| !t.trim().is_empty()),
        titles: head.practice.titles(),
        signed: false,
        addenda: vec![],
        logo: head.logo,
        signature: head.signature.filter(|_| head.practice.signature_image),
    };
    doc.short_subject = doc.subject.clone();
    doc
}

/// The letter as DOCX: letterhead and address at the top of page one, the practice footer on
/// every page. Positions follow DIN 5008 only roughly (Word places them by paragraph spacing).
pub fn letter_docx(doc: &LetterDoc) -> Result<Vec<u8>> {
    use docx_rs::{AlignmentType, Footer, LineSpacing, PageMargin, Paragraph, Pic, Run, Table, TableCell, TableRow};

    let mm = |v: f32| (v * 56.7).round() as i32; // twips
    let mut d = super::docx_base()
        .page_margin(PageMargin::new().top(mm(20.0)).bottom(mm(30.0)).left(mm(25.0)).right(mm(20.0)).footer(mm(10.0)));

    if let Some(logo) = &doc.logo {
        if let Ok(img) = image::load_from_memory(logo) {
            // 22 mm high, width by aspect; EMU = 36 000 per mm.
            let h = 22.0 * 36_000.0;
            let w = h * img.width() as f32 / img.height().max(1) as f32;
            let pic = Pic::new(logo).size(w.round() as u32, h.round() as u32);
            d = d.add_paragraph(Paragraph::new().align(AlignmentType::Right).add_run(Run::new().add_image(pic)));
        }
    }
    if !doc.practice_name.is_empty() {
        d = d.add_paragraph(Paragraph::new().align(AlignmentType::Center).add_run(Run::new().add_text(&doc.practice_name).bold().size(30)));
    }
    if !doc.practice_subtitle.is_empty() {
        d = d.add_paragraph(Paragraph::new().align(AlignmentType::Center).add_run(Run::new().add_text(&doc.practice_subtitle).size(19)));
    }
    let tight = || LineSpacing::new().before(0).after(0);
    d = d.add_paragraph(Paragraph::new().line_spacing(LineSpacing::new().before(mm(12.0) as u32)));
    if !doc.sender_line.is_empty() {
        d = d.add_paragraph(Paragraph::new().line_spacing(tight()).add_run(Run::new().add_text(&doc.sender_line).size(14).underline("single")));
    }
    d = d.add_paragraph(Paragraph::new().line_spacing(tight()));
    for l in &doc.recipient {
        d = d.add_paragraph(Paragraph::new().line_spacing(tight()).add_run(Run::new().add_text(l).size(22)));
    }
    d = d
        .add_paragraph(Paragraph::new().line_spacing(LineSpacing::new().before(mm(14.0) as u32)))
        .add_paragraph(Paragraph::new().align(AlignmentType::Right).add_run(Run::new().add_text(&doc.place_date).size(22)))
        .add_paragraph(Paragraph::new().add_run(Run::new().add_text(&doc.subject).bold().size(22)));
    for r in &doc.refs {
        d = d.add_paragraph(Paragraph::new().line_spacing(tight()).add_run(Run::new().add_text(format!("{}: {}", r.label, r.value)).size(22)));
    }
    d = d.add_paragraph(Paragraph::new()).add_paragraph(Paragraph::new().add_run(Run::new().add_text(&doc.salutation).size(22)));

    for s in &doc.sections {
        if !s.heading.is_empty() {
            let heading = match &s.number {
                Some(n) => format!("{n}. {}", s.heading),
                None => s.heading.clone(),
            };
            d = d.add_paragraph(Paragraph::new().keep_next(true).add_run(Run::new().add_text(heading).bold().size(22)));
        }
        if !s.diagnoses.is_empty() {
            let cell = |t: &str, bold: bool| {
                let r = Run::new().add_text(t).size(20);
                TableCell::new().add_paragraph(Paragraph::new().add_run(if bold { r.bold() } else { r }))
            };
            let mut rows = vec![TableRow::new(vec![cell("ICD-10", true), cell("Diagnose", true), cell("Sicherheit", true)])];
            rows.extend(s.diagnoses.iter().map(|x| TableRow::new(vec![cell(&x.code, false), cell(&x.label, false), cell(&x.certainty, false)])));
            d = d.add_table(Table::new(rows));
        }
        d = super::docx_blocks(d, &s.blocks);
    }
    d = d.add_paragraph(Paragraph::new());
    if let Some(c) = &doc.closing {
        d = d.add_paragraph(Paragraph::new().keep_next(true).add_run(Run::new().add_text(c).size(22)));
    }
    d = d.add_paragraph(Paragraph::new().keep_next(true).add_run(Run::new().add_text("Mit freundlichen Grüßen").size(22)));
    d = d.add_paragraph(Paragraph::new().keep_next(true).line_spacing(LineSpacing::new().before(mm(12.0) as u32)));
    if let Some(s) = &doc.signer {
        d = d.add_paragraph(Paragraph::new().keep_next(true).line_spacing(tight()).add_run(Run::new().add_text(s).size(22)));
    }
    for t in &doc.titles {
        d = d.add_paragraph(Paragraph::new().line_spacing(tight()).add_run(Run::new().add_text(t).size(19)));
    }
    for a in &doc.addenda {
        d = d.add_paragraph(Paragraph::new()).add_paragraph(Paragraph::new().keep_next(true).add_run(Run::new().add_text(&a.heading).bold().size(22)));
        d = super::docx_blocks(d, &a.blocks);
    }
    if !doc.footer.is_empty() {
        let line = doc.footer.iter().map(|c| c.join(" · ")).collect::<Vec<_>>().join("   |   ");
        d = d.footer(Footer::new().add_paragraph(Paragraph::new().align(AlignmentType::Center).add_run(Run::new().add_text(line).size(14))));
    }

    let mut buf = std::io::Cursor::new(Vec::new());
    d.build().pack(&mut buf).map_err(|e| Error::Render(format!("docx: {e}")))?;
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::*;
    use crate::letter::{LetterInput, LetterMeta, LetterRecipient};
    use crate::store::Store;

    fn logo() -> Vec<u8> {
        let img = image::RgbaImage::from_fn(400, 160, |x, _| image::Rgba([40, 90, 140, if x < 200 { 255 } else { 0 }]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
        crate::photo::normalize_logo(&out.into_inner()).unwrap()
    }

    #[test]
    fn renders_a_din_letter_with_letterhead_and_footer() {
        let store = Store::open_in_memory().unwrap();
        let p = store
            .create_patient(PatientInput {
                first_name: "Anna".into(),
                last_name: "Beispiel".into(),
                birth_date: Some("1985-04-03".into()),
                gender: Some(Gender::W),
                ..Default::default()
            })
            .unwrap();
        store.create_episode(EpisodeInput { patient_id: p.id.clone(), method: Method::Vt, age_group: AgeGroup::Adult, setting: Setting::Einzel }).unwrap();
        let l = store.create_letter(LetterInput { patient_id: p.id.clone(), template_key: "bescheinigung".into(), episode_id: None, recipient_id: None }).unwrap();
        let meta = LetterMeta {
            episode_id: l.episode_id.clone(),
            recipient: LetterRecipient { kind: None, name: "Firma Muster GmbH".into(), department: "Personalabteilung".into(), lines: vec!["Werkstraße 5".into(), "12345 Musterstadt".into()] },
            salutation: l.salutation.clone(),
            subject: l.subject.clone(),
            refs: vec![RefLine { label: "Personalnummer".into(), value: "0815".into() }],
            place: Some("Musterstadt".into()),
            letter_date: Some("2026-11-07".into()),
            ..Default::default()
        };
        let l = store.save_letter_meta(&l.report_id, meta).unwrap();
        let report = store.sign_report(&l.report_id, "Dipl.-Psych. M. Muster").unwrap();
        let t = store.template_at(&report.template_key, &report.template_version).unwrap();
        let practice = Practice {
            name: "Praxis für Psychotherapie".into(),
            subtitle: "Dipl.-Psych. M. Muster".into(),
            street: "Musterstraße 1".into(),
            postal_code: "12345".into(),
            city: "Musterstadt".into(),
            phone: "0123 456".into(),
            iban: "DE00 1234 5678".into(),
            tax_no: "12/345/67890".into(),
            title_lines: "Diplom-Psychologin\nPsychologische Psychotherapeutin (VT)".into(),
            ..Default::default()
        };
        let head = Letterhead { practice, logo: Some(logo()), signature: Some(logo()), therapist: None };
        let doc = build_letter(&report, &t, &l, head, "2026-11-08T10:00:00Z");
        assert_eq!(doc.place_date, "Musterstadt, den 07.11.2026");
        assert_eq!(doc.recipient, ["Firma Muster GmbH", "Personalabteilung", "Werkstraße 5", "12345 Musterstadt"]);
        assert_eq!(doc.sender_line, "Praxis für Psychotherapie · Musterstraße 1 · 12345 Musterstadt");
        assert_eq!(doc.footer.len(), 4);
        assert_eq!(doc.titles.len(), 2);
        assert!(doc.signature.is_none(), "the signature image is off by default");
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].heading, "");
        let pdf = letter_pdf(&doc).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        let pages = letter_preview_pngs(&doc, 1.0).unwrap();
        assert_eq!(pages.len(), 1);
        let docx = letter_docx(&doc).unwrap();
        assert!(docx.starts_with(b"PK"));
        if let Ok(dir) = std::env::var("THERA_RENDER_OUT") {
            std::fs::write(format!("{dir}/brief-test.pdf"), &pdf).unwrap();
            std::fs::write(format!("{dir}/brief-test.docx"), &docx).unwrap();
        }
        // Without any practice data and images it still renders.
        let bare = Letterhead { practice: Practice::default(), logo: None, signature: None, therapist: Some("M. Muster".into()) };
        assert!(letter_pdf(&sample(bare, "2026-11-08T10:00:00Z")).unwrap().starts_with(b"%PDF"));
    }
}
