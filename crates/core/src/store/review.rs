//! Bericht-Lotse "Prüfen" (slice 40): the Vorbericht a Bericht builds on, and the last AI
//! Gutachter-Check with the hints she ignored.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

use super::{new_id, parse_col, Store, ACTOR};
use crate::audit::{self, Entry};
use crate::domain::*;
use crate::review::{Hint, HintLevel};
use crate::{Error, Result};

/// One hint of the Gutachter-Check, as step 4 shows it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReviewHint {
    pub id: String,
    /// `None`: the whole Bericht.
    pub section_key: Option<String>,
    pub level: HintLevel,
    pub text: String,
    pub ignored: bool,
}

/// The last Gutachter-Check of a Bericht.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReportReview {
    pub model: String,
    pub checked_at: String,
    /// The text it read, to tell when she changed the Bericht since.
    pub text_digest: String,
    pub hints: Vec<ReviewHint>,
}

/// Provenance of one check.
pub struct ReviewMeta<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub prompt_hash: &'a str,
    pub text_digest: &'a str,
    /// Sections it read.
    pub sections: usize,
    pub dropped: usize,
}

impl Store {
    /// The Vorbericht of a Bericht: the last signed Bericht of the same Behandlungsfall to an
    /// earlier application.
    pub fn prior_report(&self, report_id: &str) -> Result<Option<Report>> {
        let report = self.report(report_id)?;
        let (Some(episode_id), Some(application_id)) = (&report.episode_id, &report.application_id) else { return Ok(None) };
        let id: Option<String> = self
            .conn
            .query_row(
                "SELECT r.id FROM report r JOIN application a ON a.id = r.application_id
                 WHERE r.episode_id = ?1 AND r.kind = 'bericht' AND r.status IN ('signed', 'exported') AND r.id != ?2
                   AND a.created_at < (SELECT created_at FROM application WHERE id = ?3)
                 ORDER BY r.signed_at DESC, r.rowid DESC LIMIT 1",
                params![episode_id, report_id, application_id],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|id| self.report(&id)).transpose()
    }

    pub fn report_review(&self, report_id: &str) -> Result<Option<ReportReview>> {
        let head = self
            .conn
            .query_row("SELECT model, checked_at, text_digest FROM report_review WHERE report_id = ?1", [report_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })
            .optional()?;
        let Some((model, checked_at, text_digest)) = head else { return Ok(None) };
        let mut stmt = self
            .conn
            .prepare("SELECT id, section_key, level, text, ignored FROM report_review_hint WHERE report_id = ?1 ORDER BY seq")?;
        let hints = stmt
            .query_map([report_id], |r| {
                Ok(ReviewHint { id: r.get(0)?, section_key: r.get(1)?, level: parse_col(r, 2)?, text: r.get(3)?, ignored: r.get(4)? })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(Some(ReportReview { model, checked_at, text_digest, hints }))
    }

    /// Replaces the Bericht's Gutachter-Check. The text itself is not touched.
    pub fn save_review(&self, report_id: &str, meta: &ReviewMeta, hints: &[Hint]) -> Result<ReportReview> {
        let report = self.report(report_id)?;
        if report.kind != "bericht" {
            return Err(Error::Invalid("Den Gutachter-Check gibt es nur für Berichte".into()));
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM report_review WHERE report_id = ?1", [report_id])?;
        tx.execute(
            "INSERT INTO report_review (report_id, provider, model, prompt_hash, text_digest, checked_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![report_id, meta.provider, meta.model, meta.prompt_hash, meta.text_digest, now()],
        )?;
        for (seq, h) in hints.iter().enumerate() {
            tx.execute(
                "INSERT INTO report_review_hint (id, report_id, seq, section_key, level, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![new_id(), report_id, seq as i64, h.section_key, h.level.as_str(), h.text],
            )?;
        }
        audit::append(&tx, Entry {
            actor: ACTOR, action: "report.review", entity_type: "report", entity_id: Some(report_id),
            meta: json!({
                "provider": meta.provider, "model": meta.model,
                "prompt_version": crate::review::PROMPT_VERSION, "prompt_hash": meta.prompt_hash,
                "sections": meta.sections, "hints": hints.len(),
                "important": hints.iter().filter(|h| h.level == HintLevel::Important).count(), "dropped": meta.dropped,
            }),
        })?;
        tx.commit()?;
        self.report_review(report_id)?.ok_or_else(|| Error::NotFound("review".into()))
    }

    /// "Ignorieren" on a hint, or back.
    pub fn set_review_hint_ignored(&self, report_id: &str, hint_id: &str, ignored: bool) -> Result<ReportReview> {
        let n = self.conn.execute(
            "UPDATE report_review_hint SET ignored = ?3 WHERE id = ?1 AND report_id = ?2",
            params![hint_id, report_id, ignored],
        )?;
        if n == 0 {
            return Err(Error::NotFound(format!("Hinweis {hint_id}")));
        }
        self.report_review(report_id)?.ok_or_else(|| Error::NotFound("review".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draft::{DraftSentence, Parsed, SectionResult};

    fn bericht(store: &Store) -> (Patient, Episode, Application) {
        let p = store
            .create_patient(PatientInput { first_name: "Anna".into(), last_name: "Beispiel".into(), chiffre: Some("B140385".into()), ..Default::default() })
            .unwrap();
        let ep = store
            .create_episode(EpisodeInput { patient_id: p.id.clone(), method: Method::Vt, age_group: AgeGroup::Adult, setting: Setting::Einzel })
            .unwrap();
        let app = store.create_application(&ep.id, ApplicationKind::Erstantrag).unwrap();
        (p, ep, app)
    }

    #[test]
    fn review_replaces_and_hints_can_be_ignored() {
        let store = Store::open_in_memory().unwrap();
        let (_, _, app) = bericht(&store);
        let report_id = app.report_id.unwrap();
        assert!(store.report_review(&report_id).unwrap().is_none());
        let meta = ReviewMeta { provider: "local", model: "m", prompt_hash: "ph", text_digest: "d1", sections: 3, dropped: 0 };
        let hints = vec![
            Hint { section_key: Some("behandlungsplan_prognose".into()), level: HintLevel::Important, text: "Prognose begründen?".into() },
            Hint { section_key: None, level: HintLevel::Note, text: "Bezug Modell–Plan.".into() },
        ];
        let r = store.save_review(&report_id, &meta, &hints).unwrap();
        assert_eq!((r.text_digest.as_str(), r.hints.len()), ("d1", 2));
        assert_eq!(r.hints[0].section_key.as_deref(), Some("behandlungsplan_prognose"));
        assert_eq!((r.hints[1].level, r.hints[1].section_key.clone()), (HintLevel::Note, None));
        let r = store.set_review_hint_ignored(&report_id, &r.hints[1].id, true).unwrap();
        assert!(!r.hints[0].ignored && r.hints[1].ignored);
        assert!(store.set_review_hint_ignored(&report_id, "nope", true).is_err());
        // A new check replaces the old one, ignored hints included.
        let r = store.save_review(&report_id, &ReviewMeta { text_digest: "d2", ..meta }, &hints[..1]).unwrap();
        assert_eq!((r.text_digest.as_str(), r.hints.len(), r.hints[0].ignored), ("d2", 1, false));
        let left: i64 = store.conn().query_row("SELECT COUNT(*) FROM report_review_hint", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 1);
    }

    #[test]
    fn the_vorbericht_is_the_last_signed_bericht_and_drafts_cite_it() {
        let store = Store::open_in_memory().unwrap();
        let (p, ep, first) = bericht(&store);
        let first_id = first.report_id.clone().unwrap();
        assert!(store.prior_report(&first_id).unwrap().is_none(), "nothing before the Erstantrag");
        store.save_section(&first_id, "behandlungsplan_prognose", "<p>Ziel: allein Bus fahren.</p>").unwrap();
        store.save_section(&first_id, "symptomatik_befund", "<p>Panik im Bus.</p>").unwrap();
        // The next application is created later than the first.
        store.conn().execute("UPDATE application SET created_at = '2026-01-01T00:00:00Z' WHERE id = ?1", [&first.id]).unwrap();
        let next = store.create_application(&ep.id, ApplicationKind::Umwandlung).unwrap();
        let next_id = next.report_id.clone().unwrap();
        assert!(store.prior_report(&next_id).unwrap().is_none(), "an unsigned Bericht is no Vorbericht");
        store.sign_report(&first_id, "M. Berger").unwrap();
        let prior = store.prior_report(&next_id).unwrap().unwrap();
        assert_eq!(prior.id, first_id);
        assert!(store.prior_report(&first_id).unwrap().is_none(), "never a later one");

        // Index 0 is her answer, 1 the Vorbericht section.
        let a = store.set_report_answer(&next_id, "umwandlung", "verlauf", "Verlauf?", "besser", AnswerSource::Typed).unwrap().unwrap();
        let priors = vec![PriorRef { report_id: first_id.clone(), section_key: "behandlungsplan_prognose".into() }];
        let meta = DraftMeta { provider: "local".into(), model: "m".into(), prompt_hash: "h".into(), instructed: false, session_ids: None, source: DraftSource::Facts };
        let run = vec![SectionResult {
            key: "umwandlung".into(),
            parsed: Parsed { sentences: vec![DraftSentence { text: "Pat. fährt wieder Bus.".into(), facts: vec![0, 1] }], dropped: 0 },
            with_existing: false,
        }];
        let d = store.save_drafts(&next_id, &meta, &run, &[], &[a], &priors).unwrap();
        let s = &d[0].sentences[0];
        assert_eq!((s.answers.len(), s.prior.len()), (1, 1));
        assert_eq!((s.prior[0].excerpt.as_str(), s.prior[0].signed_at.is_some()), ("Ziel: allein Bus fahren.", true));
        let bad = vec![SectionResult { key: "umwandlung".into(), parsed: Parsed { sentences: vec![DraftSentence { text: "x".into(), facts: vec![2] }], dropped: 0 }, with_existing: false }];
        assert!(store.save_drafts(&next_id, &meta, &bad, &[], &[], &priors).is_err(), "unknown index");
        store.erase_patient(&p.id).unwrap();
        let left: i64 = store.conn().query_row("SELECT COUNT(*) FROM prior_citation", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
    }
}
