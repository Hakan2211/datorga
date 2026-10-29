//! Bericht-Lotse (Phase 5): the guided way to a Bericht. Which Antragsart fits a case now.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::contingent::Contingent;
use crate::domain::{ApplicationKind, Method, Stage};

/// An open Behandlungsfall in "Neuer Bericht".
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BerichtCandidate {
    pub episode_id: String,
    pub patient_id: String,
    pub first_name: String,
    pub last_name: String,
    pub chiffre: Option<String>,
    pub has_photo: bool,
    pub method: Method,
    pub contingent: Contingent,
    /// The Antragsart the Lotse suggests ([`suggest_kind`]).
    pub suggested: ApplicationKind,
    /// An open (unsigned) Bericht of the suggested kind: resume it instead of starting another.
    pub open_report_id: Option<String>,
}

/// The application due now, else the one that leads out of the current stage: the Probatorik
/// into therapy (Erstantrag), KZT into LZT (Umwandlung), the first LZT step to the maximum
/// (Fortführung).
pub fn suggest_kind(contingent: &Contingent) -> ApplicationKind {
    contingent.due.unwrap_or(match contingent.stage {
        Stage::Probatorik => ApplicationKind::Erstantrag,
        Stage::Kzt1 | Stage::Kzt2 => ApplicationKind::Umwandlung,
        Stage::Lzt => ApplicationKind::Fortfuehrung,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(stage: Stage, due: Option<ApplicationKind>) -> Contingent {
        Contingent { episode_id: "e".into(), stage, used: 0, limit: 12, max: 24, due }
    }

    #[test]
    fn follows_the_stage_unless_something_is_due() {
        assert_eq!(suggest_kind(&c(Stage::Probatorik, None)), ApplicationKind::Erstantrag);
        assert_eq!(suggest_kind(&c(Stage::Kzt1, None)), ApplicationKind::Umwandlung);
        assert_eq!(suggest_kind(&c(Stage::Kzt2, None)), ApplicationKind::Umwandlung);
        assert_eq!(suggest_kind(&c(Stage::Lzt, None)), ApplicationKind::Fortfuehrung);
        assert_eq!(suggest_kind(&c(Stage::Kzt1, Some(ApplicationKind::Umwandlung))), ApplicationKind::Umwandlung);
    }
}
