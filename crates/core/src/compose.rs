//! Quick AI writing in Briefe (Phase 6, slice 47): "Mit KI schreiben" drafts all chosen sections
//! of a letter in one request, "Neu schreiben" re-writes one section with her instruction, and a
//! pasted request ("Anschreiben") is read for its numbered questions.
//!
//! The material is what the therapist ticked in the rail: the applied facts of the Behandlungsfall
//! (`[F n]`), her answers to the Lotse questions (`[A n]`) and the last signed Bericht or letter
//! (`[V n]`). Her instruction (`[I]`) and, on a re-write, the section's current text (`[B]`) count
//! as her own statements. Everything goes through one pseudonymizer; the patient is written as
//! "Pat." first and the model calls her "die Patientin" / "der Patient" (as the Lotse's letter
//! drafts, W4). Recipient, address, Betreff and salutation are never sent.
//!
//! Grounding differs from a Bericht draft: a sentence that cites nothing valid, or states a
//! prognosis, goal or motivation its sources don't, is **kept and marked "ohne Beleg"** (she keeps
//! or deletes it before signing). A sentence naming someone else, an ICD code or a token the
//! model invented is dropped as in the Bericht draft.

use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::Patient;
use crate::draft::{self, DraftSentence, Frame, InputFact, Parsed, SectionResult, Target};
use crate::extract::{json_list, sha256, Preview, TokenView, ICD};
use crate::llm::{ChatOptions, Effort};
use crate::pseudo::{Dictionary, Pseudonymized, Pseudonymizer, TokenMap};
use crate::templates::{Audience, SectionDef, Template};
use crate::{Error, Result};

/// Bump on any change to the prompt or the answer format.
pub const PROMPT_VERSION: &str = "letter-2026.3";
pub const QUESTIONS_PROMPT_VERSION: &str = "letter-questions-2026.1";

/// Longest pasted request sent to find its questions.
pub const MAX_REQUEST_CHARS: usize = 12_000;
/// Most questions taken from one request.
pub const MAX_QUESTIONS: usize = 20;
/// Longest question kept (it becomes a section heading).
pub const MAX_QUESTION_CHARS: usize = 300;

pub fn chat_options() -> ChatOptions {
    ChatOptions { temperature: 0.2, max_tokens: 8192, timeout: Duration::from_secs(900), reasoning: Some(Effort::Medium) }
}

pub fn questions_options() -> ChatOptions {
    ChatOptions { temperature: 0.0, max_tokens: 2048, timeout: Duration::from_secs(300), reasoning: Some(Effort::Low) }
}

/// What to write and from what.
pub struct Request<'a> {
    /// Sections to write, in any order (the template's order is used).
    pub keys: &'a [String],
    /// The material: session facts, then her answers (`own`), then the Vorbericht (`prior`).
    /// `DraftSentence::facts` index into this list.
    pub facts: &'a [InputFact],
    /// The request's question a section answers (`key`, question).
    pub questions: &'a [(String, String)],
    /// A re-write of one section: its current plain text.
    pub rewrite: Option<&'a str>,
    pub instruction: Option<&'a str>,
    /// Her Textbausteine for this letter type as (title, plain text for the model, see
    /// [`crate::textblock::for_ai`]): preferred wording `[T n]`, not material.
    pub blocks: &'a [(String, String)],
    /// Her style exemplars for this letter type as (title, excerpt, see [`crate::style`]):
    /// wording only, never a source.
    pub style: &'a [(String, String)],
}

pub struct Prepared {
    pub system: Pseudonymized<String>,
    pub user: Pseudonymized<String>,
    /// Sections the model may write, in template order.
    pub keys: Vec<String>,
    /// Source label ("F3", "A1", "V2") → index into the request's facts.
    labels: Vec<(String, usize)>,
    /// `[I]` was offered (an instruction went along).
    instructed: bool,
    /// `[B]` was offered (a re-write).
    rewrite: bool,
    pub map: TokenMap,
    /// SHA-256 of version + system prompt (no patient data), stored with the drafts.
    pub prompt_hash: String,
    /// SHA-256 over the outgoing messages: what the preview showed is what is sent.
    pub digest: String,
    /// The style exemplars as sent (inside the system prompt), for the preview.
    pub shared: Vec<String>,
}

impl Prepared {
    pub fn preview(&self) -> Preview {
        Preview { shared: self.shared.clone(), messages: vec![self.user.get().clone()], tokens: self.map.entries().iter().map(TokenView::from).collect(), digest: self.digest.clone() }
    }
}

/// The system prompt for a (resolved) letter template. Holds no patient data.
pub fn system_prompt(template: &Template, frame: &Frame, rewrite: bool) -> String {
    let (audience, gender) = match frame.target {
        Target::Letter { audience, gender } => (audience, gender),
        Target::Ptv3(_) => (Audience::Frei, None),
    };
    let extra = match audience {
        Audience::Behoerde => {
            "- Einschränkungen und ihre Auswirkungen auf Alltag, Beruf und Teilhabe konkret beschreiben, soweit die \
             Quellen sie hergeben.\n\
             - Keine Einstufungen vorschlagen (Grad der Behinderung, Pflegegrad, Leistungsvermögen in Stunden, \
             Merkzeichen), außer eine Angabe der Therapeutin nennt sie ausdrücklich; dann genau so übernehmen.\n"
        }
        Audience::Arzt => "- Kollegial und knapp; was für die Mitbehandlung wichtig ist, zuerst.\n",
        Audience::Patient | Audience::Frei => "",
    };
    let task = if rewrite {
        "Aufgabe: Formuliere den einen Abschnitt neu, nach der Anweisung der Therapeutin. Unter \"Bisheriger Text\" [B] \
         steht ihr aktueller Text; was er sagt, darf bleiben (Quelle \"B\"), wenn die Anweisung nichts anderes verlangt."
    } else {
        "Aufgabe: Schreibe den Text für die genannten Abschnitte des Schreibens. Jeden Inhalt nur einem Abschnitt \
         zuordnen, dem passendsten. Gibt keine Quelle etwas für einen Abschnitt her, lass ihn weg (leere Liste)."
    };
    format!(
        "Du unterstützt eine Psychotherapeutin beim Schreiben eines Schreibens ({title}; {method}) an {reader}.\n\
         Leitfaden: {intro}\n\n\
         Du erhältst die Abschnitte mit Überschrift und Hinweisen, bei Anfragen die Frage der Empfängerin, die der \
         Abschnitt beantwortet, und nummerierte Quellen: [F1] … Fakten aus den Sitzungsnotizen, die die Therapeutin \
         geprüft hat (Sitzung 1 ist die erste dokumentierte); [A1] … ihre eigenen Angaben zu einem Abschnitt, die \
         Frage in Klammern; [V1] … Abschnitte ihres letzten unterschriebenen Berichts oder Briefs (Vorbericht; bei \
         Widerspruch gilt der neuere Fakt); [I] ihre Anweisung, soweit sie Inhalt nennt. Unter [T1] … stehen ihre \
         Textbausteine: bevorzugte Formulierungen, keine Fakten.\n\n\
         {task}\n\
         Regeln:\n\
         - Nur was die Quellen hergeben. Nichts erfinden, keine Vermutungen als Tatsachen.\n\
         - Jeder Satz nennt im Feld \"sources\" die Quellen, auf denen er beruht, z. B. [\"F2\", \"A1\"]. Ein Satz ohne \
         Quelle wird als \"ohne Beleg\" markiert und muss von der Therapeutin geprüft werden: vermeide solche Sätze. \
         Keine Quellen wie [F1] in den Text schreiben.\n\
         - Prognose, Motivation, Therapieziele, Setting und Sitzungszahl nur, wenn eine zitierte Quelle sie \
         ausdrücklich nennt.\n\
         - Beantworte die Fragen der Empfängerin in ihrer Reihenfolge, jeweils im zugehörigen Abschnitt.\n\
         - Textbausteine [T…]: Passt einer zu einem Abschnitt und belegen die Quellen seinen Inhalt für diese Person, \
         übernimm seine Formulierung möglichst wörtlich und nenne zusätzlich \"T…\" in \"sources\". Einen Baustein \
         nie nur deshalb verwenden, weil es ihn gibt; er allein belegt keinen Satz. \"…\" in einem Baustein ist eine \
         Lücke: nur füllen, wenn eine Quelle den Inhalt nennt, sonst den Teil weglassen.\n\
         - Sachlich, in ganzen Sätzen, Fachsprache, auf Deutsch. Ein Eintrag pro Satz.\n\
         - Die Person heißt durchgehend {person}. Keine Namen, keine Anrede, keine Grußformel; Anschrift, Betreff und \
         Gruß setzt die Therapeutin selbst.\n\
         {extra}\
         - Andere Personen nur über ihre Rolle (die Schwester, der Partner, die Vorgesetzte), keine Orte und keine \
         Arbeitgeber nennen. Platzhalter in eckigen Klammern wie [PERSON_n] nicht übernehmen.\n\
         - Keine Diagnosen, keine ICD-Codes, keine Vorschläge zu Medikamenten oder Dosierungen.\n\
         - Eine Anweisung der Therapeutin befolgen, soweit sie diesen Regeln nicht widerspricht.\n\n\
         Antworte ausschließlich mit JSON in dieser Form, ohne weiteren Text:\n\
         {{\"sections\": [{{\"key\": \"<Schlüssel>\", \"sentences\": [{{\"text\": \"…\", \"sources\": [\"F1\", \"A2\"]}}]}}]}}\n",
        title = template.title,
        method = draft::method_name(frame.method),
        reader = draft::reader(audience),
        intro = template.intro.trim(),
        person = draft::person(gender),
    )
}

/// The label prefix of a source: session fact, her answer, Vorbericht.
fn prefix(f: &InputFact) -> char {
    if f.own {
        'A'
    } else if f.prior {
        'V'
    } else {
        'F'
    }
}

/// The source list as the model sees it, and label → index.
fn source_list(facts: &[InputFact], defs: &[&SectionDef], patient: &Patient) -> (String, Vec<(String, usize)>) {
    let (mut out, mut labels) = (String::new(), Vec::new());
    let (mut n_f, mut n_a, mut n_v, mut session, mut head) = (0, 0, 0, 0, ' ');
    for (i, f) in facts.iter().enumerate() {
        let p = prefix(f);
        // Her answers belong to one section: only those for a section being written go along.
        if f.own && !f.bericht.as_ref().is_some_and(|b| b.iter().any(|k| defs.iter().any(|d| &d.key == k))) {
            continue;
        }
        let n = match p {
            'A' => {
                n_a += 1;
                n_a
            }
            'V' => {
                n_v += 1;
                n_v
            }
            _ => {
                n_f += 1;
                n_f
            }
        };
        if p != head {
            head = p;
            out.push_str(match p {
                'A' => "Angaben der Therapeutin:\n",
                'V' => "Vorbericht:\n",
                _ => "Fakten aus den Sitzungsnotizen:\n",
            });
        }
        if p == 'F' && f.session_no != session {
            session = f.session_no;
            out.push_str(&format!("Sitzung {session}:\n"));
        }
        let label = format!("{p}{n}");
        let note = if f.own {
            let section = f.bericht.as_ref().and_then(|b| b.first()).and_then(|k| defs.iter().find(|d| &d.key == k)).map(|d| d.heading.trim());
            let q = draft::patient_as_pat(f.label.trim(), patient);
            match section {
                Some(s) if !s.is_empty() => format!("{q}; zu „{s}“"),
                _ => q,
            }
        } else {
            f.label.clone()
        };
        out.push_str(&format!("[{label}] ({note}) {}\n", draft::patient_as_pat(f.text.trim(), patient)));
        labels.push((label, i));
    }
    (out, labels)
}

fn user_message(
    defs: &[&SectionDef],
    questions: &[(String, String)],
    sources: &str,
    rewrite: Option<&str>,
    instruction: Option<&str>,
    blocks: &[(String, String)],
) -> String {
    let mut m = String::from("Abschnitte (Schlüssel: Überschrift):\n");
    for d in defs {
        let heading = d.heading.trim();
        m.push_str(&format!("- {}: {}\n", d.key, if heading.is_empty() { "(Einleitung, ohne Überschrift)" } else { heading }));
        if let Some((_, q)) = questions.iter().find(|(k, _)| k == &d.key) {
            m.push_str(&format!("  Frage der Empfängerin: {}\n", q.trim()));
        }
        for h in &d.hints {
            m.push_str(&format!("  · {}\n", h.text.trim()));
        }
    }
    if let Some(i) = instruction {
        m.push_str(&format!("\n[I] Anweisung der Therapeutin: {i}\n"));
    }
    if let Some(b) = rewrite {
        m.push_str(&format!("\n[B] Bisheriger Text:\n{b}\n"));
    }
    m.push('\n');
    if sources.is_empty() {
        m.push_str("Keine weiteren Quellen.\n");
    } else {
        m.push_str(sources);
    }
    if !blocks.is_empty() {
        m.push_str("\nTextbausteine der Therapeutin (bevorzugte Formulierungen, keine Fakten):\n");
        for (i, (title, text)) in blocks.iter().enumerate() {
            m.push_str(&format!("[T{}] ({}) {}\n", i + 1, title.trim(), text.trim()));
        }
    }
    m
}

/// Everything that leaves the machine for one letter draft (or one section's re-write).
pub fn prepare(template: &Template, frame: &Frame, req: &Request, patient: &Patient, dict: Dictionary) -> Result<Prepared> {
    let targets = draft::target_sections(template);
    let mut defs: Vec<&SectionDef> = targets.iter().copied().filter(|d| req.keys.contains(&d.key)).collect();
    defs.dedup_by(|a, b| a.key == b.key);
    if defs.is_empty() {
        return Err(Error::Invalid("Kein Abschnitt zum Schreiben gewählt".into()));
    }
    if req.rewrite.is_some() && defs.len() != 1 {
        return Err(Error::Invalid("Neu schreiben gilt für einen Abschnitt".into()));
    }
    let instruction = draft::clean_instruction(req.instruction)?.map(|i| draft::patient_as_pat(&i, patient));
    let rewrite = req.rewrite.map(str::trim).filter(|t| !t.is_empty());
    if rewrite.is_some_and(|t| t.chars().count() > draft::MAX_EXISTING_CHARS) {
        return Err(Error::Invalid(format!("Der Abschnitt ist zu lang zum Neuschreiben (höchstens {} Zeichen)", draft::MAX_EXISTING_CHARS)));
    }
    if req.rewrite.is_some() && instruction.is_none() {
        return Err(Error::Invalid("Bitte angeben, wie der Abschnitt neu geschrieben werden soll".into()));
    }
    let (sources, labels) = source_list(req.facts, &defs, patient);
    if labels.is_empty() && instruction.is_none() && rewrite.is_none() {
        return Err(Error::Invalid(
            "Kein Material gewählt: Fakten aus Sitzungen, Ihre Angaben oder den Vorbericht ankreuzen, oder eine Anweisung schreiben, worum es gehen soll.".into(),
        ));
    }
    let chars = sources.chars().count();
    if chars > draft::FACT_CHARS {
        return Err(Error::Invalid(format!("Zu viel Material für einen Brief ({chars} Zeichen, höchstens {}). Weniger ankreuzen.", draft::FACT_CHARS)));
    }
    let questions: Vec<(String, String)> = req.questions.iter().map(|(k, q)| (k.clone(), draft::patient_as_pat(q, patient))).collect();
    let rewrite_text = rewrite.map(|t| draft::patient_as_pat(t, patient));
    let blocks: Vec<(String, String)> = req
        .blocks
        .iter()
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(title, t)| (draft::patient_as_pat(title, patient), draft::patient_as_pat(t, patient)))
        .collect();

    let mut p = Pseudonymizer::new(dict);
    let mut system_raw = system_prompt(template, frame, rewrite.is_some());
    if !req.style.is_empty() {
        system_raw.push_str(&format!("\n{}\n", crate::style::PROMPT_RULES));
    }
    let prompt_hash = sha256(&format!("{PROMPT_VERSION}\n{system_raw}"));
    let (system, shared) = with_style(&mut p, &system_raw, req.style, patient)?;
    let user = p.pseudonymize(&user_message(&defs, &questions, &sources, rewrite_text.as_deref(), instruction.as_deref(), &blocks))?;
    let mut h = Sha256::new();
    h.update(system.get());
    h.update([0]);
    h.update(user.get());
    Ok(Prepared {
        system,
        user,
        keys: defs.iter().map(|d| d.key.clone()).collect(),
        labels,
        instructed: instruction.is_some(),
        rewrite: rewrite.is_some(),
        map: p.into_map(),
        prompt_hash,
        digest: hex::encode(h.finalize()),
        shared,
    })
}

/// The system prompt with her style exemplars appended, and the exemplars as sent (for the
/// preview). One pseudonymizer: a date in an exemplar gets the same token everywhere.
pub(crate) fn with_style(p: &mut Pseudonymizer, system: &str, style: &[(String, String)], patient: &Patient) -> Result<(Pseudonymized<String>, Vec<String>)> {
    let block = crate::style::prompt_block(style);
    if block.is_empty() {
        return Ok((p.pseudonymize(system)?, Vec::new()));
    }
    let block = draft::patient_as_pat(&block, patient);
    let shared = p.pseudonymize(&block)?.into_inner();
    Ok((p.pseudonymize(&format!("{system}\n{block}"))?, vec![shared]))
}

// Source markers some models write into the text anyway ("… Arbeit [F1].", "(A2, F3)").
static MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*[\[(](?:[FAVIBTS]\s?\d*)(?:\s*[,;]\s*[FAVIBTS]\s?\d*)*[\])]").expect("valid regex"));
static DANGLING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*[,;]+(\s*[.!?]|\s*$)").expect("valid regex"));

/// Checks the answer: one result per section that got sentences, in template order. Fails only
/// without any JSON.
pub fn parse(answer: &str, prepared: &Prepared, facts: &[InputFact]) -> Result<Vec<SectionResult>> {
    let mut by_key: Vec<(String, Parsed)> = prepared.keys.iter().map(|k| (k.clone(), Parsed::default())).collect();
    let mut stray = 0;
    for item in json_list(answer, "sections")? {
        let key = item.get("key").and_then(Value::as_str).map(str::trim).unwrap_or_default();
        let sentences = item.get("sentences").and_then(Value::as_array).cloned().unwrap_or_default();
        let Some((_, parsed)) = by_key.iter_mut().find(|(k, _)| k == key) else {
            stray += sentences.len().max(1);
            continue;
        };
        for s in sentences {
            let (text, sources) = match &s {
                Value::String(t) => (Some(t.as_str()), vec![]),
                Value::Object(o) => (
                    o.get("text").and_then(Value::as_str),
                    o.get("sources").or_else(|| o.get("facts")).and_then(Value::as_array).cloned().unwrap_or_default(),
                ),
                _ => (None, vec![]),
            };
            let Some(text) = text.and_then(|t| clean(t, &prepared.map)) else {
                parsed.dropped += 1;
                continue;
            };
            if parsed.sentences.iter().any(|x| x.text == text) {
                continue;
            }
            let (cited, own) = citations(&sources, prepared);
            // Her own statements (instruction, current text) ground a sentence; so do cited facts,
            // unless it states a prognosis, goal or motivation none of them states.
            let texts: Vec<&str> = cited
                .iter()
                .filter_map(|&i| facts.get(i))
                .flat_map(|f| if f.own { vec![f.label.as_str(), f.text.as_str()] } else { vec![f.text.as_str()] })
                .collect();
            let grounded = own || (!cited.is_empty() && !draft::unsupported(&text, &texts));
            if !grounded {
                parsed.unsupported.push(parsed.sentences.len());
            }
            parsed.sentences.push(DraftSentence { text, facts: cited });
        }
    }
    // What was dropped from sections left with nothing counts in the first result.
    let lost = stray + by_key.iter().filter(|(_, p)| p.sentences.is_empty()).map(|(_, p)| p.dropped).sum::<usize>();
    if let Some((_, first)) = by_key.iter_mut().find(|(_, p)| !p.sentences.is_empty()) {
        first.dropped += lost;
    }
    Ok(by_key
        .into_iter()
        .filter(|(_, p)| !p.sentences.is_empty())
        .map(|(key, parsed)| SectionResult { key, parsed, with_existing: prepared.rewrite })
        .collect())
}

/// The valid fact indices a sentence cites, and whether it cites her own statement ([I], [B]).
/// A Textbaustein ([T n]) is wording only: citing it neither backs a sentence nor is stored.
fn citations(sources: &[Value], prepared: &Prepared) -> (Vec<usize>, bool) {
    let (mut cited, mut own) = (Vec::new(), false);
    for v in sources {
        let label = match v {
            Value::String(t) => t.trim().trim_matches(['[', ']']).replace(' ', "").to_ascii_uppercase(),
            Value::Number(n) => format!("F{n}"),
            _ => continue,
        };
        if (label == "I" && prepared.instructed) || (label == "B" && prepared.rewrite) {
            own = true;
        } else if let Some((_, i)) = prepared.labels.iter().find(|(l, _)| *l == label) {
            if !cited.contains(i) {
                cited.push(*i);
            }
        }
    }
    cited.sort_unstable();
    (cited, own)
}

fn clean(raw: &str, map: &TokenMap) -> Option<String> {
    let text = MARKER.replace_all(raw, "");
    let text = DANGLING.replace_all(&text, "$1").split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() || text.chars().count() > draft::MAX_SENTENCE_CHARS || ICD.is_match(&text) {
        return None;
    }
    // Only dates come back; a name, place or employer token means the sentence names someone.
    if draft::TOKEN.captures_iter(&text).any(|c| &c[1] != "DATUM") {
        return None;
    }
    let restored = map.restore(&text);
    if !restored.unknown.is_empty() {
        return None;
    }
    Some(restored.text)
}

/// The sentences as one paragraph of TipTap HTML; a sentence "ohne Beleg" is marked.
pub fn html<'a>(sentences: impl IntoIterator<Item = (&'a str, bool)>) -> String {
    let text: Vec<String> = sentences
        .into_iter()
        .map(|(t, unsupported)| {
            let t = t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
            if unsupported {
                format!("<mark {UNSUPPORTED_ATTR}=\"\">{t}</mark>")
            } else {
                t
            }
        })
        .collect();
    format!("<p>{}</p>", text.join(" "))
}

/// The attribute that marks a sentence "ohne Beleg" in a section's HTML (the editor's mark).
pub const UNSUPPORTED_ATTR: &str = "data-ohne-beleg";

/// Sentences still marked "ohne Beleg" in a section's HTML.
pub fn unsupported_count(html: &str) -> usize {
    html.matches(UNSUPPORTED_ATTR).count()
}

// ---------- the request ("Anschreiben") ----------

/// A question or request found in an Anschreiben, in its order and wording.
pub struct PreparedQuestions {
    pub system: Pseudonymized<String>,
    pub user: Pseudonymized<String>,
    pub map: TokenMap,
    pub prompt_hash: String,
    pub digest: String,
}

impl PreparedQuestions {
    pub fn preview(&self) -> Preview {
        Preview { shared: Vec::new(), messages: vec![self.user.get().clone()], tokens: self.map.entries().iter().map(TokenView::from).collect(), digest: self.digest.clone() }
    }
}

pub fn questions_system_prompt() -> String {
    "Du liest das Anschreiben einer Behörde, Kasse oder Ärztin an eine Psychotherapeutin, die um einen Befundbericht \
     oder eine Stellungnahme gebeten wird.\n\n\
     Aufgabe: Finde die Fragen und Bitten, die der Bericht beantworten soll, in ihrer Reihenfolge. Meist sind sie \
     nummeriert (1., 2., a), b)) oder als Liste gesetzt.\n\
     Regeln:\n\
     - Jede Frage einzeln, so nah wie möglich am Wortlaut des Schreibens, ohne Nummer am Anfang.\n\
     - Zusammengehörige Teile einer Frage zusammen lassen (\"Depressionen (Art, Häufigkeit und Dauer)\").\n\
     - Keine Höflichkeitsformeln, keine Hinweise zu Fristen, Vergütung, Datenschutz oder Rücksendung.\n\
     - Platzhalter in eckigen Klammern wie [PERSON_n], [DATUM_n] oder [ZEICHEN] unverändert übernehmen.\n\
     - Steht keine Frage im Text, eine leere Liste.\n\n\
     Antworte ausschließlich mit JSON in dieser Form, ohne weiteren Text:\n\
     {\"questions\": [\"…\", \"…\"]}\n"
        .to_string()
}

// A labelled reference number in a request ("Az. 52-123/26", "Versicherungsnummer 12 030485 S 123"):
// it ties the letter to the person at the recipient, so its value stays here. The value starts
// with a digit; further space-separated parts start with a digit or are one capital letter.
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(Az\.?|Aktenzeichen|Gesch(?:äfts|\.)-?\s?(?:Z\.?|Zeichen)|Kennzeichen|(?:Ihr|Unser)e?\s+Zeichen|Vers(?:icherungs|icherten)?-?\s?Nr\.?|Versicherungsnummer|Versichertennummer|Rentenversicherungsnummer|(?:RV|KV|Mitglieds)-?\s?Nr\.?|BSNR|LANR)(\s*[:.]?\s*)(?-i:\d[\w./-]*(?: (?:\d[\w./-]*|[A-Z]\b))*)",
    )
    .expect("valid regex")
});

/// Placeholder sent instead of a reference number (not restored: the questions don't need it).
pub const REFERENCE_PLACEHOLDER: &str = "[ZEICHEN]";

/// Byte ranges of the values of labelled reference numbers in `text`.
pub fn reference_spans(text: &str) -> Vec<(usize, usize)> {
    REFERENCE.captures_iter(text).map(|c| (c.get(2).expect("group 2").end(), c.get(0).expect("match").end())).collect()
}

/// The request with labelled reference numbers replaced by [`REFERENCE_PLACEHOLDER`].
pub fn redact_references(text: &str) -> String {
    REFERENCE.replace_all(text, format!("${{1}}${{2}}{REFERENCE_PLACEHOLDER}")).into_owned()
}

/// Everything that leaves the machine to find a request's questions.
pub fn prepare_questions(request: &str, patient: &Patient, dict: Dictionary) -> Result<PreparedQuestions> {
    let text = request.trim();
    if text.is_empty() {
        return Err(Error::Invalid("Kein Anschreiben eingefügt".into()));
    }
    let chars = text.chars().count();
    if chars > MAX_REQUEST_CHARS {
        return Err(Error::Invalid(format!("Das Anschreiben ist zu lang ({chars} Zeichen, höchstens {MAX_REQUEST_CHARS}). Nur den Teil mit den Fragen einfügen.")));
    }
    let mut p = Pseudonymizer::new(dict);
    let system_raw = questions_system_prompt();
    let prompt_hash = sha256(&format!("{QUESTIONS_PROMPT_VERSION}\n{system_raw}"));
    let system = p.pseudonymize(&system_raw)?;
    let text = redact_references(&draft::patient_as_pat(text, patient));
    let user = p.pseudonymize(&format!("Anschreiben:\n\"\"\"\n{text}\n\"\"\"\n"))?;
    let mut h = Sha256::new();
    h.update(system.get());
    h.update([0]);
    h.update(user.get());
    Ok(PreparedQuestions { system, user, map: p.into_map(), prompt_hash, digest: hex::encode(h.finalize()) })
}

static LEADING_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:\(?\d{1,2}[.)]|\(?[a-z][.)])\s+").expect("valid regex"));

/// The questions in the answer, names and dates restored (the request is hers to read).
pub fn parse_questions(answer: &str, prepared: &PreparedQuestions) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for item in json_list(answer, "questions")? {
        let raw = match &item {
            Value::String(t) => t.as_str(),
            Value::Object(o) => o.get("text").or_else(|| o.get("question")).and_then(Value::as_str).unwrap_or_default(),
            _ => continue,
        };
        let text = LEADING_NUMBER.replace(raw, "").split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() || text.chars().count() > MAX_QUESTION_CHARS {
            continue;
        }
        let restored = prepared.map.restore(&text);
        if !restored.unknown.is_empty() || out.contains(&restored.text) {
            continue;
        }
        out.push(restored.text);
        if out.len() == MAX_QUESTIONS {
            break;
        }
    }
    Ok(out)
}

static DEADLINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bbis\s+(?:spätestens\s+)?(?:zum\s+|am\s+)?(\d{1,2})\.\s?(\d{1,2})\.\s?(\d{2}|\d{4})\b").expect("valid regex")
});

/// A deadline the request names ("bis zum 15.12.2026", "bis spätestens 3.1.27"), as YYYY-MM-DD.
/// Read locally; proposed as the letter's Frist.
pub fn deadline(request: &str) -> Option<String> {
    DEADLINE.captures_iter(request).find_map(|c| {
        let (d, m): (u32, u32) = (c[1].parse().ok()?, c[2].parse().ok()?);
        let y: i32 = match c[3].len() {
            2 => 2000 + c[3].parse::<i32>().ok()?,
            _ => c[3].parse().ok()?,
        };
        let month = time::Month::try_from(u8::try_from(m).ok()?).ok()?;
        time::Date::from_calendar_date(y, month, u8::try_from(d).ok()?).ok().map(|_| format!("{y:04}-{m:02}-{d:02}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgeGroup, Gender, Method};
    use crate::templates;

    fn template() -> Template {
        let t = templates::builtin().unwrap().into_iter().find(|t| t.key == "versorgungsamt-befund").unwrap();
        t.resolve(&templates::Context { method: Method::Vt, age_group: AgeGroup::Adult, kind: None })
    }

    fn frame() -> Frame {
        Frame { method: Method::Vt, age_group: AgeGroup::Adult, target: Target::Letter { audience: Audience::Behoerde, gender: Some(Gender::W) } }
    }

    fn patient() -> Patient {
        Patient {
            id: "p".into(),
            external_ref: None,
            first_name: "Anna".into(),
            last_name: "Beispiel".into(),
            birth_date: Some("1985-03-14".into()),
            chiffre: None,
            pronouns: None,
            gender: Some(Gender::W),
            consent_recording: false,
            status: "active".into(),
            created_at: String::new(),
            archived_at: None,
            contact: Default::default(),
        }
    }

    fn dict() -> Dictionary {
        let mut d = Dictionary::from_patient(&patient());
        d.add(crate::pseudo::EntityKind::Person, "Jonas");
        d
    }

    fn facts(keys: &[String]) -> Vec<InputFact> {
        vec![
            InputFact { session_no: 1, label: "Befund".into(), text: "Frau Beispiel schläft seit dem 12.03.2026 kaum.".into(), bericht: None, own: false, prior: false },
            InputFact { session_no: 2, label: "Verlauf".into(), text: "Jonas hilft beim Einkaufen.".into(), bericht: None, own: false, prior: false },
            InputFact::answer(&keys[0], "Wie oft findet die Behandlung statt?", "wöchentlich"),
            InputFact::answer("anderer", "Nicht für diesen Brief", "x"),
        ]
    }

    fn keys(t: &Template) -> Vec<String> {
        draft::target_sections(t).iter().map(|d| d.key.clone()).collect()
    }

    #[test]
    fn prepare_pseudonymizes_and_labels_sources() {
        let t = template();
        let k = keys(&t);
        let f = facts(&k);
        let q = vec![(k[1].clone(), "Depressionen (Art, Häufigkeit und Dauer) bei Frau Beispiel".to_string())];
        let req = Request { keys: &k, facts: &f, questions: &q, rewrite: None, instruction: Some("knapp, Frau Beispiel siezen"), blocks: &[], style: &[] };
        let p = prepare(&t, &frame(), &req, &patient(), dict()).unwrap();
        let msg = p.user.get();
        for secret in ["Anna", "Beispiel", "Jonas", "12.03.2026", "1985"] {
            assert!(!msg.contains(secret), "{secret} in {msg}");
        }
        assert!(msg.contains("[F1] (Befund) Pat. schläft") && msg.contains("[F2]") && msg.contains("[A1] (Wie oft"), "{msg}");
        assert!(msg.contains("Frage der Empfängerin: Depressionen (Art, Häufigkeit und Dauer) bei Pat."), "{msg}");
        assert!(!msg.contains("Nicht für diesen Brief"), "answers of other sections stay out");
        assert!(msg.contains("[I] Anweisung der Therapeutin: knapp, Pat. siezen"));
        assert!(p.system.get().contains("\"die Patientin\"") && p.system.get().contains("Keine Einstufungen"));
        assert_eq!(p.keys, k);
        assert_eq!(p.preview().messages.len(), 1);
    }

    #[test]
    fn prepare_needs_material_or_an_instruction() {
        let t = template();
        let k = keys(&t);
        let none = Request { keys: &k, facts: &[], questions: &[], rewrite: None, instruction: None, blocks: &[], style: &[] };
        assert!(prepare(&t, &frame(), &none, &patient(), dict()).is_err());
        let told = Request { instruction: Some("Bitte um Verlängerung der Frist um vier Wochen"), ..none };
        assert!(prepare(&t, &frame(), &told, &patient(), dict()).is_ok());
        let nothing: Vec<String> = vec![];
        let no_keys = Request { keys: &nothing, ..told };
        assert!(prepare(&t, &frame(), &no_keys, &patient(), dict()).is_err());
        let one = vec![k[0].clone()];
        let rewrite = Request { keys: &one, facts: &[], questions: &[], rewrite: Some("Alter Text."), instruction: None, blocks: &[], style: &[] };
        assert!(prepare(&t, &frame(), &rewrite, &patient(), dict()).is_err(), "a re-write needs an instruction");
        let rewrite = Request { instruction: Some("kürzer"), ..rewrite };
        let p = prepare(&t, &frame(), &rewrite, &patient(), dict()).unwrap();
        assert!(p.user.get().contains("[B] Bisheriger Text:\nAlter Text."));
        let two = Request { keys: &k, ..rewrite };
        assert!(prepare(&t, &frame(), &two, &patient(), dict()).is_err(), "a re-write is one section");
    }

    #[test]
    fn parse_keeps_unbacked_sentences_marked_and_drops_names() {
        let t = template();
        let k = keys(&t);
        let f = facts(&k);
        let req = Request { keys: &k, facts: &f, questions: &[], rewrite: None, instruction: Some("Behandlung wöchentlich erwähnen"), blocks: &[], style: &[] };
        let p = prepare(&t, &frame(), &req, &patient(), dict()).unwrap();
        let date = p.map.entries().iter().find(|e| e.token.starts_with("[DATUM")).unwrap().token.clone();
        let person = p.map.entries().iter().find(|e| e.token.starts_with("[PERSON")).unwrap().token.clone();
        let (a, b) = (k[0].clone(), k[1].clone());
        let answer = format!(
            r#"{{"sections": [
                {{"key": "{a}", "sentences": [
                    {{"text": "Die Patientin schläft seit dem {date} kaum [F1].", "sources": ["F1"]}},
                    {{"text": "Die Behandlung findet wöchentlich statt.", "sources": ["A1"]}},
                    {{"text": "Die Prognose ist günstig.", "sources": ["F1"]}},
                    {{"text": "Sie ist sehr belastet.", "sources": []}},
                    {{"text": "Wöchentliche Termine sind vereinbart.", "sources": ["I"]}},
                    {{"text": "Unbekannte Quelle.", "sources": ["F9"]}}
                ]}},
                {{"key": "{b}", "sentences": [
                    {{"text": "{person} hilft beim Einkaufen.", "sources": ["F2"]}},
                    {{"text": "Diagnose F32.1.", "sources": ["F1"]}}
                ]}},
                {{"key": "nope", "sentences": ["x"]}}
            ]}}"#
        );
        let r = parse(&answer, &p, &f).unwrap();
        assert_eq!(r.len(), 1, "the second section has nothing left");
        let texts: Vec<&str> = r[0].parsed.sentences.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            texts,
            ["Die Patientin schläft seit dem 12.03.2026 kaum.", "Die Behandlung findet wöchentlich statt.", "Die Prognose ist günstig.", "Sie ist sehr belastet.", "Wöchentliche Termine sind vereinbart.", "Unbekannte Quelle."]
        );
        assert_eq!(r[0].parsed.sentences[0].facts, vec![0]);
        assert_eq!(r[0].parsed.sentences[1].facts, vec![2], "A1 is the third input");
        assert_eq!(r[0].parsed.unsupported, vec![2, 3, 5], "prognosis not in F1, nothing cited, unknown source");
        assert_eq!(r[0].parsed.dropped, 3, "named person, ICD code, stray section");
        assert!(parse("kein json", &p, &f).is_err());
    }

    #[test]
    fn textblocks_are_wording_not_evidence() {
        let t = template();
        let k = keys(&t);
        let f = facts(&k);
        let blocks = vec![(
            "Teufelskreis".to_string(),
            crate::textblock::for_ai("<p>{Frau/Herr} {Nachname}: Schmerzen und Stimmung verstärken sich gegenseitig.</p>", Some(Gender::W)),
        )];
        let req = Request { keys: &k, facts: &f, questions: &[], rewrite: None, instruction: None, blocks: &blocks, style: &[] };
        let p = prepare(&t, &frame(), &req, &patient(), dict()).unwrap();
        let msg = p.user.get();
        assert!(msg.contains("[T1] (Teufelskreis) Pat.: Schmerzen und Stimmung"), "{msg}");
        assert!(!msg.contains("Beispiel"));
        assert!(p.system.get().contains("er allein belegt keinen Satz"));
        let a = k[0].clone();
        let answer = format!(
            r#"{{"sections": [{{"key": "{a}", "sentences": [
                {{"text": "Schmerzen und Stimmung verstärken sich gegenseitig [T1].", "sources": ["T1"]}},
                {{"text": "Die Patientin schläft kaum, Schmerzen und Stimmung verstärken sich.", "sources": ["F1", "T1"]}}
            ]}}]}}"#
        );
        let r = parse(&answer, &p, &f).unwrap();
        assert_eq!(r[0].parsed.sentences[0].text, "Schmerzen und Stimmung verstärken sich gegenseitig.");
        assert_eq!(r[0].parsed.unsupported, vec![0], "a block alone backs nothing");
        assert_eq!(r[0].parsed.sentences[1].facts, vec![0], "T is not stored as a source");
    }

    #[test]
    fn style_exemplars_go_once_in_the_system_prompt_and_back_nothing() {
        let t = template();
        let k = keys(&t);
        let f = facts(&k);
        // An exemplar is redacted when stored; a name of this patient or a date left in it is
        // still pseudonymized on the way out.
        let style = vec![("DRV 2023".to_string(), "Frau [Name] befindet sich seit dem 02.05.2019 in Behandlung; Frau Beispiel auch.".to_string())];
        let req = Request { keys: &k, facts: &f, questions: &[], rewrite: None, instruction: None, blocks: &[], style: &style };
        let p = prepare(&t, &frame(), &req, &patient(), dict()).unwrap();
        let system = p.system.get();
        assert!(system.contains("Stilbeispiele") && system.contains("[S1] DRV 2023") && system.contains("Ein Stilbeispiel ist nie eine Quelle"), "{system}");
        assert!(!system.contains("02.05.2019") && !system.contains("Beispiel auch"), "{system}");
        assert!(!p.user.get().contains("Stilbeispiele"), "not repeated in the message");
        assert_eq!(p.preview().shared.len(), 1);
        assert!(p.preview().shared[0].starts_with("Stilbeispiele") && p.preview().shared[0].contains("[DATUM_"));
        let without = prepare(&t, &frame(), &Request { style: &[], ..req }, &patient(), dict()).unwrap();
        assert_ne!(without.prompt_hash, p.prompt_hash, "the rules are part of the prompt");
        assert!(without.preview().shared.is_empty() && !without.system.get().contains("Stilbeispiel"));
        assert_ne!(without.digest, p.digest);
        let a = k[0].clone();
        let answer = format!(r#"{{"sections": [{{"key": "{a}", "sentences": [{{"text": "Sie befindet sich in Behandlung [S1].", "sources": ["S1"]}}]}}]}}"#);
        let r = parse(&answer, &p, &f).unwrap();
        assert_eq!(r[0].parsed.sentences[0].text, "Sie befindet sich in Behandlung.");
        assert_eq!(r[0].parsed.unsupported, vec![0], "an exemplar is no source");
    }

    #[test]
    fn html_marks_unbacked_sentences() {
        let h = html([("A & B.", false), ("Ohne <Beleg>.", true)]);
        assert_eq!(h, "<p>A &amp; B. <mark data-ohne-beleg=\"\">Ohne &lt;Beleg&gt;.</mark></p>");
        assert_eq!(unsupported_count(&h), 1);
    }

    #[test]
    fn questions_are_pseudonymized_and_restored() {
        let req = "Sehr geehrte Damen und Herren,\nin der Schwerbehindertenangelegenheit von Frau Anna Beispiel bitten wir bis zum 15.12.2026 um:\n1. Datum der letzten Behandlung\n2. Depressionen (Art, Häufigkeit und Dauer)";
        let p = prepare_questions(req, &patient(), dict()).unwrap();
        assert!(!p.user.get().contains("Beispiel") && !p.user.get().contains("15.12.2026"));
        let date = p.map.entries().iter().find(|e| e.token.starts_with("[DATUM")).unwrap().token.clone();
        let answer = format!(r#"{{"questions": ["1. Datum der letzten Behandlung", "2) Depressionen (Art, Häufigkeit und Dauer)", "Datum der letzten Behandlung", "Befund bis {date}", "[PERSON_9] fragen", ""]}}"#);
        let q = parse_questions(&answer, &p).unwrap();
        assert_eq!(q, ["Datum der letzten Behandlung", "Depressionen (Art, Häufigkeit und Dauer)", "Befund bis 15.12.2026"]);
        assert!(prepare_questions("  ", &patient(), dict()).is_err());
    }

    #[test]
    fn reference_numbers_stay_local() {
        assert_eq!(redact_references("Az. 52-123/26\nSehr geehrte"), "Az. [ZEICHEN]\nSehr geehrte");
        assert_eq!(redact_references("Versicherungsnummer: 12 030485 S 123 Sehr"), "Versicherungsnummer: [ZEICHEN] Sehr");
        assert_eq!(redact_references("Ihr Zeichen 4711-B, unser Zeichen: KV 1"), "Ihr Zeichen [ZEICHEN], unser Zeichen: KV 1");
        assert_eq!(redact_references("Aktenzeichen fehlt noch"), "Aktenzeichen fehlt noch");
        assert_eq!(redact_references("Behandlung seit 2013"), "Behandlung seit 2013");
        let p = prepare_questions("Geschäftszeichen 9/88-X\n1. Befund", &patient(), dict()).unwrap();
        assert!(p.user.get().contains("Geschäftszeichen [ZEICHEN]") && !p.user.get().contains("9/88"), "{}", p.user.get());
    }

    #[test]
    fn deadline_is_read_locally() {
        assert_eq!(deadline("Wir bitten um Rücksendung bis zum 15.12.2026.").as_deref(), Some("2026-12-15"));
        assert_eq!(deadline("bis spätestens 3.1.27 zurück").as_deref(), Some("2027-01-03"));
        assert_eq!(deadline("Behandlung von 2013 bis heute"), None);
        assert_eq!(deadline("bis zum 31.02.2026"), None);
    }
}
