//! Domain entities and value objects. Plain data; persistence lives in `store`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

macro_rules! str_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
        #[ts(export)]
        pub enum $name { $(#[serde(rename = $s)] $variant),+ }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $s),+ }
            }
        }

        impl std::str::FromStr for $name {
            type Err = crate::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($s => Ok(Self::$variant),)+
                    other => Err(crate::Error::Invalid(format!(
                        concat!("unknown ", stringify!($name), ": {}"), other))),
                }
            }
        }
    };
}

str_enum!(
    /// Richtlinienverfahren.
    Method { Vt => "VT", Tp => "TP", Ap => "AP" }
);
str_enum!(
    /// Erwachsene vs. Kinder und Jugendliche.
    AgeGroup { Adult => "adult", Kjp => "kjp" }
);
str_enum!(Setting { Einzel => "einzel", Gruppe => "gruppe", Kombi => "kombi" });
str_enum!(Stage { Probatorik => "probatorik", Kzt1 => "kzt1", Kzt2 => "kzt2", Lzt => "lzt" });
str_enum!(
    /// PTV 3 report kinds.
    ApplicationKind { Erstantrag => "erstantrag", Umwandlung => "umwandlung", Fortfuehrung => "fortfuehrung" }
);
str_enum!(ReportStatus { Draft => "draft", InReview => "in_review", Signed => "signed", Exported => "exported" });
str_enum!(SectionOrigin { Ai => "ai", Human => "human", Mixed => "mixed" });

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Patient {
    pub id: String,
    pub external_ref: Option<String>,
    pub first_name: String,
    pub last_name: String,
    pub birth_date: Option<String>,
    pub chiffre: Option<String>,
    pub pronouns: Option<String>,
    pub consent_recording: bool,
    pub status: String,
    pub created_at: String,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PatientInput {
    pub external_ref: Option<String>,
    pub first_name: String,
    pub last_name: String,
    pub birth_date: Option<String>,
    pub chiffre: Option<String>,
    pub pronouns: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Episode {
    pub id: String,
    pub patient_id: String,
    pub method: Method,
    pub age_group: AgeGroup,
    pub setting: Setting,
    pub stage: Stage,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EpisodeInput {
    pub patient_id: String,
    pub method: Method,
    pub age_group: AgeGroup,
    pub setting: Setting,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Application {
    pub id: String,
    pub episode_id: String,
    pub kind: ApplicationKind,
    pub seq: i64,
    pub status: String,
    pub report_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Session {
    pub id: String,
    pub patient_id: String,
    pub episode_id: Option<String>,
    pub scheduled_at: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub duration_s: Option<i64>,
    pub modality: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionInput {
    pub patient_id: String,
    pub episode_id: Option<String>,
    pub scheduled_at: Option<String>,
    pub modality: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Report {
    pub id: String,
    pub patient_id: String,
    pub episode_id: Option<String>,
    pub application_id: Option<String>,
    pub template_key: String,
    pub template_version: String,
    pub kind: String,
    pub status: ReportStatus,
    pub created_at: String,
    pub updated_at: String,
    pub signed_at: Option<String>,
    pub signed_by: Option<String>,
    pub sections: Vec<ReportSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReportSection {
    pub key: String,
    pub body: String,
    pub origin: SectionOrigin,
    pub edited_at: Option<String>,
}

/// ICD-10 diagnosis, entered by the therapist only (never AI-filled; MDR boundary).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Diagnosis {
    pub code: String,
    pub label: String,
    /// G (gesichert) | V (Verdacht) | Z (Zustand nach) | A (ausgeschlossen)
    pub certainty: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AuditEvent {
    pub seq: i64,
    pub at: String,
    pub actor: String,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub meta_json: String,
    pub prev_hash: String,
    pub hash: String,
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("rfc3339 formatting cannot fail")
}
