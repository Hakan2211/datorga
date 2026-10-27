//! Minimal parser for the HTML subset TipTap's StarterKit emits (p, ul/ol/li, strong/b, em/i,
//! br, h1-h4 treated as bold paragraphs). Produces a neutral block model for PDF and DOCX.

use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Block {
    P { runs: Vec<Run> },
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
            ("br", _) => self.text("\n"),
            ("h1" | "h2" | "h3" | "h4", false) => {
                self.flush_para();
                self.bold += 1;
            }
            ("h1" | "h2" | "h3" | "h4", true) => {
                self.bold = self.bold.saturating_sub(1);
                self.flush_para();
            }
            ("p", true) => self.flush_para(),
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
        let (bold, italic) = (self.bold > 0, self.italic > 0);
        match self.runs.last_mut() {
            Some(r) if r.bold == bold && r.italic == italic => r.text.push_str(t),
            _ => self.runs.push(Run { text: t.to_string(), bold, italic }),
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
        if let Some(runs) = self.take_runs() {
            self.blocks.push(Block::P { runs });
        }
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
            Block::P { runs } => {
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
    fn plain_text_becomes_paragraph() {
        assert_eq!(parse("nur Text").len(), 1);
        assert!(parse("<p></p>").is_empty());
    }
}
