//! Stage, contingent count and "Antrag vorbereiten" per Behandlungsfall (slice 31).

use rusqlite::{params, OptionalExtension};
use serde_json::json;

use super::{Store, ACTOR};
use crate::audit::{self, Entry};
use crate::contingent::{self, CaseFacts, Contingent};
use crate::domain::*;
use crate::{Error, Result};

impl Store {
    /// Sets the stage the therapist has approved. Leaving the Probatorik sets the start of the
    /// Richtlinientherapie (`YYYY-MM-DD`, default: kept or today); going back clears it.
    pub fn set_episode_stage(&self, episode_id: &str, stage: Stage, therapy_since: Option<&str>) -> Result<Episode> {
        let episode = self.episode(episode_id)?;
        let since = match stage {
            Stage::Probatorik => None,
            _ => Some(match therapy_since.map(str::trim).filter(|s| !s.is_empty()) {
                Some(d) => {
                    if !is_date(d) {
                        return Err(Error::Invalid(format!("Datum ungültig: {d}")));
                    }
                    d.to_string()
                }
                None => episode.therapy_since.clone().unwrap_or_else(|| now()[..10].to_string()),
            }),
        };
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("UPDATE episode SET stage = ?2, therapy_since = ?3 WHERE id = ?1", params![episode_id, stage.as_str(), since])?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "episode.stage", entity_type: "episode", entity_id: Some(episode_id),
            meta: json!({ "stage": stage.as_str(), "therapy_since": since }),
        })?;
        tx.commit()?;
        self.episode(episode_id)
    }

    /// Held sessions of the case up to `now` (later ones once they have a note or recording),
    /// from `since` on when given. Cancelled sessions don't count.
    pub(crate) fn held_sessions(&self, episode_id: &str, now: &str, since: Option<&str>) -> Result<u32> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM session s
             LEFT JOIN report n ON n.session_id = s.id AND n.kind = 'sitzungsnotiz'
             WHERE s.episode_id = ?1 AND s.status != 'cancelled' AND COALESCE(s.scheduled_at, s.started_at) IS NOT NULL
               AND (COALESCE(s.scheduled_at, s.started_at) <= ?2 OR n.id IS NOT NULL OR s.status IN ('in_progress', 'recorded'))
               AND (?3 IS NULL OR COALESCE(s.scheduled_at, s.started_at) >= ?3)",
            params![episode_id, now, since],
            |r| r.get(0),
        )?)
    }

    pub fn episode_contingent(&self, episode_id: &str, now: &str) -> Result<Contingent> {
        let (ep, facts) = self.case_facts(episode_id, now)?;
        Ok(contingent::compute(episode_id, ep.stage, &facts))
    }

    /// For an open Bericht: sessions left before the application has to be filed, and the
    /// planned session that uses up the last of them. `None` once the case left the stage the
    /// application leads out of (the Erstantrag ends the Probatorik, the Umwandlung KZT, the
    /// Fortführung the first LZT step).
    pub(crate) fn application_deadline(
        &self,
        episode_id: &str,
        kind: ApplicationKind,
        now: &str,
    ) -> Result<Option<(i64, Option<String>)>> {
        let (ep, facts) = self.case_facts(episode_id, now)?;
        let limit = match (kind, ep.stage) {
            (ApplicationKind::Erstantrag, Stage::Probatorik)
            | (ApplicationKind::Umwandlung, Stage::Kzt1 | Stage::Kzt2) => contingent::compute(episode_id, ep.stage, &facts).limit,
            (ApplicationKind::Fortfuehrung, Stage::Lzt) => contingent::lzt_steps(ep.method, ep.age_group, facts.youth)[0],
            _ => return Ok(None),
        };
        let left = i64::from(limit) - i64::from(facts.used);
        let due_by = if left > 0 {
            self.conn
                .query_row(
                    "SELECT s.scheduled_at FROM session s
                     WHERE s.episode_id = ?1 AND s.status = 'planned' AND s.scheduled_at > ?2
                       AND NOT EXISTS (SELECT 1 FROM report n WHERE n.session_id = s.id)
                     ORDER BY s.scheduled_at LIMIT 1 OFFSET ?3",
                    params![episode_id, now, left - 1],
                    |r| r.get(0),
                )
                .optional()?
        } else {
            None
        };
        Ok(Some((left, due_by)))
    }

    fn case_facts(&self, episode_id: &str, now: &str) -> Result<(Episode, CaseFacts)> {
        let ep = self.episode(episode_id)?;
        let patient = self.patient(&ep.patient_id)?;
        let since = if ep.stage == Stage::Probatorik { None } else { ep.therapy_since.as_deref() };
        let count = |kind: &str| -> Result<u32> {
            Ok(self.conn.query_row(
                "SELECT COUNT(*) FROM application WHERE episode_id = ?1 AND kind = ?2",
                params![episode_id, kind],
                |r| r.get(0),
            )?)
        };
        let youth = ep.age_group == AgeGroup::Kjp
            && patient.birth_date.as_deref().and_then(|b| contingent::age(b, now)).is_some_and(|a| a >= contingent::YOUTH_AGE);
        let facts = CaseFacts {
            method: ep.method,
            age_group: ep.age_group,
            youth,
            used: self.held_sessions(episode_id, now, since)?,
            umwandlungen: count("umwandlung")?,
            fortfuehrungen: count("fortfuehrung")?,
        };
        Ok((ep, facts))
    }

    /// "Antrag vorbereiten" for every open case whose contingent is nearly used up.
    pub(crate) fn applications_due(&self, now: &str) -> Result<Vec<DueItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT e.id, p.id, p.first_name || ' ' || p.last_name, p.chiffre FROM episode e JOIN patient p ON p.id = e.patient_id
             WHERE e.ended_at IS NULL AND e.stage != 'probatorik' ORDER BY p.last_name, p.first_name",
        )?;
        let rows: Vec<(String, String, String, Option<String>)> =
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<std::result::Result<_, _>>()?;
        let mut items = Vec::new();
        for (episode_id, patient_id, patient_name, chiffre) in rows {
            let c = self.episode_contingent(&episode_id, now)?;
            let Some(kind) = c.due else { continue };
            items.push(DueItem {
                kind: DueKind::ApplicationDue,
                patient_id,
                patient_name,
                chiffre,
                session_id: None,
                report_id: None,
                at: None,
                application_kind: Some(kind),
                seq: None,
                contingent: Some(c),
            });
        }
        Ok(items)
    }
}

fn is_date(s: &str) -> bool {
    time::Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(store: &Store, method: Method) -> Episode {
        let p = store
            .create_patient(PatientInput { first_name: "Anna".into(), last_name: "Kurz".into(), chiffre: Some("K010190".into()), ..Default::default() })
            .unwrap();
        store.create_episode(EpisodeInput { patient_id: p.id, method, age_group: AgeGroup::Adult, setting: Setting::Einzel }).unwrap()
    }

    fn held(store: &Store, ep: &Episode, at: &str) -> Session {
        store
            .create_session(SessionInput {
                patient_id: ep.patient_id.clone(),
                episode_id: Some(ep.id.clone()),
                scheduled_at: Some(at.into()),
                modality: None,
                planned_minutes: None,
            })
            .unwrap()
    }

    #[test]
    fn sessions_count_from_therapy_start_and_an_application_falls_due() {
        let store = Store::open_in_memory().unwrap();
        let ep = case(&store, Method::Vt);
        let now = "2026-06-01T12:00:00Z";
        // Two probatorische Sitzungen, then ten in KZT 1, one cancelled, one in the future.
        for d in ["2026-01-05", "2026-01-12"] {
            held(&store, &ep, &format!("{d}T09:00:00Z"));
        }
        for i in 0..10 {
            held(&store, &ep, &format!("2026-02-{:02}T09:00:00Z", i + 1));
        }
        let cancelled = held(&store, &ep, "2026-03-01T09:00:00Z");
        store.cancel_session(&cancelled.id, None).unwrap();
        held(&store, &ep, "2026-07-01T09:00:00Z");

        let c = store.episode_contingent(&ep.id, now).unwrap();
        assert_eq!((c.stage, c.used, c.due), (Stage::Probatorik, 12, None));
        assert!(store.applications_due(now).unwrap().is_empty());

        let ep = store.set_episode_stage(&ep.id, Stage::Kzt1, Some("2026-02-01")).unwrap();
        assert_eq!(ep.therapy_since.as_deref(), Some("2026-02-01"));
        let c = store.episode_contingent(&ep.id, now).unwrap();
        assert_eq!((c.used, c.limit, c.due), (10, 12, Some(ApplicationKind::Umwandlung)));
        let due = store.applications_due(now).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!((due[0].kind, due[0].application_kind), (DueKind::ApplicationDue, Some(ApplicationKind::Umwandlung)));

        let app = store.create_application(&ep.id, ApplicationKind::Umwandlung).unwrap();
        assert!(store.applications_due(now).unwrap().is_empty());

        // The Berichte board: 2 sessions left, the second planned one uses up the last.
        held(&store, &ep, "2026-07-08T09:00:00Z");
        let report_id = app.report_id.clone().unwrap();
        let row = |store: &Store| store.list_reports(None, now).unwrap().into_iter().find(|r| r.id == report_id).unwrap();
        let r = row(&store);
        assert_eq!((r.sessions_left, r.due_by.as_deref()), (Some(2), Some("2026-07-08T09:00:00Z")));
        assert!(r.sections_total > 0);
        assert_eq!(r.sections_filled, 0);
        let first = store.report(&report_id).unwrap().sections[0].key.clone();
        store.save_section(&report_id, &first, "<p> </p>").unwrap();
        assert_eq!(row(&store).sections_filled, 0, "an empty paragraph isn't text");
        store.save_section(&report_id, &first, "<p>Anlass</p>").unwrap();
        assert_eq!(row(&store).sections_filled, 1);
        // Once the case is in LZT the Umwandlung has no deadline any more.
        store.set_episode_stage(&ep.id, Stage::Lzt, None).unwrap();
        assert_eq!((row(&store).sessions_left, row(&store).due_by), (None, None));
        let ep = store.set_episode_stage(&ep.id, Stage::Kzt1, None).unwrap();

        // Back to the Probatorik clears the start; a bad date is refused.
        assert!(store.set_episode_stage(&ep.id, Stage::Lzt, Some("01.02.2026")).is_err());
        let ep = store.set_episode_stage(&ep.id, Stage::Probatorik, None).unwrap();
        assert_eq!(ep.therapy_since, None);
        let last = audit::list(store.conn(), 1).unwrap().remove(0);
        assert_eq!(last.action, "episode.stage");
    }
}
