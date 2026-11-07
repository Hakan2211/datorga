//! Reading an uploaded document as text, all locally (Phase 6, slice 49): a request
//! ("Anschreiben") for a letter, or one of her old letters as a style exemplar.
//!
//! - Text files (UTF-8, else Windows-1252) are read as they are.
//! - DOCX: the paragraphs of `word/document.xml`.
//! - PDF: the text layer (`pdf-extract`); a page with (almost) no text is a scan and goes
//!   through the Windows OCR ([`crate::ocr`]).
//! - Photos and scans (PNG, JPEG, TIFF, BMP, GIF, HEIC): Windows OCR.
//!
//! Nothing is kept: the caller stores the text (encrypted) and at most the file's SHA-256.

use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use ts_rs::TS;

use crate::extract::sha256_bytes;
use crate::ocr;
use crate::{Error, Result};

/// Largest file read.
pub const MAX_FILE: u64 = 40 * 1024 * 1024;
/// Most PDF pages read (and recognized) from one file.
pub const MAX_PAGES: u32 = 40;
/// A PDF page with fewer characters in its text layer counts as a scan.
const SCAN_PAGE_CHARS: usize = 40;
/// Less text than this means nothing readable was found.
const MIN_TEXT_CHARS: usize = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ImportMethod {
    /// A text file.
    Text,
    Docx,
    /// A PDF's text layer.
    Pdf,
    /// Recognized by OCR (a photo, a scan, or a PDF without text layer).
    Ocr,
    /// A PDF with text on some pages and scans on others.
    PdfOcr,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Imported {
    pub text: String,
    pub method: ImportMethod,
    /// Pages read: of a PDF, 1 for a photo or scan, 0 for text and Word files.
    pub pages: u32,
    /// Pages beyond [`MAX_PAGES`] that were left out.
    pub pages_skipped: u32,
    /// SHA-256 of the file (to recognize the same upload again).
    pub sha256: String,
    pub file_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Pdf,
    Docx,
    Image,
    Text,
}

fn kind_of(name: &str, bytes: &[u8]) -> Result<Kind> {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    if bytes.starts_with(b"%PDF") {
        return Ok(Kind::Pdf);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return if ext == "docx" || ext == "docm" || ext.is_empty() {
            Ok(Kind::Docx)
        } else {
            Err(Error::Invalid(format!("Dateien vom Typ .{ext} können nicht gelesen werden. Bitte als PDF oder DOCX speichern.")))
        };
    }
    let image = bytes.starts_with(&[0x89, b'P', b'N', b'G'])
        || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(b"II*\0")
        || bytes.starts_with(b"MM\0*")
        || bytes.starts_with(b"BM")
        || bytes.starts_with(b"GIF8")
        || (bytes.len() > 12 && &bytes[4..8] == b"ftyp" && matches!(&bytes[8..12], b"heic" | b"heix" | b"mif1" | b"msf1"));
    if image {
        return Ok(Kind::Image);
    }
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) || ext == "doc" || ext == "rtf" || ext == "odt" {
        return Err(Error::Invalid("Ältere Word-Dateien (.doc), RTF und ODT können nicht gelesen werden. Bitte in Word als PDF oder DOCX speichern.".into()));
    }
    if matches!(ext.as_str(), "txt" | "md" | "text" | "") || std::str::from_utf8(bytes).is_ok() {
        return Ok(Kind::Text);
    }
    Err(Error::Invalid(format!("Dateien vom Typ .{ext} können nicht gelesen werden (PDF, DOCX, TXT oder ein Foto/Scan).")))
}

/// Reads the file at `path`.
pub fn read(path: &Path) -> Result<Imported> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_FILE {
        return Err(Error::Invalid(format!("Die Datei ist zu groß (höchstens {} MB)", MAX_FILE / 1024 / 1024)));
    }
    let bytes = std::fs::read(path)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("Dokument");
    from_bytes(name, &bytes)
}

/// Reads an uploaded file's bytes; `name` gives the extension where the content doesn't.
pub fn from_bytes(name: &str, bytes: &[u8]) -> Result<Imported> {
    let (raw, method, pages, pages_skipped) = match kind_of(name, bytes)? {
        Kind::Text => (decode_text(bytes), ImportMethod::Text, 0, 0),
        Kind::Docx => (docx_text(bytes)?, ImportMethod::Docx, 0, 0),
        Kind::Image => (ocr::image(bytes)?.text, ImportMethod::Ocr, 1, 0),
        Kind::Pdf => pdf_text(bytes)?,
    };
    let text = tidy(&raw);
    if text.chars().filter(|c| c.is_alphanumeric()).count() < MIN_TEXT_CHARS {
        return Err(Error::Invalid("Im Dokument wurde kein lesbarer Text gefunden. Bitte den Text abtippen oder diktieren.".into()));
    }
    Ok(Imported { text, method, pages, pages_skipped, sha256: sha256_bytes(bytes), file_name: name.to_string() })
}

/// UTF-8 (with or without BOM), else Windows-1252 (old Notepad files).
fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| cp1252(b)).collect(),
    }
}

fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—',
        '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

static DOCX_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<w:t(?:\s[^>]*)?>(.*?)</w:t>|<w:(tab|br|cr)\b[^>]*/>|</w:p>").expect("valid regex"));

/// The text of a Word document: one line per paragraph.
fn docx_text(bytes: &[u8]) -> Result<String> {
    let bad = || Error::Invalid("Die Word-Datei kann nicht gelesen werden".into());
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| bad())?;
    let mut xml = String::new();
    zip.by_name("word/document.xml").map_err(|_| bad())?.take(64 * 1024 * 1024).read_to_string(&mut xml).map_err(|_| bad())?;
    let mut out = String::new();
    for c in DOCX_TOKEN.captures_iter(&xml) {
        if let Some(t) = c.get(1) {
            out.push_str(&xml_unescape(t.as_str()));
        } else if let Some(k) = c.get(2) {
            out.push(if k.as_str() == "tab" { '\t' } else { '\n' });
        } else {
            out.push('\n');
        }
    }
    Ok(out)
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The text layer page by page; pages without one are recognized by OCR.
fn pdf_text(bytes: &[u8]) -> Result<(String, ImportMethod, u32, u32)> {
    // pdf-extract panics on some broken files; treat that like a PDF without text.
    let layer: Vec<String> = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(bytes))
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or_default();
    let scanned = |t: &str| t.chars().filter(|c| c.is_alphanumeric()).count() < SCAN_PAGE_CHARS;
    if !layer.is_empty() && !layer.iter().any(|p| scanned(p)) {
        let n = layer.len() as u32;
        let skipped = n.saturating_sub(MAX_PAGES);
        return Ok((layer.into_iter().take(MAX_PAGES as usize).collect::<Vec<_>>().join("\n\n"), ImportMethod::Pdf, n.min(MAX_PAGES), skipped));
    }
    let mut pdf = match ocr::Pdf::open(bytes) {
        Ok(p) => p,
        // No OCR here: what the text layer has is still better than nothing.
        Err(e) if layer.iter().any(|p| !scanned(p)) => {
            let _ = e;
            return Ok((layer.join("\n\n"), ImportMethod::Pdf, layer.len() as u32, 0));
        }
        Err(e) => return Err(e),
    };
    let n = pdf.pages()?;
    let (mut parts, mut recognized) = (Vec::new(), 0);
    for i in 0..n.min(MAX_PAGES) {
        match layer.get(i as usize) {
            Some(t) if !scanned(t) => parts.push(t.clone()),
            _ => {
                parts.push(pdf.page(i)?.text);
                recognized += 1;
            }
        }
    }
    let method = if recognized as usize == parts.len() { ImportMethod::Ocr } else { ImportMethod::PdfOcr };
    Ok((parts.join("\n\n"), method, n.min(MAX_PAGES), n.saturating_sub(MAX_PAGES)))
}

// A word split at the line end ("Behand-\nlung"), not a dash list or a compound ("Ein- und").
static HYPHEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\p{Ll})-\n(\p{Ll}+)").expect("valid regex"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t\u{a0}]+").expect("valid regex"));
static BLANK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").expect("valid regex"));

/// Line endings, runs of spaces, words split at the line end, more than one blank line.
pub fn tidy(raw: &str) -> String {
    let text = raw.replace("\r\n", "\n").replace('\r', "\n").replace('\u{c}', "\n\n").replace('\u{ad}', "");
    let lines: Vec<String> = text.lines().map(|l| SPACES.replace_all(l, " ").trim().to_string()).collect();
    let text = lines.join("\n");
    let text = HYPHEN.replace_all(&text, |c: &regex::Captures| match &c[2] {
        "und" | "oder" | "bis" | "sowie" | "bzw" => format!("{}-\n{}", &c[1], &c[2]),
        _ => format!("{}{}", &c[1], &c[2]),
    });
    BLANK.replace_all(text.trim(), "\n\n").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docx(body: &str) -> Vec<u8> {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        z.start_file("[Content_Types].xml", opts).unwrap();
        z.write_all(b"<Types/>").unwrap();
        z.start_file("word/document.xml", opts).unwrap();
        z.write_all(format!(r#"<?xml version="1.0"?><w:document><w:body>{body}</w:body></w:document>"#).as_bytes()).unwrap();
        z.finish().unwrap();
        buf.into_inner()
    }

    #[test]
    fn reads_docx_paragraphs() {
        let bytes = docx(
            r#"<w:p><w:r><w:t>Sehr geehrte Damen und Herren,</w:t></w:r></w:p><w:p><w:r><w:t xml:space="preserve">Frau B. ist seit </w:t></w:r><w:r><w:t>2019 in Behandlung &amp; kommt wöchentlich.</w:t></w:r><w:r><w:br/></w:r><w:r><w:t>Zweite Zeile</w:t></w:r></w:p>"#,
        );
        let r = from_bytes("brief.docx", &bytes).unwrap();
        assert_eq!(r.method, ImportMethod::Docx);
        assert_eq!(r.text, "Sehr geehrte Damen und Herren,\nFrau B. ist seit 2019 in Behandlung & kommt wöchentlich.\nZweite Zeile");
        assert_eq!(r.sha256.len(), 64);
    }

    #[test]
    fn reads_text_files_in_both_encodings() {
        let r = from_bytes("a.txt", "\u{feff}Schlafstörungen   seit\r\nMärz, Behand-\nlung wöchentlich\n\n\n\nEnde".as_bytes()).unwrap();
        assert_eq!(r.text, "Schlafstörungen seit\nMärz, Behandlung wöchentlich\n\nEnde");
        let latin = b"Schlafst\xf6rungen seit M\xe4rz \x96 wieder gr\xfc\xdfer";
        assert_eq!(from_bytes("b.txt", latin).unwrap().text, "Schlafstörungen seit März – wieder grüßer");
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        assert!(from_bytes("alt.doc", &[0xD0, 0xCF, 0x11, 0xE0, 0, 0]).is_err());
        assert!(from_bytes("x.txt", b"  \n ").is_err(), "no text");
        assert!(from_bytes("x.zip", b"PK\x03\x04rest").is_err());
        assert!(from_bytes("kaputt.docx", b"PK\x03\x04rest").is_err());
    }

    #[test]
    fn tidy_keeps_lists_and_compounds() {
        assert_eq!(tidy("Ein-\nund Ausschlafen\n- Punkt"), "Ein-\nund Ausschlafen\n- Punkt");
    }

    fn sample() -> crate::render::letter::LetterDoc {
        let head = crate::render::letter::Letterhead { practice: Default::default(), therapist: None, logo: None, signature: None };
        crate::render::letter::sample(head, "2026-11-07")
    }

    /// The sample letter through the real Windows OCR, as a scanned PDF page and as a photo
    /// (needs Windows with German text recognition).
    #[test]
    #[ignore]
    fn ocr_reads_a_rendered_letter() {
        assert!(ocr::available());
        let doc = sample();
        let pdf = crate::render::letter::letter_pdf(&doc).unwrap();
        let mut scan = ocr::Pdf::open(&pdf).unwrap();
        assert_eq!(scan.pages().unwrap(), 1);
        let text = scan.page(0).unwrap().text;
        assert!(text.contains("Rentenversicherung") && text.contains("Briefkopf"), "{text}");
        let r = from_bytes("brief.pdf", &pdf).unwrap();
        assert_eq!(r.method, ImportMethod::Pdf, "the text layer is used");
        assert!(r.text.contains("Mustermann"), "{}", r.text);
        let png = crate::render::letter::letter_preview_pngs(&doc, 2.0).unwrap().remove(0);
        let r = from_bytes("foto.png", &png).unwrap();
        assert_eq!(r.method, ImportMethod::Ocr);
        assert!(r.text.contains("Bei Fragen stehe ich Ihnen gerne"), "{}", r.text);
    }
}
