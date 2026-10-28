//! iCalendar (.ics) export of planned sessions (slice 22). Chiffre only: no names, no birth
//! dates, no note text leave the app this way.

use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::domain::{CalendarSession, DEFAULT_SESSION_MINUTES};
use crate::{Error, Result};

/// One VCALENDAR with a VEVENT per session that has a time. `stamp` is DTSTAMP (RFC 3339).
pub fn ics(sessions: &[CalendarSession], stamp: &str) -> Result<String> {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".into(),
        "PRODID:-//datorga//thera//DE".into(),
        "CALSCALE:GREGORIAN".into(),
    ];
    let stamp = ics_time(parse(stamp)?);
    for c in sessions {
        let Some(at) = c.session.scheduled_at.as_deref().or(c.session.started_at.as_deref()) else { continue };
        let start = parse(at)?;
        let end = start + Duration::minutes(c.session.planned_minutes.unwrap_or(DEFAULT_SESSION_MINUTES));
        let who = c.chiffre.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or("ohne Chiffre");
        lines.push("BEGIN:VEVENT".into());
        lines.push(format!("UID:{}@thera.datorga.de", c.session.id));
        lines.push(format!("DTSTAMP:{stamp}"));
        lines.push(format!("DTSTART:{}", ics_time(start)));
        lines.push(format!("DTEND:{}", ics_time(end)));
        lines.push(format!("SUMMARY:{}", escape(&format!("Sitzung {} ({})", c.number, who))));
        if c.session.modality.as_deref() == Some("video") {
            lines.push("LOCATION:Video".into());
        }
        if c.session.status == "cancelled" {
            lines.push("STATUS:CANCELLED".into());
        }
        lines.push("END:VEVENT".into());
    }
    lines.push("END:VCALENDAR".into());
    Ok(lines.iter().map(|l| fold(l)).collect::<Vec<_>>().join("\r\n") + "\r\n")
}

fn parse(s: &str) -> Result<OffsetDateTime> {
    OffsetDateTime::parse(s, &Rfc3339).map_err(|_| Error::Invalid(format!("Zeitpunkt ungültig: {s}")))
}

fn ics_time(t: OffsetDateTime) -> String {
    let t = t.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    )
}

/// RFC 5545 §3.3.11 text escaping.
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace('\n', "\\n")
}

/// RFC 5545 §3.1: lines longer than 75 octets continue on the next line after CRLF + space,
/// never inside a UTF-8 character.
fn fold(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 8);
    let mut width = 0;
    for ch in line.chars() {
        let n = ch.len_utf8();
        if width + n > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(ch);
        width += n;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Session;

    fn cal(id: &str, at: &str, chiffre: Option<&str>, status: &str, minutes: Option<i64>) -> CalendarSession {
        CalendarSession {
            session: Session {
                id: id.into(),
                patient_id: "p".into(),
                episode_id: None,
                scheduled_at: Some(at.into()),
                started_at: None,
                ended_at: None,
                duration_s: None,
                modality: Some("praesenz".into()),
                status: status.into(),
                planned_minutes: minutes,
                cancelled_reason: Some("krank".into()),
                note_id: None,
                note_status: None,
                note_export_outdated: false,
            },
            patient_name: "Anna Beispiel".into(),
            chiffre: chiffre.map(Into::into),
            number: 3,
            has_prep: false,
            sources_finished: 0,
            sources_unreviewed: 0,
            recording: false,
        }
    }

    #[test]
    fn exports_chiffre_and_times_but_never_the_name() {
        let out = ics(
            &[
                cal("s1", "2026-10-28T08:00:00Z", Some("B140385"), "planned", None),
                cal("s2", "2026-10-29T09:30:00Z", None, "cancelled", Some(25)),
            ],
            "2026-10-28T12:00:00Z",
        )
        .unwrap();
        assert!(out.starts_with("BEGIN:VCALENDAR\r\n") && out.ends_with("END:VCALENDAR\r\n"));
        assert!(out.contains("DTSTART:20261028T080000Z\r\nDTEND:20261028T085000Z"));
        assert!(out.contains("SUMMARY:Sitzung 3 (B140385)"));
        assert!(out.contains("DTEND:20261029T095500Z"));
        assert!(out.contains("SUMMARY:Sitzung 3 (ohne Chiffre)\r\nSTATUS:CANCELLED"));
        assert!(!out.contains("Anna") && !out.contains("Beispiel") && !out.contains("krank"));
        assert_eq!(out.matches("BEGIN:VEVENT").count(), 2);
    }

    #[test]
    fn folds_long_lines_on_char_boundaries() {
        let line = format!("SUMMARY:{}", "ä".repeat(60));
        let folded = fold(&line);
        assert!(folded.split("\r\n").all(|l| l.len() <= 75));
        assert_eq!(folded.replace("\r\n ", ""), line);
    }
}
