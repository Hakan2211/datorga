//! The assistant (Phase 3, slice 25): a chat across the app that reads through tools and only
//! proposes changes. Every write tool creates an [`AgentProposal`] row; the therapist applies or
//! discards it ([`Store::apply_proposal`]). There is no tool to sign, export, erase, change
//! consent, diagnoses or settings, and an unknown tool name is refused.
//!
//! Egress: everything sent passes the pseudonymizer with one [`TokenMap`] per thread (stored with
//! the thread) and a dictionary of all patients and their terms, so a name keeps its token for the
//! whole conversation. Messages are stored restored and pseudonymized again for each request.
//! The model's tool arguments come back tokenized and are restored before a tool runs.
//!
//! Times go out as weekday + day.month ("Fr 30.10., 10:00"; another year in brackets). The
//! pseudonymizer leaves yearless dates alone on purpose, so the model can plan; birth dates and
//! dates with a year in note text still become `[DATUM_n]`.

use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use time::{Date, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};
use ts_rs::TS;

use crate::domain::*;
use crate::llm::{ChatOptions, Completion, Effort, Message, Role, SentCall};
use crate::pseudo::{defuse_tokens, Pseudonymizer, TokenMap};
use crate::store::{normalize_ts, Store, TextMode, TextProposal};
use crate::templates::FieldKind;
use crate::{Error, Result};

/// Bump on any change to the system prompt or the tools.
pub const PROMPT_VERSION: &str = "agent-2026.1";
/// Model requests per question (each answer or batch of tool calls is one).
pub const MAX_STEPS: usize = 8;
/// Longest tool result sent; longer ones are cut with a note.
pub const MAX_RESULT_CHARS: usize = 12_000;
/// Tool results from earlier questions are sent shortened.
pub const OLD_RESULT_CHARS: usize = 2_000;
/// Messages of the thread sent along (the window starts at a question).
pub const HISTORY_MESSAGES: usize = 40;
pub const MAX_QUESTION_CHARS: usize = 4_000;
/// Longest text a proposal may carry into a note, Bericht or day plan.
pub const MAX_PROPOSAL_TEXT: usize = 8_000;

pub fn chat_options() -> ChatOptions {
    ChatOptions { temperature: 0.2, max_tokens: 4096, timeout: Duration::from_secs(300), reasoning: Some(Effort::Medium) }
}

// ---------- local time ----------

/// The therapist's time zone as the WebView knows it: Rust has no zone database. `base` applies
/// before the first change; each change (DST) applies from its instant on.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export)]
pub struct Zone {
    /// Minutes east of UTC (CET = 60).
    pub base: i32,
    #[serde(default)]
    pub changes: Vec<ZoneChange>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct ZoneChange {
    /// RFC 3339 instant.
    pub at: String,
    pub offset: i32,
}

/// Now plus the zone: turns stored UTC instants into local days and times and back.
#[derive(Debug, Clone)]
pub struct Clock {
    pub now: OffsetDateTime,
    base: i32,
    changes: Vec<(OffsetDateTime, i32)>,
}

const DAYS: [&str; 7] = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"];
const DAY_NAMES: [&str; 7] = ["Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag", "Sonntag"];

impl Clock {
    pub fn new(zone: &Zone, now: OffsetDateTime) -> Result<Self> {
        let mut changes = zone
            .changes
            .iter()
            .map(|c| Ok((parse_ts(&c.at)?, c.offset)))
            .collect::<Result<Vec<_>>>()?;
        changes.sort_by_key(|c| c.0);
        Ok(Self { now, base: zone.base, changes })
    }

    pub fn current(zone: &Zone) -> Result<Self> {
        Self::new(zone, OffsetDateTime::now_utc())
    }

    fn offset_at(&self, t: OffsetDateTime) -> UtcOffset {
        let minutes = self.changes.iter().take_while(|c| c.0 <= t).last().map_or(self.base, |c| c.1);
        UtcOffset::from_whole_seconds(minutes.clamp(-18 * 60, 18 * 60) * 60).expect("clamped offset")
    }

    pub fn local(&self, t: OffsetDateTime) -> PrimitiveDateTime {
        let t = t.to_offset(self.offset_at(t));
        PrimitiveDateTime::new(t.date(), t.time())
    }

    pub fn today(&self) -> Date {
        self.local(self.now).date()
    }

    /// The instant of a local day and time (the offset from a first guess, checked once more).
    pub fn utc(&self, date: Date, time: Time) -> OffsetDateTime {
        let local = PrimitiveDateTime::new(date, time);
        let guess = local.assume_offset(self.offset_at(local.assume_utc()));
        local.assume_offset(self.offset_at(guess)).to_offset(UtcOffset::UTC)
    }

    /// `[start, end)` of a local day as stored instants.
    pub fn day_range(&self, date: Date) -> (String, String) {
        let next = date.next_day().unwrap_or(date);
        (fmt_ts(self.utc(date, Time::MIDNIGHT)), fmt_ts(self.utc(next, Time::MIDNIGHT)))
    }

    /// "Fr 30.10." (another year: "Fr 30.10. (2025)"). Survives the pseudonymizer unchanged.
    pub fn day(&self, d: Date) -> String {
        let year = if d.year() == self.today().year() { String::new() } else { format!(" ({})", d.year()) };
        format!("{} {}.{}.{year}", DAYS[d.weekday().number_days_from_monday() as usize], d.day(), d.month() as u8)
    }

    /// "Fr 30.10., 10:00" for a stored instant; the input itself if it isn't one.
    pub fn when(&self, ts: &str) -> String {
        match parse_ts(ts) {
            Ok(t) => {
                let l = self.local(t);
                format!("{}, {:02}:{:02}", self.day(l.date()), l.hour(), l.minute())
            }
            Err(_) => ts.to_string(),
        }
    }

    fn today_line(&self) -> String {
        let l = self.local(self.now);
        let d = l.date();
        format!(
            "Heute ist {}, {}.{}. ({}), {:02}:{:02} Uhr.",
            DAY_NAMES[d.weekday().number_days_from_monday() as usize],
            d.day(),
            d.month() as u8,
            d.year(),
            l.hour(),
            l.minute()
        )
    }

    /// A date from a tool argument: `2026-10-30`, `30.10.2026`, `30.10.26`, `30.10.`, `heute`,
    /// `morgen`, `übermorgen`. Without a year: this year, or next year if that is long past.
    pub fn parse_date(&self, s: &str) -> Result<Date> {
        let s = s.trim();
        let today = self.today();
        let bad = || Error::Invalid(format!("Datum nicht lesbar: {s} (erwartet JJJJ-MM-TT)"));
        match s.to_lowercase().as_str() {
            "heute" => return Ok(today),
            "morgen" => return today.next_day().ok_or_else(bad),
            "übermorgen" => return today.next_day().and_then(|d| d.next_day()).ok_or_else(bad),
            _ => {}
        }
        let nums: Vec<i32> = s.split(['-', '.']).map(str::trim).filter(|p| !p.is_empty()).map(|p| p.parse().map_err(|_| bad())).collect::<Result<_>>()?;
        let (y, m, d) = match nums[..] {
            [y, m, d] if s.contains('-') => (y, m, d),
            [d, m, y] if y < 100 => (2000 + y, m, d),
            [d, m, y] => (y, m, d),
            [d, m] => {
                let this = date(today.year(), m, d).ok_or_else(bad)?;
                return Ok(if this < today - time::Duration::days(60) { date(today.year() + 1, m, d).ok_or_else(bad)? } else { this });
            }
            _ => return Err(bad()),
        };
        date(y, m, d).ok_or_else(bad)
    }

    /// `10:00`, `9:30`, `10 Uhr`, `10.30`.
    pub fn parse_time(&self, s: &str) -> Result<Time> {
        let t = s.trim().trim_end_matches("Uhr").trim();
        let bad = || Error::Invalid(format!("Uhrzeit nicht lesbar: {s} (erwartet HH:MM)"));
        let mut parts = t.split([':', '.']);
        let h: u8 = parts.next().and_then(|p| p.trim().parse().ok()).ok_or_else(bad)?;
        let m: u8 = match parts.next() {
            Some(p) => p.trim().parse().map_err(|_| bad())?,
            None => 0,
        };
        Time::from_hms(h, m, 0).map_err(|_| bad())
    }
}

fn date(y: i32, m: i32, d: i32) -> Option<Date> {
    let month = time::Month::try_from(u8::try_from(m).ok()?).ok()?;
    Date::from_calendar_date(y, month, u8::try_from(d).ok()?).ok()
}

fn parse_ts(s: &str) -> Result<OffsetDateTime> {
    OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).map_err(|_| Error::Invalid(format!("Zeitpunkt ungültig: {s}")))
}

fn fmt_ts(t: OffsetDateTime) -> String {
    normalize_ts(&t.format(&time::format_description::well_known::Rfc3339).expect("rfc3339 formatting cannot fail")).expect("valid instant")
}

fn iso(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month() as u8, d.day())
}

// ---------- tools ----------

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": { "type": "object", "properties": properties, "required": required, "additionalProperties": false },
        },
    })
}

fn s(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

/// Names of the read tools.
pub const READ_TOOLS: &[&str] =
    &["search_patients", "get_patient_overview", "list_sessions", "get_session_note", "get_facts", "get_session_summaries", "get_report", "calendar_day", "due_items"];
/// Names of the tools that create a proposal.
pub const PROPOSE_TOOLS: &[&str] =
    &["propose_session", "propose_reschedule", "propose_task", "propose_section_text", "propose_note_text", "propose_day_plan"];

/// The tool definitions sent with every request (no patient data).
pub fn tools() -> Value {
    let date = || s("Datum als JJJJ-MM-TT");
    json!([
        tool("search_patients", "Sucht Patient:innen nach Namen. Leere Suche listet alle aktiven.", json!({ "query": s("Name oder Teil davon; leer für alle") }), &[]),
        tool(
            "get_patient_overview",
            "Überblick über eine Patientin oder einen Patienten: Behandlungen, Anträge, Berichte, letzte und nächste Sitzung, offene Notizen.",
            json!({ "patient_id": s("patient_id aus einem Werkzeugergebnis") }),
            &["patient_id"],
        ),
        tool(
            "list_sessions",
            "Sitzungen in einem Zeitraum (Standard: 14 Tage zurück bis 28 Tage voraus), optional nur für eine Person.",
            json!({ "patient_id": s("optional"), "from": s("erster Tag, JJJJ-MM-TT"), "to": s("letzter Tag, JJJJ-MM-TT") }),
            &[],
        ),
        tool("get_session_note", "Die Sitzungsnotiz einer Sitzung (alle Abschnitte außer Diagnosen).", json!({ "session_id": s("session_id") }), &["session_id"]),
        tool(
            "get_facts",
            "Übernommene Fakten aus den Sitzungsnotizen einer Behandlung, neueste zuerst.",
            json!({ "patient_id": s("patient_id"), "episode_id": s("optional; Standard: die neueste Behandlung") }),
            &["patient_id"],
        ),
        tool(
            "get_session_summaries",
            "KI-Kernpunkte der letzten Sitzungen einer Person (kürzer als die Notizen), neueste zuerst.",
            json!({ "patient_id": s("patient_id"), "limit": { "type": "integer", "description": "Anzahl Sitzungen (Standard 3, höchstens 10)" } }),
            &["patient_id"],
        ),
        tool("get_report", "Ein Bericht oder eine Sitzungsnotiz mit allen Abschnitten (ohne Diagnosen).", json!({ "report_id": s("report_id") }), &["report_id"]),
        tool("calendar_day", "Termine, Aufgaben und Tagesplanung eines Tages.", json!({ "date": date() }), &["date"]),
        tool("due_items", "Was fällig ist: offene Sitzungsnotizen, fehlende Vorbereitung für morgen, Berichte im Entwurf.", json!({}), &[]),
        tool(
            "propose_session",
            "Schlägt einen neuen Termin vor. Legt nichts an: die Therapeutin übernimmt den Vorschlag selbst.",
            json!({
                "patient_id": s("patient_id"),
                "date": date(),
                "time": s("Beginn, HH:MM"),
                "minutes": { "type": "integer", "description": "Dauer in Minuten (Standard 50)" },
                "modality": { "type": "string", "enum": ["praesenz", "video"] },
            }),
            &["patient_id", "date", "time"],
        ),
        tool(
            "propose_reschedule",
            "Schlägt vor, einen geplanten Termin zu verschieben (nur Termine ohne Aufnahme oder Notiz).",
            json!({ "session_id": s("session_id"), "date": date(), "time": s("neuer Beginn, HH:MM"), "minutes": { "type": "integer" } }),
            &["session_id", "date", "time"],
        ),
        tool(
            "propose_task",
            "Schlägt eine Aufgabe für einen Tag vor.",
            json!({ "title": s("kurz"), "date": date(), "time": s("optional, HH:MM"), "patient_id": s("optional"), "session_id": s("optional") }),
            &["title", "date"],
        ),
        tool(
            "propose_section_text",
            "Schlägt Text für einen Abschnitt eines Berichts oder einer Sitzungsnotiz vor (anhängen oder ersetzen). Nicht für Diagnosen.",
            json!({
                "report_id": s("report_id"),
                "section_key": s("Schlüssel des Abschnitts aus get_report"),
                "text": s("Fachtext, Patient:in als \"Pat.\""),
                "mode": { "type": "string", "enum": ["append", "replace"], "description": "Standard: append" },
            }),
            &["report_id", "section_key", "text"],
        ),
        tool(
            "propose_note_text",
            "Schlägt Text für die Sitzungsnotiz einer Sitzung vor (wird angehängt; die Notiz wird beim Übernehmen angelegt, falls es noch keine gibt).",
            json!({ "session_id": s("session_id"), "section_key": s("z. B. verlauf, vereinbarungen"), "text": s("Fachtext") }),
            &["session_id", "section_key", "text"],
        ),
        tool("propose_day_plan", "Schlägt Text für die Tagesplanung eines Tages vor (wird angehängt).", json!({ "date": date(), "text": s("Planungstext") }), &["date", "text"]),
    ])
}

/// What running one tool produced.
pub struct ToolOutcome {
    /// Restored text for the model (pseudonymized when sent).
    pub text: String,
    /// Patients whose data the tool read or proposed for.
    pub patients: Vec<String>,
    pub proposal: Option<NewProposal>,
}

pub struct NewProposal {
    pub kind: ProposalKind,
    pub patient_id: Option<String>,
    pub summary: String,
    pub payload: Value,
}

/// Runs tools for one thread.
pub struct ToolCtx<'a> {
    pub store: &'a Store,
    pub clock: &'a Clock,
    /// Set for a patient or session thread: the tools see only this patient.
    pub scope_patient: Option<String>,
}

fn arg<'v>(args: &'v Value, key: &str) -> Option<&'v str> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty())
}

fn need<'v>(args: &'v Value, key: &str) -> Result<&'v str> {
    arg(args, key).ok_or_else(|| Error::Invalid(format!("Argument fehlt: {key}")))
}

fn name(p: &Patient) -> String {
    format!("{} {}", p.first_name, p.last_name)
}

fn status_word(s: &str) -> &str {
    match s {
        "planned" => "geplant",
        "in_progress" => "läuft",
        "recorded" => "aufgenommen",
        "cancelled" => "abgesagt",
        other => other,
    }
}

fn report_status_word(s: ReportStatus) -> &'static str {
    match s {
        ReportStatus::Draft => "Entwurf",
        ReportStatus::InReview => "in Prüfung",
        ReportStatus::Signed => "unterschrieben",
        ReportStatus::Exported => "exportiert",
    }
}

fn cut(text: String, max: usize) -> String {
    if text.chars().count() <= max {
        return text;
    }
    let mut t: String = text.chars().take(max).collect();
    t.push_str("\n… (gekürzt)");
    t
}

impl ToolCtx<'_> {
    fn allow(&self, patient_id: &str) -> Result<()> {
        match &self.scope_patient {
            Some(p) if p != patient_id => {
                Err(Error::Invalid("Dieses Gespräch gehört zu einer anderen Person; hier sind nur deren Daten sichtbar".into()))
            }
            _ => Ok(()),
        }
    }

    fn patient(&self, id: &str) -> Result<Patient> {
        self.allow(id)?;
        self.store.patient(id)
    }

    fn session(&self, id: &str) -> Result<Session> {
        let s = self.store.session(id)?;
        self.allow(&s.patient_id)?;
        Ok(s)
    }

    fn report(&self, id: &str) -> Result<Report> {
        let r = self.store.report(id)?;
        self.allow(&r.patient_id)?;
        Ok(r)
    }

    fn session_when(&self, s: &Session) -> String {
        s.scheduled_at.as_deref().or(s.started_at.as_deref()).map(|t| self.clock.when(t)).unwrap_or_else(|| "ohne Zeit".into())
    }

    /// Runs `name`. A refused or failed call comes back as an error for the model to read.
    pub fn run(&self, tool: &str, args: &Value) -> Result<ToolOutcome> {
        let read = |text: String, patients: Vec<String>| Ok(ToolOutcome { text: cut(text, MAX_RESULT_CHARS), patients, proposal: None });
        match tool {
            "search_patients" => {
                let found: Vec<Patient> = match &self.scope_patient {
                    Some(p) => vec![self.store.patient(p)?],
                    None => self.store.list_patients(arg(args, "query"))?,
                };
                let mut t = format!("{} Treffer\n", found.len());
                for p in found.iter().take(50) {
                    t.push_str(&format!("- patient_id {}: {}", p.id, name(p)));
                    if let Some(pr) = p.pronouns.as_deref().filter(|x| !x.is_empty()) {
                        t.push_str(&format!(" ({pr})"));
                    }
                    t.push('\n');
                }
                read(t, found.iter().map(|p| p.id.clone()).collect())
            }
            "get_patient_overview" => {
                let p = self.patient(need(args, "patient_id")?)?;
                read(self.overview(&p)?, vec![p.id])
            }
            "list_sessions" => self.list_sessions(args).and_then(|(t, ps)| read(t, ps)),
            "get_session_note" => {
                let s = self.session(need(args, "session_id")?)?;
                let t = match &s.note_id {
                    None => format!("Zur Sitzung {} gibt es noch keine Sitzungsnotiz.", self.session_when(&s)),
                    Some(id) => format!("Sitzungsnotiz zur Sitzung {}\n{}", self.session_when(&s), self.report_text(&self.store.report(id)?)?),
                };
                read(t, vec![s.patient_id])
            }
            "get_facts" => {
                let p = self.patient(need(args, "patient_id")?)?;
                let episodes = self.store.list_episodes(&p.id)?;
                let episode = match arg(args, "episode_id") {
                    Some(e) => episodes.iter().find(|x| x.id == e).ok_or_else(|| Error::NotFound(format!("episode {e}")))?,
                    None => match episodes.iter().max_by(|a, b| a.started_at.cmp(&b.started_at)) {
                        Some(e) => e,
                        None => return read(format!("{} hat noch keine Behandlung.", name(&p)), vec![p.id]),
                    },
                };
                let facts = self.store.episode_facts(&episode.id)?;
                let mut t = format!("{} übernommene Fakten (Behandlung {}), neueste zuerst:\n", facts.len(), episode.id);
                for f in facts.iter().rev().take(80) {
                    let when = f.session_at.as_deref().map(|w| self.clock.when(w)).unwrap_or_default();
                    t.push_str(&format!("- [{}. Sitzung, {when}; {}] {}\n", f.session_no, f.section_key, f.text));
                }
                read(t, vec![p.id])
            }
            "get_session_summaries" => {
                let p = self.patient(need(args, "patient_id")?)?;
                let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(3).clamp(1, 10) as usize;
                let found = self.store.recent_summaries(&p.id, None, limit)?;
                let mut t = format!("Kernpunkte der letzten {} Sitzungen von {} (KI-Zusammenfassung, neueste zuerst):\n", found.len(), name(&p));
                for (at, s) in &found {
                    let when = at.as_deref().map(|w| self.clock.when(w)).unwrap_or_default();
                    let edited = if s.status == "edited" { ", von der Therapeutin bearbeitet" } else { "" };
                    t.push_str(&format!("Sitzung session_id {}, {when}{edited}:\n", s.session_id));
                    for point in &s.points {
                        t.push_str(&format!("- {}\n", point.text));
                    }
                }
                if found.is_empty() {
                    t.push_str("keine\n");
                }
                read(t, vec![p.id])
            }
            "get_report" => {
                let r = self.report(need(args, "report_id")?)?;
                let what = if r.kind == "sitzungsnotiz" { "Sitzungsnotiz" } else { "Bericht" };
                let t = format!("{what} (report_id {}), Status {}\n{}", r.id, report_status_word(r.status), self.report_text(&r)?);
                read(t, vec![r.patient_id])
            }
            "calendar_day" => {
                let d = self.clock.parse_date(need(args, "date")?)?;
                self.day(d).and_then(|(t, ps)| read(t, ps))
            }
            "due_items" => self.due().and_then(|(t, ps)| read(t, ps)),
            n if PROPOSE_TOOLS.contains(&n) => self.propose(n, args),
            other => Err(Error::Invalid(format!("Das Werkzeug „{other}“ gibt es nicht. Unterschreiben, Exportieren, Löschen, Einwilligungen, Diagnosen und Einstellungen sind dem Assistenten nicht möglich."))),
        }
    }

    fn overview(&self, p: &Patient) -> Result<String> {
        let mut t = format!("Name: {} (patient_id {})\n", name(p), p.id);
        if let Some(pr) = p.pronouns.as_deref().filter(|x| !x.is_empty()) {
            t.push_str(&format!("Pronomen: {pr}\n"));
        }
        t.push_str(&format!("Einwilligung Aufnahme: {}\n", if p.consent_recording { "ja" } else { "nein" }));
        let episodes = self.store.list_episodes(&p.id)?;
        if episodes.is_empty() {
            t.push_str("Behandlungen: keine\n");
        }
        for e in &episodes {
            let since = parse_ts(&e.started_at).map(|s| self.clock.day(self.clock.local(s).date())).unwrap_or_default();
            t.push_str(&format!(
                "Behandlung episode_id {}: {}, {}, seit {since}{}\n",
                e.id,
                crate::draft::method_name(e.method),
                if e.age_group == AgeGroup::Kjp { "Kinder/Jugendliche" } else { "Erwachsene" },
                if e.ended_at.is_some() { " (beendet)" } else { "" }
            ));
            for a in self.store.list_applications(&e.id)? {
                let report = match &a.report_id {
                    Some(id) => format!(", Bericht report_id {id} ({})", report_status_word(self.store.report(id)?.status)),
                    None => String::new(),
                };
                t.push_str(&format!("  - {} Nr. {}{report}\n", crate::draft::kind_name(a.kind), a.seq));
            }
        }
        let sessions: Vec<Session> = self.store.list_sessions(Some(&p.id))?.into_iter().filter(|s| s.status != "cancelled").collect();
        let now = fmt_ts(self.clock.now);
        let at = |s: &Session| s.scheduled_at.clone().or(s.started_at.clone()).unwrap_or_default();
        let last = sessions.iter().filter(|s| at(s) < now).max_by_key(|s| at(s));
        let next = sessions.iter().filter(|s| at(s) >= now).min_by_key(|s| at(s));
        t.push_str(&format!("Sitzungen: {}\n", sessions.len()));
        if let Some(s) = last {
            t.push_str(&format!("Letzte: session_id {}, {}{}\n", s.id, self.session_when(s), note_word(s)));
        }
        if let Some(s) = next {
            t.push_str(&format!("Nächste: session_id {}, {}\n", s.id, self.session_when(s)));
        }
        let open: Vec<&Session> =
            sessions.iter().filter(|s| at(s) < now && !matches!(s.note_status, Some(ReportStatus::Signed | ReportStatus::Exported))).collect();
        if !open.is_empty() {
            t.push_str("Sitzungen ohne unterschriebene Notiz:\n");
            for s in open.iter().take(20) {
                t.push_str(&format!("- session_id {}, {}{}\n", s.id, self.session_when(s), note_word(s)));
            }
        }
        Ok(t)
    }

    fn list_sessions(&self, args: &Value) -> Result<(String, Vec<String>)> {
        let today = self.clock.today();
        let from = arg(args, "from").map(|d| self.clock.parse_date(d)).transpose()?.unwrap_or(today - time::Duration::days(14));
        let to = arg(args, "to").map(|d| self.clock.parse_date(d)).transpose()?.unwrap_or(today + time::Duration::days(28));
        if to < from || (to - from).whole_days() > 366 {
            return Err(Error::Invalid("Zeitraum ungültig (höchstens ein Jahr)".into()));
        }
        let only = match arg(args, "patient_id") {
            Some(p) => Some(self.patient(p)?.id),
            None => None,
        };
        let (start, _) = self.clock.day_range(from);
        let (_, end) = self.clock.day_range(to);
        let mut t = format!("Sitzungen {} bis {}:\n", self.clock.day(from), self.clock.day(to));
        let mut patients = Vec::new();
        let mut n = 0;
        for c in self.store.sessions_between(&start, &end)? {
            if only.as_ref().is_some_and(|p| p != &c.session.patient_id) {
                continue;
            }
            n += 1;
            if n > 80 {
                t.push_str("… (weitere ausgelassen; Zeitraum enger wählen)\n");
                break;
            }
            t.push_str(&self.session_line(&c));
            if !patients.contains(&c.session.patient_id) && self.allow(&c.session.patient_id).is_ok() {
                patients.push(c.session.patient_id.clone());
            }
        }
        if n == 0 {
            t.push_str("keine\n");
        }
        Ok((t, patients))
    }

    /// One calendar line; another patient's session in a scoped thread shows as "belegt".
    fn session_line(&self, c: &CalendarSession) -> String {
        let s = &c.session;
        let minutes = s.planned_minutes.unwrap_or(DEFAULT_SESSION_MINUTES);
        if self.allow(&s.patient_id).is_err() {
            return format!("- {}, {minutes} Min.: belegt (andere Person)\n", self.session_when(s));
        }
        format!(
            "- session_id {}: {}, {minutes} Min., {} (patient_id {}), {}. Sitzung, {}{}\n",
            s.id,
            self.session_when(s),
            c.patient_name,
            s.patient_id,
            c.number,
            status_word(&s.status),
            note_word(s)
        )
    }

    fn day(&self, d: Date) -> Result<(String, Vec<String>)> {
        let (start, end) = self.clock.day_range(d);
        let mut t = format!("{}:\nTermine:\n", self.clock.day(d));
        let mut patients = Vec::new();
        let sessions = self.store.sessions_between(&start, &end)?;
        for c in sessions.iter().filter(|c| c.session.status != "cancelled") {
            t.push_str(&self.session_line(c));
            if self.allow(&c.session.patient_id).is_ok() {
                patients.push(c.session.patient_id.clone());
            }
        }
        if sessions.is_empty() {
            t.push_str("keine\n");
        }
        let tasks: Vec<Task> = self
            .store
            .list_tasks(&iso(d), &iso(d), false)?
            .into_iter()
            .filter(|task| task.patient_id.as_deref().is_none_or(|p| self.allow(p).is_ok()))
            .collect();
        t.push_str("Aufgaben:\n");
        for task in &tasks {
            let time = task.time.as_deref().map(|x| format!("{x} ")).unwrap_or_default();
            t.push_str(&format!("- {time}{}{}\n", task.title, if task.done { " (erledigt)" } else { "" }));
        }
        if tasks.is_empty() {
            t.push_str("keine\n");
        }
        if let Some(note) = self.store.day_note(&iso(d))? {
            t.push_str(&format!("Tagesplanung:\n{note}\n"));
        }
        Ok((t, patients))
    }

    fn due(&self) -> Result<(String, Vec<String>)> {
        let today = self.clock.today();
        let (from, _) = self.clock.day_range(today - time::Duration::days(60));
        let (day_start, _) = self.clock.day_range(today);
        let (_, until) = self.clock.day_range(today.next_day().unwrap_or(today));
        let items: Vec<DueItem> = self.store.due_items(&from, &day_start, &until)?.into_iter().filter(|i| self.allow(&i.patient_id).is_ok()).collect();
        let mut t = String::from("Fällig:\n");
        for i in &items {
            let when = i.at.as_deref().map(|w| self.clock.when(w)).unwrap_or_default();
            let line = match i.kind {
                DueKind::NoteOpen => format!("Sitzungsnotiz offen: {} (patient_id {}), Sitzung {when}, session_id {}", i.patient_name, i.patient_id, i.session_id.as_deref().unwrap_or("")),
                DueKind::PrepMissing => format!("Vorbereitung fehlt: {} (patient_id {}), Sitzung {when}, session_id {}", i.patient_name, i.patient_id, i.session_id.as_deref().unwrap_or("")),
                DueKind::ReportDraft => format!(
                    "Bericht im Entwurf: {} (patient_id {}), {}, report_id {}, zuletzt geändert {when}",
                    i.patient_name,
                    i.patient_id,
                    i.application_kind.map(crate::draft::kind_name).unwrap_or("Bericht"),
                    i.report_id.as_deref().unwrap_or("")
                ),
                DueKind::TranscriptReview => format!("Transkript prüfen: {} (patient_id {}), Sitzung {when}, session_id {}", i.patient_name, i.patient_id, i.session_id.as_deref().unwrap_or("")),
                DueKind::ApplicationDue => {
                    let c = i.contingent.as_ref();
                    format!(
                        "{} vorbereiten: {} (patient_id {}), {} von {} Sitzungen",
                        i.application_kind.map(crate::draft::kind_name).unwrap_or("Antrag"),
                        i.patient_name,
                        i.patient_id,
                        c.map_or(0, |c| c.used),
                        c.map_or(0, |c| c.limit)
                    )
                }
            };
            t.push_str(&format!("- {line}\n"));
        }
        if items.is_empty() {
            t.push_str("nichts\n");
        }
        Ok((t, items.iter().map(|i| i.patient_id.clone()).collect()))
    }

    /// Sections in template order as plain text, diagnoses left out.
    fn report_text(&self, r: &Report) -> Result<String> {
        let template = self.store.template(&r.template_key)?;
        let mut t = String::new();
        for def in &template.sections {
            let Some(sec) = r.sections.iter().find(|s| s.key == def.key) else { continue };
            let heading = format!("{}{}", def.number.as_deref().map(|n| format!("{n} ")).unwrap_or_default(), def.heading.trim());
            if def.field == FieldKind::Diagnoses {
                t.push_str(&format!("## {heading} [{}]\n(Diagnosen werden nicht an den Assistenten gegeben)\n", def.key));
                continue;
            }
            let body = crate::draft::plain_text(&sec.body);
            t.push_str(&format!("## {heading} [{}]\n{}\n", def.key, if body.trim().is_empty() { "(leer)" } else { body.trim() }));
        }
        Ok(t)
    }

    fn propose(&self, tool: &str, args: &Value) -> Result<ToolOutcome> {
        let (kind, payload, summary, note) = match tool {
            "propose_session" => {
                let p = self.patient(need(args, "patient_id")?)?;
                let d = self.clock.parse_date(need(args, "date")?)?;
                let at = self.clock.utc(d, self.clock.parse_time(need(args, "time")?)?);
                if at < self.clock.now {
                    return Err(Error::Invalid("Der Zeitpunkt liegt in der Vergangenheit".into()));
                }
                let minutes = args.get("minutes").and_then(Value::as_i64);
                let episode = self.store.list_episodes(&p.id)?.into_iter().filter(|e| e.ended_at.is_none()).max_by(|a, b| a.started_at.cmp(&b.started_at));
                let input = SessionInput {
                    patient_id: p.id.clone(),
                    episode_id: episode.map(|e| e.id),
                    scheduled_at: Some(fmt_ts(at)),
                    modality: arg(args, "modality").map(str::to_string),
                    planned_minutes: minutes,
                };
                let ts = fmt_ts(at);
                let summary = format!("Termin: {}, {} ({} Min.)", name(&p), self.clock.when(&ts), minutes.unwrap_or(DEFAULT_SESSION_MINUTES));
                let note = self.overlap(at, minutes.unwrap_or(DEFAULT_SESSION_MINUTES), None)?;
                (ProposalKind::Session, serde_json::to_value(input)?, summary, note)
            }
            "propose_reschedule" => {
                let s = self.session(need(args, "session_id")?)?;
                let d = self.clock.parse_date(need(args, "date")?)?;
                let at = self.clock.utc(d, self.clock.parse_time(need(args, "time")?)?);
                if at < self.clock.now {
                    return Err(Error::Invalid("Der neue Zeitpunkt liegt in der Vergangenheit".into()));
                }
                let minutes = args.get("minutes").and_then(Value::as_i64).or(s.planned_minutes);
                let p = self.store.patient(&s.patient_id)?;
                let update = SessionUpdate { scheduled_at: fmt_ts(at), planned_minutes: minutes, modality: s.modality.clone() };
                let summary = format!("Verschieben: {}, {} → {}", name(&p), self.session_when(&s), self.clock.when(&update.scheduled_at));
                let note = self.overlap(at, minutes.unwrap_or(DEFAULT_SESSION_MINUTES), Some(&s.id))?;
                (ProposalKind::Reschedule, json!({ "session_id": s.id, "update": update }), summary, note)
            }
            "propose_task" => {
                let d = self.clock.parse_date(need(args, "date")?)?;
                let time = arg(args, "time").map(|t| self.clock.parse_time(t)).transpose()?.map(|t| format!("{:02}:{:02}", t.hour(), t.minute()));
                let session_id = arg(args, "session_id").map(|id| self.session(id)).transpose()?;
                let patient_id = match (arg(args, "patient_id"), &session_id) {
                    (Some(p), _) => Some(self.patient(p)?.id),
                    (None, Some(s)) => Some(s.patient_id.clone()),
                    (None, None) => self.scope_patient.clone(),
                };
                let input = TaskInput { date: iso(d), time, title: need(args, "title")?.to_string(), patient_id, session_id: session_id.map(|s| s.id) };
                let summary = format!("Aufgabe für {}: {}", self.clock.day(d), input.title);
                (ProposalKind::Task, serde_json::to_value(input)?, summary, None)
            }
            "propose_section_text" | "propose_note_text" => {
                let (report, session) = if tool == "propose_section_text" {
                    (Some(self.report(need(args, "report_id")?)?), None)
                } else {
                    let s = self.session(need(args, "session_id")?)?;
                    (s.note_id.as_deref().map(|id| self.store.report(id)).transpose()?, Some(s))
                };
                let mut text = need(args, "text")?.to_string();
                let patient_id = report.as_ref().map(|r| r.patient_id.clone()).or(session.as_ref().map(|s| s.patient_id.clone())).unwrap_or_default();
                if report.as_ref().is_some_and(|r| r.kind != "sitzungsnotiz") {
                    // A Bericht goes to the Gutachter under a Chiffre (slice 14 rule).
                    text = crate::draft::patient_as_pat(&text, &self.store.patient(&patient_id)?);
                }
                let mode = match arg(args, "mode") {
                    Some("replace") if tool == "propose_section_text" => TextMode::Replace,
                    _ => TextMode::Append,
                };
                let p = TextProposal {
                    report_id: report.as_ref().map(|r| r.id.clone()),
                    session_id: session.as_ref().map(|s| s.id.clone()),
                    section_key: need(args, "section_key")?.to_string(),
                    heading: String::new(),
                    text,
                    mode,
                };
                let p = self.store.check_text_proposal(p)?;
                let target = match &session {
                    Some(s) => format!("Sitzungsnotiz vom {}", self.session_when(s)),
                    None => format!("{} von {}", if report.as_ref().is_some_and(|r| r.kind == "sitzungsnotiz") { "Sitzungsnotiz" } else { "Bericht" }, name(&self.store.patient(&patient_id)?)),
                };
                let verb = if p.mode == TextMode::Replace { "ersetzen" } else { "ergänzen" };
                let summary = format!("{target}: „{}“ {verb}", p.heading);
                let kind = if tool == "propose_section_text" { ProposalKind::SectionText } else { ProposalKind::NoteText };
                (kind, serde_json::to_value(p)?, summary, None)
            }
            "propose_day_plan" => {
                let d = self.clock.parse_date(need(args, "date")?)?;
                let summary = format!("Tagesplanung für {} ergänzen", self.clock.day(d));
                (ProposalKind::DayPlan, json!({ "date": iso(d), "text": need(args, "text")? }), summary, None)
            }
            _ => unreachable!("only PROPOSE_TOOLS"),
        };
        let patient_id = self.store.check_proposal(kind, &payload)?;
        let mut text = format!("Vorschlag angelegt: {summary}. Er wird erst wirksam, wenn die Therapeutin ihn übernimmt.");
        if let Some(n) = note {
            text.push_str(&format!("\nHinweis: {n}"));
        }
        Ok(ToolOutcome {
            text,
            patients: patient_id.iter().cloned().collect(),
            proposal: Some(NewProposal { kind, patient_id, summary, payload }),
        })
    }

    /// Sessions overlapping `[at, at + minutes)`, as a hint for the model and the card.
    fn overlap(&self, at: OffsetDateTime, minutes: i64, except: Option<&str>) -> Result<Option<String>> {
        let day = self.clock.local(at).date();
        let (start, end) = self.clock.day_range(day);
        let until = at + time::Duration::minutes(minutes);
        let clash: Vec<String> = self
            .store
            .sessions_between(&start, &end)?
            .into_iter()
            .filter(|c| c.session.status != "cancelled" && Some(c.session.id.as_str()) != except)
            .filter(|c| {
                let Some(s) = c.session.scheduled_at.as_deref().and_then(|t| parse_ts(t).ok()) else { return false };
                s < until && at < s + time::Duration::minutes(c.session.planned_minutes.unwrap_or(DEFAULT_SESSION_MINUTES))
            })
            .map(|c| self.session_when(&c.session))
            .collect();
        Ok((!clash.is_empty()).then(|| format!("überschneidet sich mit einem Termin ({})", clash.join("; "))))
    }
}

fn note_word(s: &Session) -> &'static str {
    match s.note_status {
        None => ", keine Notiz",
        Some(ReportStatus::Signed | ReportStatus::Exported) => ", Notiz unterschrieben",
        Some(_) => ", Notiz in Arbeit",
    }
}

/// "Was ich getan habe": one German line per tool call (shown locally, never sent).
pub fn step_label(name: &str, args: &Value) -> String {
    let a = |k: &str| arg(args, k).unwrap_or("").to_string();
    match name {
        "search_patients" if a("query").is_empty() => "Patient:innen aufgelistet".into(),
        "search_patients" => format!("Patient:innen gesucht: „{}“", a("query")),
        "get_patient_overview" => "Überblick über eine Person gelesen".into(),
        "list_sessions" => "Sitzungen aufgelistet".into(),
        "get_session_note" => "Sitzungsnotiz gelesen".into(),
        "get_facts" => "Fakten aus den Notizen gelesen".into(),
        "get_session_summaries" => "Kernpunkte der letzten Sitzungen gelesen".into(),
        "get_report" => "Bericht gelesen".into(),
        "calendar_day" => {
            let d = a("date");
            let parts: Vec<&str> = d.split('-').collect();
            let shown = match parts[..] {
                [y, m, day] if y.len() == 4 => format!("{}.{}.{y}", day.trim_start_matches('0'), m.trim_start_matches('0')),
                _ => d,
            };
            format!("Kalender angesehen: {shown}")
        }
        "due_items" => "Fällige Aufgaben angesehen".into(),
        "propose_session" => "Termin vorgeschlagen".into(),
        "propose_reschedule" => "Verschiebung vorgeschlagen".into(),
        "propose_task" => "Aufgabe vorgeschlagen".into(),
        "propose_section_text" | "propose_note_text" => "Text vorgeschlagen".into(),
        "propose_day_plan" => "Tagesplanung vorgeschlagen".into(),
        other => format!("Unbekanntes Werkzeug abgelehnt: {other}"),
    }
}

// ---------- the turn ----------

fn system_prompt(store: &Store, thread: &ChatThread, clock: &Clock) -> Result<String> {
    let scope = match thread.scope {
        ChatScope::Global => String::new(),
        ChatScope::Patient => {
            let p = store.patient(thread.ref_id.as_deref().unwrap_or_default())?;
            format!("Dieses Gespräch gehört zu {} (patient_id {}). Die Werkzeuge zeigen nur deren Daten.\n", name(&p), p.id)
        }
        ChatScope::Session => {
            let s = store.session(thread.ref_id.as_deref().unwrap_or_default())?;
            let p = store.patient(&s.patient_id)?;
            let when = s.scheduled_at.as_deref().or(s.started_at.as_deref()).map(|t| clock.when(t)).unwrap_or_default();
            format!(
                "Dieses Gespräch gehört zur Sitzung {when} (session_id {}) mit {} (patient_id {}). Die Werkzeuge zeigen nur deren Daten.\n",
                s.id,
                name(&p),
                p.id
            )
        }
    };
    Ok(format!(
        "Du bist der Assistent in thera, der Dokumentationssoftware einer psychotherapeutischen Praxis. \
         Du hilfst der Therapeutin, sich zurechtzufinden, Termine und Aufgaben zu planen und Texte für Notizen und Berichte vorzubereiten.\n\
         {}\n\
         {scope}\n\
         Regeln:\n\
         - Du liest mit den Werkzeugen und legst mit den propose_-Werkzeugen Vorschläge an. Du änderst selbst nie etwas: \
         Jeder Vorschlag erscheint als Karte, die Therapeutin übernimmt oder verwirft ihn. Sag nie, etwas sei erledigt; sag, dass ein Vorschlag bereitliegt.\n\
         - Nicht möglich und abzulehnen: unterschreiben, exportieren, löschen, Einwilligungen, Diagnosen und ICD-Codes, Medikamente, Einstellungen.\n\
         - Namen, Orte und manche Daten erscheinen als Platzhalter wie [PERSON_1]. Übernimm sie unverändert, auch in Werkzeug-Argumenten; \
         die App setzt die echten Angaben ein. Erfinde keine Platzhalter.\n\
         - IDs (patient_id, session_id, report_id) nur aus Werkzeugergebnissen übernehmen, nie raten. Erst suchen, dann vorschlagen.\n\
         - In Werkzeug-Argumenten: Datum als JJJJ-MM-TT, Uhrzeit als HH:MM. Daten ohne Jahr in Ergebnissen liegen im aktuellen Jahr.\n\
         - Texte für Notizen und Berichte: sachlich, in ganzen Sätzen, die Patientin oder der Patient heißt \"Pat.\". \
         Nur was in den Quellen steht, nichts erfinden, keine Diagnosen.\n\
         - Antworte kurz und auf Deutsch. Frag nach, wenn etwas unklar ist (z. B. welche Person gemeint ist).\n",
        clock.today_line()
    ))
}

/// The patient a thread is limited to.
pub fn scope_patient(store: &Store, thread: &ChatThread) -> Result<Option<String>> {
    Ok(match thread.scope {
        ChatScope::Global => None,
        ChatScope::Patient => thread.ref_id.clone(),
        ChatScope::Session => Some(store.session(thread.ref_id.as_deref().unwrap_or_default())?.patient_id),
    })
}

fn safe(p: &mut Pseudonymizer, text: &str) -> Result<crate::pseudo::Pseudonymized<String>> {
    p.pseudonymize(&defuse_tokens(text))
}

/// The request for the next step: system prompt and the recent thread, pseudonymized with the
/// thread's map (new tokens are stored with the thread).
pub fn prepare(store: &Store, thread_id: &str, clock: &Clock) -> Result<(Vec<Message>, TokenMap)> {
    let thread = store.chat_thread(thread_id)?;
    let mut p = Pseudonymizer::with_map(store.assistant_dictionary()?, store.chat_thread_map(thread_id)?);
    let mut messages = vec![Message::Text(Role::System, safe(&mut p, &system_prompt(store, &thread, clock)?)?)];
    let history = store.chat_messages(thread_id)?;
    let mut start = history.len().saturating_sub(HISTORY_MESSAGES);
    while start < history.len() && history[start].role != ChatRole::User {
        start += 1;
    }
    let current = history.iter().rposition(|m| m.role == ChatRole::User).unwrap_or(0);
    for (i, m) in history.iter().enumerate().skip(start) {
        messages.push(match m.role {
            ChatRole::User => Message::Text(Role::User, safe(&mut p, &m.content)?),
            ChatRole::Assistant if m.tool_calls.is_empty() => Message::Text(Role::Assistant, safe(&mut p, &m.content)?),
            ChatRole::Assistant => Message::Calls {
                content: if m.content.trim().is_empty() { None } else { Some(safe(&mut p, &m.content)?) },
                calls: m
                    .tool_calls
                    .iter()
                    .map(|c| {
                        let args = match &c.arguments {
                            Value::String(raw) => raw.clone(),
                            v => v.to_string(),
                        };
                        Ok(SentCall { id: c.id.clone(), name: c.name.clone(), arguments: safe(&mut p, &args)? })
                    })
                    .collect::<Result<_>>()?,
            },
            ChatRole::Tool => {
                let text = if i < current { cut(m.content.clone(), OLD_RESULT_CHARS) } else { m.content.clone() };
                Message::ToolResult { call_id: m.tool_call_id.clone().unwrap_or_default(), content: safe(&mut p, &text)? }
            }
        });
    }
    let map = p.into_map();
    store.set_chat_thread_map(thread_id, &map)?;
    Ok((messages, map))
}

/// Restores tokens in every string of the model's JSON arguments. Unreadable JSON is kept as
/// restored text (the tool then reports it).
pub fn restore_args(map: &TokenMap, raw: &str) -> Value {
    fn walk(map: &TokenMap, v: Value) -> Value {
        match v {
            Value::String(s) => Value::String(map.restore(&s).text),
            Value::Array(a) => Value::Array(a.into_iter().map(|x| walk(map, x)).collect()),
            Value::Object(o) => Value::Object(o.into_iter().map(|(k, x)| (k, walk(map, x))).collect()),
            other => other,
        }
    }
    let raw = if raw.trim().is_empty() { "{}" } else { raw };
    match serde_json::from_str::<Value>(raw) {
        Ok(v) => walk(map, v),
        Err(_) => Value::String(map.restore(raw).text),
    }
}

static OPEN_TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[A-Z_]{0,12}[0-9]{0,4}$").expect("valid regex"));

/// Streamed text restored for display. A token cut off at the end ("[PERS") is held back.
pub fn restore_partial(map: &TokenMap, text: &str) -> String {
    let end = OPEN_TOKEN.find(text).map_or(text.len(), |m| m.start());
    map.restore(&text[..end]).text
}

/// What the app gives the loop: store access (locked only briefly) and the model.
pub trait Host {
    fn store<T>(&mut self, f: impl FnOnce(&Store) -> Result<T>) -> Result<T>;
    /// One model request. `Ok(None)`: cancelled. `map` restores streamed text for display.
    fn complete(&mut self, messages: &[Message], tools: &Value, map: &TokenMap) -> Result<Option<Completion>>;
    /// A message or proposal was stored.
    fn changed(&mut self) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnEnd {
    Answered,
    Cancelled,
    /// [`MAX_STEPS`] requests without a final answer.
    StepLimit,
}

/// One question: stores it, then lets the model read and propose until it answers.
pub fn run_turn<H: Host>(host: &mut H, thread_id: &str, question: &str, clock: &Clock) -> Result<TurnEnd> {
    let question = question.trim();
    if question.is_empty() {
        return Err(Error::Invalid("Die Frage ist leer".into()));
    }
    if question.chars().count() > MAX_QUESTION_CHARS {
        return Err(Error::Invalid(format!("Die Frage ist zu lang (höchstens {MAX_QUESTION_CHARS} Zeichen)")));
    }
    if defuse_tokens(question) != question {
        return Err(Error::Invalid("Platzhalter wie [PERSON_1] bitte nicht selbst eingeben".into()));
    }
    host.store(|s| s.add_chat_message(thread_id, ChatRole::User, question, &[], None))?;
    host.changed();
    let tools = tools();
    for _ in 0..MAX_STEPS {
        let (messages, map) = host.store(|s| prepare(s, thread_id, clock))?;
        let Some(c) = host.complete(&messages, &tools, &map)? else { return Ok(TurnEnd::Cancelled) };
        let text = map.restore(&c.text).text;
        if c.tool_calls.is_empty() {
            host.store(|s| s.add_chat_message(thread_id, ChatRole::Assistant, text.trim(), &[], None))?;
            host.changed();
            return Ok(TurnEnd::Answered);
        }
        let calls: Vec<ChatToolCall> = c
            .tool_calls
            .iter()
            .enumerate()
            .map(|(i, tc)| ChatToolCall {
                id: if tc.id.is_empty() { format!("call_{}_{i}", new_id().split('-').next().unwrap_or("x")) } else { tc.id.clone() },
                name: tc.name.clone(),
                arguments: restore_args(&map, &tc.arguments),
            })
            .collect();
        host.store(|s| s.add_chat_message(thread_id, ChatRole::Assistant, text.trim(), &calls, None))?;
        host.changed();
        for call in &calls {
            host.store(|s| {
                let thread = s.chat_thread(thread_id)?;
                let ctx = ToolCtx { store: s, clock, scope_patient: scope_patient(s, &thread)? };
                let outcome = match &call.arguments {
                    Value::Object(_) => ctx.run(&call.name, &call.arguments),
                    _ => Err(Error::Invalid("Die Argumente sind kein gültiges JSON-Objekt".into())),
                };
                let outcome = outcome.unwrap_or_else(|e| ToolOutcome { text: format!("Fehler: {}", error_text(&e)), patients: vec![], proposal: None });
                s.add_tool_result(thread_id, call, outcome)
            })?;
            host.changed();
        }
    }
    host.store(|s| {
        s.add_chat_message(
            thread_id,
            ChatRole::Assistant,
            &format!("Ich habe nach {MAX_STEPS} Schritten aufgehört, ohne fertig zu werden. Bitte die Frage enger stellen."),
            &[],
            None,
        )
    })?;
    host.changed();
    Ok(TurnEnd::StepLimit)
}

fn error_text(e: &Error) -> String {
    match e {
        Error::Invalid(m) | Error::Llm(m) | Error::Stt(m) => m.clone(),
        Error::NotFound(what) => format!("nicht gefunden ({what}); IDs nur aus Werkzeugergebnissen übernehmen"),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pseudo::Dictionary;

    fn clock() -> Clock {
        // Wed 28.10.2026 14:05 local (CET since the change on 25.10.).
        let zone = Zone { base: 120, changes: vec![ZoneChange { at: "2026-10-25T01:00:00Z".into(), offset: 60 }, ZoneChange { at: "2027-03-28T01:00:00Z".into(), offset: 120 }] };
        Clock::new(&zone, parse_ts("2026-10-28T13:05:00Z").unwrap()).unwrap()
    }

    #[test]
    fn local_times_follow_the_zone_changes() {
        let c = clock();
        assert_eq!(c.today(), date(2026, 10, 28).unwrap());
        assert_eq!(c.today_line(), "Heute ist Mittwoch, 28.10. (2026), 14:05 Uhr.");
        assert_eq!(c.when("2026-10-30T09:00:00Z"), "Fr 30.10., 10:00");
        assert_eq!(c.when("2026-10-23T08:00:00Z"), "Fr 23.10., 10:00", "summer time before the change");
        assert_eq!(c.when("2025-10-14T08:00:00Z"), "Di 14.10. (2025), 10:00");
        assert_eq!(fmt_ts(c.utc(date(2026, 10, 30).unwrap(), Time::from_hms(10, 0, 0).unwrap())), "2026-10-30T09:00:00Z");
        assert_eq!(fmt_ts(c.utc(date(2027, 4, 1).unwrap(), Time::from_hms(10, 0, 0).unwrap())), "2027-04-01T08:00:00Z");
        assert_eq!(c.day_range(date(2026, 10, 28).unwrap()), ("2026-10-27T23:00:00Z".into(), "2026-10-28T23:00:00Z".into()));
    }

    #[test]
    fn dates_and_times_from_the_model_are_read_leniently() {
        let c = clock();
        let d = |y, m, dd| date(y, m, dd).unwrap();
        assert_eq!(c.parse_date("2026-11-03").unwrap(), d(2026, 11, 3));
        assert_eq!(c.parse_date("3.11.2026").unwrap(), d(2026, 11, 3));
        assert_eq!(c.parse_date("3.11.26").unwrap(), d(2026, 11, 3));
        assert_eq!(c.parse_date("3.11.").unwrap(), d(2026, 11, 3));
        assert_eq!(c.parse_date("5.1.").unwrap(), d(2027, 1, 5), "long past = next year");
        assert_eq!(c.parse_date("morgen").unwrap(), d(2026, 10, 29));
        assert!(c.parse_date("[DATUM_1]").is_err());
        assert!(c.parse_date("2026-02-30").is_err());
        assert_eq!(c.parse_time("9:30").unwrap(), Time::from_hms(9, 30, 0).unwrap());
        assert_eq!(c.parse_time("10 Uhr").unwrap(), Time::from_hms(10, 0, 0).unwrap());
        assert!(c.parse_time("25:00").is_err());
    }

    #[test]
    fn calendar_formats_pass_the_pseudonymizer_unchanged() {
        let c = clock();
        let mut p = Pseudonymizer::new(Dictionary::new());
        for text in [c.today_line(), c.when("2026-10-30T09:00:00Z"), c.when("2025-10-14T08:00:00Z"), format!("Sitzungen {} bis {}:", c.day(c.today()), c.day(date(2026, 12, 1).unwrap()))] {
            assert_eq!(p.pseudonymize(&text).unwrap().get(), &text);
        }
        // A date with a year in free text still becomes a token.
        assert_eq!(p.pseudonymize("am 30.10.2026").unwrap().get(), "am [DATUM_1]");
    }

    #[test]
    fn partial_tokens_are_held_back_while_streaming() {
        let mut p = Pseudonymizer::new(Dictionary::new());
        p.pseudonymize("Frau Beispiel").unwrap();
        let map = p.into_map();
        assert_eq!(restore_partial(&map, "Termin mit [PERS"), "Termin mit ");
        assert_eq!(restore_partial(&map, "Termin mit [PERSON_1]"), "Termin mit Beispiel");
        assert_eq!(restore_partial(&map, "Liste: [a] und ["), "Liste: [a] und ");
    }

    #[test]
    fn tool_arguments_are_restored_inside_json() {
        let mut p = Pseudonymizer::new(Dictionary::new());
        p.pseudonymize("Frau Beispiel").unwrap();
        let map = p.into_map();
        assert_eq!(restore_args(&map, r#"{"query":"[PERSON_1]","n":[1,"[PERSON_1]"]}"#), json!({ "query": "Beispiel", "n": [1, "Beispiel"] }));
        assert_eq!(restore_args(&map, ""), json!({}));
        assert_eq!(restore_args(&map, "{kaputt [PERSON_1]"), json!("{kaputt Beispiel"));
    }

    #[test]
    fn step_labels_are_german() {
        assert_eq!(step_label("calendar_day", &json!({ "date": "2026-10-29" })), "Kalender angesehen: 29.10.2026");
        assert_eq!(step_label("search_patients", &json!({ "query": "Brenner" })), "Patient:innen gesucht: „Brenner“");
        assert_eq!(step_label("sign_report", &json!({})), "Unbekanntes Werkzeug abgelehnt: sign_report");
    }

    #[test]
    fn no_tool_can_sign_export_erase_or_touch_consent_diagnoses_or_settings() {
        let names: Vec<String> = tools().as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names.len(), READ_TOOLS.len() + PROPOSE_TOOLS.len());
        for n in &names {
            assert!(READ_TOOLS.contains(&n.as_str()) || PROPOSE_TOOLS.contains(&n.as_str()), "{n}");
            for banned in ["sign", "export", "erase", "delete", "consent", "diagnos", "setting", "icd"] {
                assert!(!n.contains(banned), "{n}");
            }
        }
    }
}
