//! The patient's course over time (slice 30): held and cancelled sessions with their key
//! points, applications with their Bericht, consents, grouped by Behandlungsfall.

use serde::Serialize;
use ts_rs::TS;

use super::summary::SessionSummary;
use super::Store;
use crate::domain::*;
use crate::Result;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum TimelineItem {
    Session {
        at: String,
        session: Session,
        /// Position among the case's held (not cancelled) sessions, 1-based; `None` when cancelled.
        /// Once the therapy started (`episode.therapy_since`), sessions from then on are numbered
        /// in the contingent and the ones before as Probatorik.
        number: Option<usize>,
        probatorik: bool,
        summary: Option<SessionSummary>,
    },
    Application {
        at: String,
        application: Application,
        report_status: Option<ReportStatus>,
        signed_at: Option<String>,
        exported_at: Option<String>,
    },
    Consent {
        at: String,
        kind: ConsentKind,
        /// `false`: the consent was revoked at `at`.
        granted: bool,
    },
}

impl TimelineItem {
    pub(crate) fn at(&self) -> &str {
        match self {
            Self::Session { at, .. } | Self::Application { at, .. } | Self::Consent { at, .. } => at,
        }
    }
}

/// One Behandlungsfall (or, `episode: None`, what belongs to none), items newest first.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TimelineGroup {
    pub episode: Option<Episode>,
    /// Where the case's contingent stands (slice 31).
    pub contingent: Option<crate::contingent::Contingent>,
    /// Held sessions in this group.
    pub sessions: usize,
    pub items: Vec<TimelineItem>,
}

impl Store {
    /// Sessions up to `now` (later ones only once they have a recording or note); newest case first.
    pub fn patient_timeline(&self, patient_id: &str, now: &str) -> Result<Vec<TimelineGroup>> {
        self.patient(patient_id)?;
        let mut episodes = self.list_episodes(patient_id)?;
        episodes.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        let mut groups: Vec<TimelineGroup> = episodes
            .iter()
            .map(|e| Ok(TimelineGroup { episode: Some(e.clone()), contingent: Some(self.episode_contingent(&e.id, now)?), sessions: 0, items: Vec::new() }))
            .collect::<Result<_>>()?;
        groups.push(TimelineGroup { episode: None, contingent: None, sessions: 0, items: Vec::new() });
        let none = groups.len() - 1;
        let group_of = |episode_id: Option<&str>| episode_id.and_then(|id| episodes.iter().position(|e| e.id == id)).unwrap_or(none);
        // A consent belongs to the case running at the time (the newest one started before it).
        let case_at = |at: &str| {
            episodes
                .iter()
                .position(|e| e.started_at.as_str() <= at && e.ended_at.as_deref().is_none_or(|end| at <= end))
                .unwrap_or(none)
        };

        let mut sessions: Vec<(String, Session)> = self
            .list_sessions(Some(patient_id))?
            .into_iter()
            .filter_map(|s| {
                let at = s.scheduled_at.clone().or(s.started_at.clone())?;
                let held = at.as_str() <= now || s.note_id.is_some() || matches!(s.status.as_str(), "in_progress" | "recorded");
                held.then_some((at, s))
            })
            .collect();
        sessions.sort_by(|a, b| a.0.cmp(&b.0));
        // Counted per group: (all held, Probatorik, in the contingent).
        let mut counts = vec![(0usize, 0usize, 0usize); groups.len()];
        for (at, s) in sessions {
            let g = group_of(s.episode_id.as_deref());
            let since = groups[g].episode.as_ref().filter(|e| e.stage != Stage::Probatorik).and_then(|e| e.therapy_since.clone());
            let probatorik = since.as_deref().is_some_and(|d| at.as_str() < d);
            let number = (s.status != "cancelled").then(|| {
                let c = &mut counts[g];
                c.0 += 1;
                match (&since, probatorik) {
                    (None, _) => c.0,
                    (Some(_), true) => {
                        c.1 += 1;
                        c.1
                    }
                    (Some(_), false) => {
                        c.2 += 1;
                        c.2
                    }
                }
            });
            let summary = if number.is_some() { self.session_summary(&s.id)? } else { None };
            groups[g].items.push(TimelineItem::Session { at, session: s, number, probatorik, summary });
        }
        for (g, c) in counts.into_iter().enumerate() {
            groups[g].sessions = c.0;
        }

        for (g, e) in episodes.iter().enumerate() {
            for a in self.list_applications(&e.id)? {
                let report = a.report_id.as_deref().map(|id| self.report(id)).transpose()?;
                groups[g].items.push(TimelineItem::Application {
                    at: a.created_at.clone(),
                    report_status: report.as_ref().map(|r| r.status),
                    signed_at: report.as_ref().and_then(|r| r.signed_at.clone()),
                    exported_at: report.as_ref().and_then(|r| r.exported_at.clone()),
                    application: a,
                });
            }
        }

        for c in self.list_consents(patient_id)? {
            groups[case_at(&c.granted_at)].items.push(TimelineItem::Consent { at: c.granted_at.clone(), kind: c.kind, granted: true });
            if let Some(at) = c.revoked_at {
                groups[case_at(&at)].items.push(TimelineItem::Consent { at, kind: c.kind, granted: false });
            }
        }

        for g in &mut groups {
            g.items.sort_by(|a, b| b.at().cmp(a.at()));
        }
        if groups[none].items.is_empty() {
            groups.pop();
        }
        Ok(groups)
    }
}
