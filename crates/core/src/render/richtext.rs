//! Minimal parser for the HTML subset the editor emits (p, h1-h4, ul/ol/li, strong/b, em/i, u,
//! br, `style="text-align: …"` on p and headings). Produces a neutral block model for PDF and DOCX.

use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// Paragraph alignment. `None` on a block is the document default (justified).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Block {
    P { runs: Vec<Run>, align: Option<Align> },
    /// A sub-heading inside a section (the section heading itself comes from the template).
    H { runs: Vec<Run>, align: Option<Align> },
    Ul { items: Vec<Vec<Run>> },
    Ol { items: Vec<Vec<Run>> },
}

pub fn parse(html: &str) -> Vec<Block> {
    let mut p = Parser::default();
    let mut rest = html;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('<') {
            let end = stripped.find('>').map(|i| i + 1).unwrap_or(stripped.len());
            p.tag(&stripped[..end.saturating_sub(1)]);
            rest = &stripped[end.min(stripped.len())..];
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            p.text(&decode(&rest[..end]));
            rest = &rest[end..];
        }
    }
    p.flush_para();
    p.blocks
}

#[derive(Default)]
struct Parser {
    blocks: Vec<Block>,
    runs: Vec<Run>,
    bold: u32,
    italic: u32,
    underline: u32,
    /// Alignment of the open p/h; `heading` = inside h1-h4.
    align: Option<Align>,
    heading: bool,
    list: Option<(bool, Vec<Vec<Run>>)>, // (ordered, items)
}

impl Parser {
    fn tag(&mut self, raw: &str) {
        let closing = raw.starts_with('/');
        let name = raw.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("").to_lowercase();
        match (name.as_str(), closing) {
            ("strong" | "b", false) => self.bold += 1,
            ("strong" | "b", true) => self.bold = self.bold.saturating_sub(1),
            ("em" | "i", false) => self.italic += 1,
            ("em" | "i", true) => self.italic = self.italic.saturating_sub(1),
            ("u", false) => self.underline += 1,
            ("u", true) => self.underline = self.underline.saturating_sub(1),
            ("br", _) => self.text("\n"),
            ("h1" | "h2" | "h3" | "h4", false) if self.list.is_none() => {
                self.flush_para();
                self.heading = true;
                self.align = align_of(raw);
            }
            ("h1" | "h2" | "h3" | "h4", true) if self.list.is_none() => {
                self.flush_para();
                self.heading = false;
            }
            ("p", false) if self.list.is_none() => {
                self.flush_para();
                self.align = align_of(raw);
            }
            ("p" | "h1" | "h2" | "h3" | "h4", true) => self.flush_para(),
            ("ul" | "ol", false) => {
                self.flush_para();
                self.list = Some((name == "ol", Vec::new()));
            }
            ("ul" | "ol", true) => {
                self.flush_item();
                if let Some((ordered, items)) = self.list.take() {
                    if !items.is_empty() {
                        self.blocks.push(if ordered { Block::Ol { items } } else { Block::Ul { items } });
                    }
                }
            }
            ("li", true) => self.flush_item(),
            _ => {}
        }
    }

    fn text(&mut self, t: &str) {
        if t.is_empty() {
            return;
        }
        let (bold, italic, underline) = (self.bold > 0, self.italic > 0, self.underline > 0);
        match self.runs.last_mut() {
            Some(r) if r.bold == bold && r.italic == italic && r.underline == underline => r.text.push_str(t),
            _ => self.runs.push(Run { text: t.to_string(), bold, italic, underline }),
        }
    }

    fn take_runs(&mut self) -> Option<Vec<Run>> {
        let runs = std::mem::take(&mut self.runs);
        runs.iter().any(|r| !r.text.trim().is_empty()).then_some(runs)
    }

    fn flush_item(&mut self) {
        if let Some(runs) = self.take_runs() {
            if let Some((_, items)) = &mut self.list {
                items.push(runs);
            }
        }
    }

    fn flush_para(&mut self) {
        if self.list.is_some() {
            return; // <p> inside <li>: keep collecting into the item
        }
        let align = self.align.take();
        if let Some(runs) = self.take_runs() {
            self.blocks.push(if self.heading { Block::H { runs, align } } else { Block::P { runs, align } });
        }
    }
}

/// `text-align` from a tag's style attribute.
fn align_of(raw: &str) -> Option<Align> {
    let rest = &raw[raw.find("text-align:")? + "text-align:".len()..];
    match rest.trim_start().split(|c: char| !c.is_ascii_alphabetic()).next()? {
        "left" => Some(Align::Left),
        "center" => Some(Align::Center),
        "right" => Some(Align::Right),
        "justify" => Some(Align::Justify),
        _ => None,
    }
}

fn decode(s: &str) -> String {
    s.replace("&nbsp;", "\u{a0}")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tiptap_output() {
        let b = parse("<p>Sehr <strong>starke</strong> Angst &amp; Unruhe</p><ul><li><p>Schlaf</p></li><li><p><em>Appetit</em></p></li></ul>");
        assert_eq!(b.len(), 2);
        match &b[0] {
            Block::P { runs, align: None } => {
                assert_eq!(runs.len(), 3);
                assert!(runs[1].bold);
                assert_eq!(runs[2].text, " Angst & Unruhe");
            }
            _ => panic!(),
        }
        match &b[1] {
            Block::Ul { items } => {
                assert_eq!(items.len(), 2);
                assert!(items[1][0].italic);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parses_underline_subheadings_and_alignment() {
        let b = parse(r#"<h3 style="text-align: center;">Verlauf <u>2025</u></h3><p style="text-align: right;">rechts</p><p>normal</p><ul><li><p style="text-align: center;">Punkt</p></li></ul>"#);
        let Block::H { runs, align } = &b[0] else { panic!("{b:?}") };
        assert_eq!(*align, Some(Align::Center));
        assert!(!runs[0].bold && !runs[0].underline && runs[1].underline);
        assert_eq!(b[1], Block::P { runs: vec![Run { text: "rechts".into(), ..Default::default() }], align: Some(Align::Right) });
        assert!(matches!(&b[2], Block::P { align: None, .. }));
        assert!(matches!(&b[3], Block::Ul { items } if items.len() == 1), "alignment inside lists is ignored");
        assert_eq!(b.len(), 4);
    }

    #[test]
    fn plain_text_becomes_paragraph() {
        assert_eq!(parse("nur Text").len(), 1);
        assert!(parse("<p></p>").is_empty());
    }
}
