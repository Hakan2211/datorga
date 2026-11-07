//! Briefe (Phase 6): letters to the Rentenversicherung, Pflegekasse, Versorgungsamt, doctors
//! and others. A letter is a report with `kind = 'brief'` plus its letter data (recipient,
//! Betreff, references, sending). Address, salutation and Betreff are filled in locally and never
//! go to an AI.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::{Consent, Episode, Gender, Patient, RecipientKind, Report, ReportStatus, SentVia};
use crate::pii::CheckItem;
use crate::templates::{FieldKind, PatientRef, Template};

/// `report.kind` of a letter.
pub const KIND: &str = "brief";
/// "Sehr geehrte Damen und Herren," when the recipient has no salutation of its own.
pub const DEFAULT_SALUTATION: &str = "Sehr geehrte Damen und Herren,";

/// An image of the letterhead, stored as PNG in the encrypted database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PracticeAsset {
    Logo,
    /// Signature or stamp, printed above the name when [`Practice::signature_image`] is on.
    Signature,
}

impl PracticeAsset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Logo => "logo",
            Self::Signature => "signature",
        }
    }
}

/// The address as printed in the window: name, department, then the address lines.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LetterRecipient {
    #[serde(default)]
    pub kind: Option<RecipientKind>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub department: String,
    #[serde(default)]
    pub lines: Vec<String>,
}

impl LetterRecipient {
    /// Non-empty lines of the address window, top to bottom.
    pub fn address_lines(&self) -> Vec<String> {
        std::iter::once(self.name.as_str())
            .chain(std::iter::once(self.department.as_str()))
            .chain(self.lines.iter().map(String::as_str))
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }

    pub fn is_complete(&self) -> bool {
        !self.name.trim().is_empty() && self.lines.iter().any(|l| !l.trim().is_empty())
    }

    pub fn cleaned(self) -> Self {
        Self {
            kind: self.kind,
            name: self.name.trim().to_string(),
            department: self.department.trim().to_string(),
            lines: self.lines.into_iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
        }
    }
}

/// One entry of the address book.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Recipient {
    pub id: String,
    pub kind: RecipientKind,
    pub name: String,
    pub department: String,
    pub lines: Vec<String>,
    pub salutation: Option<String>,
    /// Document type offered first for this recipient.
    pub default_template: Option<String>,
    #[ts(type = "number")]
    pub use_count: i64,
    pub created_at: String,
    pub updated_at: String,
    /// Only in [`crate::store::Store::patient_recipients`]: the role for that patient.
    #[serde(default)]
    pub role: Option<String>,
}

impl Recipient {
    pub fn as_letter(&self) -> LetterRecipient {
        LetterRecipient { kind: Some(self.kind), name: self.name.clone(), department: self.department.clone(), lines: self.lines.clone() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RecipientInput {
    pub kind: RecipientKind,
    pub name: String,
    #[serde(default)]
    pub department: String,
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default)]
    pub salutation: Option<String>,
    #[serde(default)]
    pub default_template: Option<String>,
}

/// A reference line under the Betreff ("Versicherungsnummer: 12 345678 A 123").
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RefLine {
    pub label: String,
    #[serde(default)]
    pub value: String,
}

/// The letter data around the sections.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Letter {
    pub report_id: String,
    pub patient_id: String,
    pub episode_id: Option<String>,
    pub recipient_id: Option<String>,
    pub recipient: LetterRecipient,
    pub salutation: String,
    pub subject: String,
    pub refs: Vec<RefLine>,
    pub place: Option<String>,
    /// YYYY-MM-DD; `None` = the day it is signed.
    pub letter_date: Option<String>,
    pub sent_at: Option<String>,
    pub sent_via: Option<SentVia>,
    pub follow_up_on: Option<String>,
    pub follow_up_note: Option<String>,
    /// The pasted request ("Anschreiben"), empty when there is none (slice 47).
    #[serde(default)]
    pub request_text: String,
    /// Its questions, in its order and wording.
    #[serde(default)]
    pub questions: Vec<RequestQuestion>,
    /// What "Mit KI schreiben" may use.
    #[serde(default)]
    pub material: Material,
}

/// A question of the request. Once taken over, it is a section of the letter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RequestQuestion {
    pub text: String,
    #[serde(default)]
    pub section_key: Option<String>,
}

/// The material ticked in the rail. Everything is on until she turns it off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Material {
    /// Applied facts of the Behandlungsfall's Sitzungsnotizen.
    #[serde(default = "on")]
    pub facts: bool,
    /// Her answers to the Lotse questions.
    #[serde(default = "on")]
    pub answers: bool,
    /// The last signed Bericht or letter of the Behandlungsfall.
    #[serde(default = "on")]
    pub prior: bool,
    /// Her Textbausteine for the letter type, as preferred wording (slice 48).
    #[serde(default = "on")]
    pub textblocks: bool,
}

fn on() -> bool {
    true
}

impl Default for Material {
    fn default() -> Self {
        Self { facts: true, answers: true, prior: true, textblocks: true }
    }
}

/// A section of the letter as she arranges it from the request: an existing request section
/// (`key`) or a new one, with its heading.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RequestSection {
    #[serde(default)]
    pub key: Option<String>,
    pub heading: String,
    /// The request question it answers (index into `Letter::questions`).
    #[serde(default)]
    pub question: Option<usize>,
}

/// Key prefix of the sections that come from a request.
pub const REQUEST_SECTION_PREFIX: &str = "frage-";

/// The template as this letter has it: template sections without a row are left out (she
/// replaced them by the request's questions), request sections (`heading` set) follow the first
/// sections up to and including the Diagnosen, in their order.
pub fn effective_template(mut template: Template, sections: &[crate::domain::ReportSection]) -> Template {
    let mut extra: Vec<&crate::domain::ReportSection> = sections.iter().filter(|s| s.heading.is_some()).collect();
    if extra.is_empty() {
        return template;
    }
    extra.sort_by_key(|s| s.ord.unwrap_or(i64::MAX));
    template.sections.retain(|d| sections.iter().any(|s| s.key == d.key));
    let at = template
        .sections
        .iter()
        .position(|d| d.field == FieldKind::Diagnoses)
        .map(|i| i + 1)
        .unwrap_or_else(|| usize::from(template.sections.first().is_some_and(|d| d.heading.trim().is_empty())));
    let defs: Vec<crate::templates::SectionDef> = extra
        .into_iter()
        .map(|s| crate::templates::SectionDef {
            key: s.key.clone(),
            number: None,
            heading: s.heading.clone().unwrap_or_default(),
            field: FieldKind::Prose,
            ai_allowed: true,
            required: false,
            optional: false,
            dictation: false,
            starter: None,
            when: Default::default(),
            hints: vec![],
            asks: vec![],
            prior: vec![],
        })
        .collect();
    template.sections.splice(at..at, defs);
    template
}

/// What the therapist edits on a draft letter.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LetterMeta {
    #[serde(default)]
    pub episode_id: Option<String>,
    #[serde(default)]
    pub recipient_id: Option<String>,
    #[serde(default)]
    pub recipient: LetterRecipient,
    #[serde(default)]
    pub salutation: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub refs: Vec<RefLine>,
    #[serde(default)]
    pub place: Option<String>,
    #[serde(default)]
    pub letter_date: Option<String>,
    #[serde(default)]
    pub follow_up_on: Option<String>,
    #[serde(default)]
    pub follow_up_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LetterInput {
    pub patient_id: String,
    pub template_key: String,
    /// Default: the patient's newest open Behandlungsfall.
    #[serde(default)]
    pub episode_id: Option<String>,
    #[serde(default)]
    pub recipient_id: Option<String>,
}

/// A Schweigepflichtentbindung she has on paper (slice 46): for one address book entry, or for
/// every recipient of a kind.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReleaseInput {
    #[serde(default)]
    pub recipient_id: Option<String>,
    #[serde(default)]
    pub scope: Option<RecipientKind>,
    /// YYYY-MM-DD it was signed; default today.
    #[serde(default)]
    pub granted_on: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// Status a letter list shows: the report status, plus "versendet" once sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LetterState {
    Draft,
    Signed,
    Sent,
}

impl LetterState {
    pub fn of(status: ReportStatus, sent_at: Option<&str>) -> Self {
        match (status, sent_at) {
            (ReportStatus::Signed | ReportStatus::Exported, Some(_)) => Self::Sent,
            (ReportStatus::Signed | ReportStatus::Exported, None) => Self::Signed,
            _ => Self::Draft,
        }
    }
}

/// A row of the Briefe list (and the timeline node). The patient's name is shown locally only.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LetterSummary {
    pub report_id: String,
    pub patient_id: String,
    pub patient_name: String,
    pub episode_id: Option<String>,
    pub template_key: String,
    pub template_title: String,
    pub recipient_name: String,
    pub recipient_kind: Option<RecipientKind>,
    pub subject: String,
    pub status: ReportStatus,
    pub state: LetterState,
    pub created_at: String,
    pub updated_at: String,
    pub signed_at: Option<String>,
    pub exported_at: Option<String>,
    pub sent_at: Option<String>,
    pub sent_via: Option<SentVia>,
    pub follow_up_on: Option<String>,
}

// ---------- practice / letterhead ----------

/// Settings → Praxis & Briefkopf. Stored as one JSON value under [`PRACTICE_KEY`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct Practice {
    pub name: String,
    pub subtitle: String,
    pub street: String,
    pub postal_code: String,
    pub city: String,
    pub phone: String,
    pub fax: String,
    pub email: String,
    pub web: String,
    pub bank: String,
    pub iban: String,
    pub bic: String,
    pub tax_no: String,
    /// Under the name in the signature block, one title per line
    /// ("Diplom-Psychologin\nPsychologische Psychotherapeutin (VT)").
    pub title_lines: String,
    /// "Ort" in "Ort, den …"; empty = the practice city.
    pub place: String,
    /// Print the uploaded signature/stamp image above the name (off by default).
    pub signature_image: bool,
}

/// Setting key of [`Practice`].
pub const PRACTICE_KEY: &str = "practice";
/// The free-text header of earlier versions, folded into [`Practice`] once.
pub const LEGACY_HEADER_KEY: &str = "practice_header";

fn join_nonempty<'a>(parts: impl IntoIterator<Item = &'a str>, sep: &str) -> String {
    parts.into_iter().map(str::trim).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(sep)
}

impl Practice {
    pub fn postal_city(&self) -> String {
        join_nonempty([self.postal_code.as_str(), self.city.as_str()], " ")
    }

    /// One line for the Bericht and consent form header: "Praxis X · Straße 1 · 12345 Ort".
    pub fn header_line(&self) -> Option<String> {
        let pc = self.postal_city();
        let line = join_nonempty([self.name.as_str(), self.street.as_str(), pc.as_str()], " · ");
        (!line.is_empty()).then_some(line)
    }

    /// Small sender line above the address window (DIN 5008 Rücksendeangabe).
    pub fn sender_line(&self) -> String {
        self.header_line().unwrap_or_default()
    }

    /// The footer of every letter page, in columns: address, contact, bank, tax.
    pub fn footer_columns(&self) -> Vec<Vec<String>> {
        let col = |items: Vec<(&str, &str)>| -> Vec<String> {
            items
                .into_iter()
                .filter(|(_, v)| !v.trim().is_empty())
                .map(|(l, v)| if l.is_empty() { v.trim().to_string() } else { format!("{l} {}", v.trim()) })
                .collect()
        };
        let pc = self.postal_city();
        [
            col(vec![("", &self.name), ("", &self.street), ("", &pc)]),
            col(vec![("Tel.", &self.phone), ("Fax", &self.fax), ("", &self.email), ("", &self.web)]),
            col(vec![("", &self.bank), ("IBAN", &self.iban), ("BIC", &self.bic)]),
            col(vec![("Steuernummer", &self.tax_no)]),
        ]
        .into_iter()
        .filter(|c| !c.is_empty())
        .collect()
    }

    /// "Ort" for "Ort, den …".
    pub fn place_or_city(&self) -> Option<String> {
        [self.place.trim(), self.city.trim()].into_iter().find(|p| !p.is_empty()).map(str::to_string)
    }

    pub fn titles(&self) -> Vec<String> {
        self.title_lines.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
    }

    /// Reads the free practice header of earlier versions: the first line is the name, a line
    /// starting with a five-digit postcode is "PLZ Ort", the line before it the street, phone,
    /// fax and e-mail by their look; everything else becomes the subtitle.
    pub fn from_legacy_header(text: &str) -> Self {
        let lines: Vec<&str> = text
            .split(['\n', '·', '|'])
            .flat_map(|l| l.split(" - "))
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let mut p = Practice::default();
        let mut rest = Vec::new();
        let is_postal = |l: &str| l.len() > 6 && l[..5].chars().all(|c| c.is_ascii_digit()) && l.as_bytes()[5] == b' ';
        let postal_at = lines.iter().position(|l| is_postal(l));
        for (i, l) in lines.iter().enumerate() {
            let lower = l.to_lowercase();
            if i == 0 {
                p.name = l.to_string();
            } else if Some(i) == postal_at {
                p.postal_code = l[..5].to_string();
                p.city = l[6..].trim().to_string();
            } else if postal_at == Some(i + 1) && p.street.is_empty() {
                p.street = l.to_string();
            } else if lower.starts_with("tel") {
                p.phone = l.trim_start_matches(|c: char| c.is_alphabetic() || c == '.' || c == ':').trim().to_string();
            } else if lower.starts_with("fax") {
                p.fax = l[3..].trim_start_matches([':', '.']).trim().to_string();
            } else if l.contains('@') && !l.contains(' ') {
                p.email = l.to_string();
            } else {
                rest.push(*l);
            }
        }
        p.subtitle = rest.join(" · ");
        p
    }
}

// ---------- placeholders ----------

/// What placeholders are filled from.
pub struct FillContext<'a> {
    pub patient: &'a Patient,
    pub episode: Option<&'a Episode>,
}

/// Placeholders a text may hold; shown in the Textbausteine help (slice 48).
pub const PLACEHOLDERS: &[&str] = &[
    "{Name}",
    "{Vorname}",
    "{Nachname}",
    "{Geburtsdatum}",
    "{Anrede}",
    "{Behandlung seit}",
    "{Diagnosen}",
    "{Krankenkasse}",
    "{Versichertennummer}",
    "{Rentenversicherungsnummer}",
    "{Frau/Herr}",
    "{sie/er}",
    "{ihre/seine}",
    "{Patientin/Patient}",
    "{die Patientin/den Patienten}",
];

/// Fills `{Name}`, `{Geburtsdatum}`, … from the patient record, and `{weiblich/männlich}` pairs
/// (`{Frau/Herrn}`, `{sie/er}`, `{ihre/seine}`, `{Patientin/Patient}`) by Geschlecht: first
/// form for weiblich, second for männlich. Unknown placeholders, values the record doesn't have,
/// and pairs for divers / keine Angabe stay as written, so the therapist sees what to fill in.
pub fn fill(text: &str, ctx: &FillContext<'_>) -> String {
    fill_with(text, ctx, |v| v.to_string())
}

/// [`fill`] with each filled value passed through `escape` (HTML text).
pub fn fill_with(text: &str, ctx: &FillContext<'_>, escape: impl Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find(['}', '{', '\n']) {
            Some(end) if after.as_bytes()[end] == b'}' => {
                let token = &after[..end];
                match value(token.trim(), ctx) {
                    Some(v) => out.push_str(&escape(&v)),
                    None => {
                        out.push('{');
                        out.push_str(token);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn value(token: &str, ctx: &FillContext<'_>) -> Option<String> {
    let p = ctx.patient;
    let some = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    if let Some((f, m)) = token.split_once('/') {
        return match p.gender {
            Some(Gender::W) => Some(f.trim().to_string()),
            Some(Gender::M) => Some(m.trim().to_string()),
            _ => None,
        };
    }
    match token {
        "Name" => Some(format!("{} {}", p.first_name, p.last_name)),
        "Vorname" => Some(p.first_name.clone()),
        "Nachname" => Some(p.last_name.clone()),
        "Geburtsdatum" => p.birth_date.as_deref().map(crate::render::german_date),
        "Anrede" => match p.gender {
            Some(Gender::W) => Some("Frau".into()),
            Some(Gender::M) => Some("Herr".into()),
            _ => None,
        },
        "Behandlung seit" => ctx.episode.map(|e| crate::render::german_date(&e.started_at)),
        "Diagnosen" => ctx
            .episode
            .filter(|e| !e.diagnoses.is_empty())
            .map(|e| e.diagnoses.iter().map(|d| format!("{} {}", d.code, d.label).trim().to_string()).collect::<Vec<_>>().join("; ")),
        "Krankenkasse" => some(&p.contact.insurer),
        "Versichertennummer" => some(&p.contact.insurance_no),
        "Rentenversicherungsnummer" => some(&p.contact.pension_no),
        _ => None,
    }
}

/// A placeholder left in `text` (`{…}` without a line break inside).
pub fn open_placeholder(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let end = text[start..].find('}')? + start;
    let token = &text[start..=end];
    (!token.contains('\n') && token.len() <= 40).then(|| token.to_string())
}

/// The reference value a template asks for, from the patient record.
pub fn patient_ref(p: &Patient, r: PatientRef) -> String {
    match r {
        PatientRef::InsuranceNo => p.contact.insurance_no.clone(),
        PatientRef::PensionNo => p.contact.pension_no.clone(),
        PatientRef::Insurer => p.contact.insurer.clone(),
    }
    .unwrap_or_default()
}

/// Starter text (plain, blank-line paragraphs) as editor HTML.
pub fn starter_html(text: &str) -> String {
    text.split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| format!("<p>{}</p>", escape(p).replace('\n', "<br>")))
        .collect()
}

fn escape(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

// ---------- checks before signing ----------

/// Letter checks on top of [`crate::pii::checklist`]. All advisory: she may send a letter
/// without a reference she doesn't have.
pub fn checklist(letter: &Letter, report: &Report, template: &Template) -> Vec<CheckItem> {
    let item = |key: &str, ok: bool, message: String| CheckItem {
        key: key.to_string(),
        ok,
        severity: "warn".into(),
        message,
        section_key: None,
    };
    let mut items = vec![
        item(
            "letter_recipient",
            letter.recipient.is_complete(),
            if letter.recipient.is_complete() { "Empfänger mit Anschrift".into() } else { "Empfänger oder Anschrift fehlt".into() },
        ),
        item(
            "letter_subject",
            !letter.subject.trim().is_empty(),
            if letter.subject.trim().is_empty() { "Betreff fehlt".into() } else { "Betreff vorhanden".into() },
        ),
    ];
    for def in template.refs.iter().filter(|r| r.required) {
        let filled = letter.refs.iter().any(|r| r.label == def.label && !r.value.trim().is_empty());
        items.push(item(
            "letter_ref",
            filled,
            if filled { format!("{} angegeben", def.label) } else { format!("{} fehlt im Betreff", def.label) },
        ));
    }
    let mut texts = vec![letter.subject.clone(), letter.salutation.clone()];
    texts.extend(letter.refs.iter().map(|r| r.value.clone()));
    texts.extend(
        report
            .sections
            .iter()
            .filter(|s| template.sections.iter().any(|d| d.key == s.key && d.field == FieldKind::Prose))
            .map(|s| crate::pii::plain_text(&s.body)),
    );
    let open = texts.iter().find_map(|t| open_placeholder(t));
    let unsupported: usize = report.sections.iter().map(|s| crate::compose::unsupported_count(&s.body)).sum();
    if unsupported > 0 {
        items.push(CheckItem {
        key: "letter_unsupported".into(),
        ok: false,
        severity: "block".into(),
        message: match unsupported {
            1 => "1 Satz ohne Beleg: behalten oder löschen".into(),
            n => format!("{n} Sätze ohne Beleg: behalten oder löschen"),
        },
        section_key: report.sections.iter().find(|s| crate::compose::unsupported_count(&s.body) > 0).map(|s| s.key.clone()),
        });
    }
    if !letter.questions.is_empty() {
        let body = |key: &str| report.sections.iter().find(|s| s.key == key).map(|s| crate::pii::plain_text(&s.body)).unwrap_or_default();
        let open = letter.questions.iter().filter(|q| q.section_key.as_deref().is_none_or(|k| body(k).trim().is_empty())).count();
        items.push(item(
            "letter_questions",
            open == 0,
            match open {
                0 => "Alle Fragen des Anschreibens beantwortet".into(),
                1 => "1 Frage des Anschreibens noch offen".into(),
                n => format!("{n} Fragen des Anschreibens noch offen"),
            },
        ));
    }
    items.push(item(
        "letter_placeholder",
        open.is_none(),
        match &open {
            Some(p) => format!("Platzhalter {p} ist noch nicht ersetzt"),
            None => "Alle Platzhalter ersetzt".into(),
        },
    ));
    items
}

/// Schweigepflichtentbindung for the letter's recipient (`None` when the type needs none or no
/// recipient is set yet). A warning: she may have it on paper without having recorded it.
pub fn release_check(letter: &Letter, template: &Template, found: Option<&Consent>) -> Option<CheckItem> {
    if !template.release || letter.recipient.name.trim().is_empty() {
        return None;
    }
    let name = letter.recipient.name.trim();
    Some(CheckItem {
        key: "letter_release".into(),
        ok: found.is_some(),
        severity: "warn".into(),
        message: match found {
            Some(c) => format!("Schweigepflichtentbindung für {name} liegt vor (vom {})", crate::render::german_date(&c.granted_at)),
            None => format!("Keine Schweigepflichtentbindung für {name} erfasst"),
        },
        section_key: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgeGroup, Diagnosis, Method, PatientContact, Setting, Stage};

    fn patient(gender: Option<Gender>) -> Patient {
        Patient {
            id: "p".into(),
            external_ref: None,
            first_name: "Anna".into(),
            last_name: "Beispiel".into(),
            birth_date: Some("1985-04-03".into()),
            chiffre: None,
            pronouns: None,
            gender,
            consent_recording: false,
            status: "active".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            archived_at: None,
            contact: PatientContact { pension_no: Some("12 030485 B 123".into()), ..Default::default() },
        }
    }

    fn episode() -> Episode {
        Episode {
            id: "e".into(),
            patient_id: "p".into(),
            method: Method::Vt,
            age_group: AgeGroup::Adult,
            setting: Setting::Einzel,
            stage: Stage::Kzt1,
            started_at: "2025-03-11T09:00:00Z".into(),
            ended_at: None,
            diagnoses: vec![Diagnosis { code: "F32.1".into(), label: "Mittelgradige depressive Episode".into(), certainty: "G".into() }],
            therapy_since: None,
        }
    }

    #[test]
    fn fills_record_fields_and_gender_pairs() {
        let (w, ep) = (patient(Some(Gender::W)), episode());
        let ctx = FillContext { patient: &w, episode: Some(&ep) };
        assert_eq!(
            fill("Beurteilung von {Frau/Herrn} {Name}, geb. {Geburtsdatum}", &ctx),
            "Beurteilung von Frau Anna Beispiel, geb. 03.04.1985"
        );
        assert_eq!(fill("{sie/er} ist seit dem {Behandlung seit} in Behandlung ({Diagnosen})", &ctx),
            "sie ist seit dem 11.03.2025 in Behandlung (F32.1 Mittelgradige depressive Episode)");
        assert_eq!(fill("RV-Nr. {Rentenversicherungsnummer}", &ctx), "RV-Nr. 12 030485 B 123");
        let m = patient(Some(Gender::M));
        let ctx = FillContext { patient: &m, episode: None };
        assert_eq!(fill("{Anrede} {Nachname}, {Patientin/Patient}", &ctx), "Herr Beispiel, Patient");
        // Unknown, missing and divers stay visible.
        assert_eq!(fill("{Behandlung seit} {Krankenkasse} {Foo}", &ctx), "{Behandlung seit} {Krankenkasse} {Foo}");
        let d = patient(Some(Gender::D));
        assert_eq!(fill("{Frau/Herrn} {Name}", &FillContext { patient: &d, episode: None }), "{Frau/Herrn} Anna Beispiel");
        assert_eq!(fill("a { b } c {", &FillContext { patient: &d, episode: None }), "a { b } c {");
        assert_eq!(open_placeholder("geb. {Geburtsdatum}").as_deref(), Some("{Geburtsdatum}"));
        assert_eq!(open_placeholder("ohne"), None);
    }

    #[test]
    fn legacy_header_becomes_fields() {
        let p = Practice::from_legacy_header("Praxis für Psychotherapie Dr. Muster\nMusterstraße 1\n12345 Musterstadt\nTel. 0123 45678\nkontakt@praxis-muster.de");
        assert_eq!(p.name, "Praxis für Psychotherapie Dr. Muster");
        assert_eq!((p.street.as_str(), p.postal_code.as_str(), p.city.as_str()), ("Musterstraße 1", "12345", "Musterstadt"));
        assert_eq!(p.phone, "0123 45678");
        assert_eq!(p.email, "kontakt@praxis-muster.de");
        assert_eq!(p.header_line().as_deref(), Some("Praxis für Psychotherapie Dr. Muster · Musterstraße 1 · 12345 Musterstadt"));
        let one = Practice::from_legacy_header("Praxis Muster · Musterstr. 1");
        assert_eq!((one.name.as_str(), one.subtitle.as_str()), ("Praxis Muster", "Musterstr. 1"));
        assert_eq!(Practice::default().header_line(), None);
    }

    #[test]
    fn footer_skips_empty_columns() {
        let p = Practice { name: "Praxis".into(), iban: "DE00 1234".into(), ..Default::default() };
        assert_eq!(p.footer_columns(), vec![vec!["Praxis".to_string()], vec!["IBAN DE00 1234".to_string()]]);
    }
}
