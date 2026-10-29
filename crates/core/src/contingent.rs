//! Session contingents of the Psychotherapie-Richtlinie (§§ 28–30, Einzeltherapie) and when the
//! next application is due (slice 31).
//!
//! Sessions count from `episode.therapy_since` (the first session after the Probatorik). KZT
//! sessions count towards a later LZT, so the count never restarts within a Behandlungsfall.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::{AgeGroup, ApplicationKind, Method, Stage};

/// KZT 1 and KZT 2 (cumulative).
pub const KZT1: u32 = 12;
pub const KZT2: u32 = 24;
/// "Antrag vorbereiten" shows this many sessions before the approved limit.
pub const LEAD: u32 = 3;
/// Most probatorische Sitzungen: adults 4, KJP 6.
pub const PROBATORIK_ADULT: u32 = 4;
pub const PROBATORIK_KJP: u32 = 6;
/// KJP patients from this age are "Jugendliche" (larger AP/TP contingents).
pub const YOUTH_AGE: u32 = 14;

/// LZT approval steps: the first approval, then the Höchstgrenze via Fortführungsantrag.
pub fn lzt_steps(method: Method, age_group: AgeGroup, youth: bool) -> [u32; 2] {
    match (method, age_group, youth) {
        (Method::Vt, _, _) => [60, 80],
        (Method::Tp, AgeGroup::Adult, _) => [60, 100],
        (Method::Ap, AgeGroup::Adult, _) => [160, 300],
        (Method::Tp | Method::Ap, AgeGroup::Kjp, false) => [70, 150],
        (Method::Tp | Method::Ap, AgeGroup::Kjp, true) => [90, 180],
    }
}

/// Where a Behandlungsfall's contingent stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Contingent {
    pub episode_id: String,
    pub stage: Stage,
    /// Held sessions counted: in the Probatorik all of them, later those since `therapy_since`.
    pub used: u32,
    /// The approved limit of the current stage (Probatorik: the most probatorische Sitzungen).
    pub limit: u32,
    /// The Höchstgrenze of the method (LZT), else the same as `limit`.
    pub max: u32,
    /// The application to prepare now (`used` is within [`LEAD`] of `limit`), if any.
    pub due: Option<ApplicationKind>,
}

/// What the count needs to know about the case besides the stage.
pub struct CaseFacts {
    pub method: Method,
    pub age_group: AgeGroup,
    /// KJP and at least [`YOUTH_AGE`] (unknown birth date: no, the smaller contingent warns earlier).
    pub youth: bool,
    /// Held sessions counted (see [`Contingent::used`]).
    pub used: u32,
    /// Umwandlungsanträge already created in this case.
    pub umwandlungen: u32,
    /// Fortführungsanträge already created in this case.
    pub fortfuehrungen: u32,
}

pub fn compute(episode_id: &str, stage: Stage, f: &CaseFacts) -> Contingent {
    let (limit, max, next) = match stage {
        Stage::Probatorik => {
            let n = if f.age_group == AgeGroup::Kjp { PROBATORIK_KJP } else { PROBATORIK_ADULT };
            (n, n, None)
        }
        // KZT → LZT needs an Umwandlungsantrag with a Bericht; KZT 2 needs none, so the app
        // suggests the Umwandlung and the therapist sets KZT 2 when she goes that way instead.
        Stage::Kzt1 => (KZT1, KZT2, (f.umwandlungen == 0).then_some(ApplicationKind::Umwandlung)),
        Stage::Kzt2 => (KZT2, KZT2, (f.umwandlungen == 0).then_some(ApplicationKind::Umwandlung)),
        Stage::Lzt => {
            let [first, max] = lzt_steps(f.method, f.age_group, f.youth);
            if f.fortfuehrungen == 0 {
                (first, max, Some(ApplicationKind::Fortfuehrung))
            } else {
                (max, max, None)
            }
        }
    };
    let due = next.filter(|_| f.used + LEAD >= limit);
    Contingent { episode_id: episode_id.to_string(), stage, used: f.used, limit, max, due }
}

/// Whole years between a `YYYY-MM-DD` birth date and the date part of `now`.
pub fn age(birth_date: &str, now: &str) -> Option<u32> {
    let parse = |s: &str| -> Option<(i32, u32, u32)> {
        let mut it = s.get(..10)?.split('-');
        Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?))
    };
    let (by, bm, bd) = parse(birth_date)?;
    let (ny, nm, nd) = parse(now)?;
    let years = ny - by - i32::from((nm, nd) < (bm, bd));
    u32::try_from(years).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(method: Method, age_group: AgeGroup, used: u32) -> CaseFacts {
        CaseFacts { method, age_group, youth: false, used, umwandlungen: 0, fortfuehrungen: 0 }
    }

    #[test]
    fn kzt_suggests_the_umwandlung_three_sessions_before_the_limit() {
        let c = compute("e", Stage::Kzt1, &facts(Method::Vt, AgeGroup::Adult, 8));
        assert_eq!((c.limit, c.max, c.due), (12, 24, None));
        let c = compute("e", Stage::Kzt1, &facts(Method::Vt, AgeGroup::Adult, 9));
        assert_eq!(c.due, Some(ApplicationKind::Umwandlung));
        // Once the Umwandlungsantrag exists, nothing more is due.
        let f = CaseFacts { umwandlungen: 1, ..facts(Method::Vt, AgeGroup::Adult, 11) };
        assert_eq!(compute("e", Stage::Kzt1, &f).due, None);
        // KZT 2 moves the limit to 24.
        let c = compute("e", Stage::Kzt2, &facts(Method::Tp, AgeGroup::Adult, 13));
        assert_eq!((c.limit, c.due), (24, None));
        assert_eq!(compute("e", Stage::Kzt2, &facts(Method::Tp, AgeGroup::Adult, 21)).due, Some(ApplicationKind::Umwandlung));
    }

    #[test]
    fn lzt_steps_follow_method_and_age_and_end_at_the_maximum() {
        assert_eq!(lzt_steps(Method::Vt, AgeGroup::Adult, false), [60, 80]);
        assert_eq!(lzt_steps(Method::Tp, AgeGroup::Adult, false), [60, 100]);
        assert_eq!(lzt_steps(Method::Ap, AgeGroup::Adult, false), [160, 300]);
        assert_eq!(lzt_steps(Method::Ap, AgeGroup::Kjp, false), [70, 150]);
        assert_eq!(lzt_steps(Method::Tp, AgeGroup::Kjp, true), [90, 180]);
        assert_eq!(lzt_steps(Method::Vt, AgeGroup::Kjp, true), [60, 80]);
        let c = compute("e", Stage::Lzt, &facts(Method::Tp, AgeGroup::Adult, 57));
        assert_eq!((c.limit, c.max, c.due), (60, 100, Some(ApplicationKind::Fortfuehrung)));
        let f = CaseFacts { fortfuehrungen: 1, ..facts(Method::Tp, AgeGroup::Adult, 99) };
        let c = compute("e", Stage::Lzt, &f);
        assert_eq!((c.limit, c.due), (100, None));
    }

    #[test]
    fn probatorik_has_no_application_due() {
        let c = compute("e", Stage::Probatorik, &facts(Method::Vt, AgeGroup::Kjp, 5));
        assert_eq!((c.limit, c.due), (6, None));
    }

    #[test]
    fn age_counts_whole_years() {
        assert_eq!(age("2012-10-30", "2026-10-29T10:00:00Z"), Some(13));
        assert_eq!(age("2012-10-29", "2026-10-29T10:00:00Z"), Some(14));
        assert_eq!(age("kaputt", "2026-10-29"), None);
    }
}
