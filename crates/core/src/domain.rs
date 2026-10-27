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
str_enum!(
    /// What a consent covers. `cloud_processing` follows with the Phase 2 LLM adapter.
    ConsentKind { Recording => "recording" }
);
str_enum!(
    /// `session_audio` records the patient and needs consent; `dictation` is the therapist alone.
    SourceKind { SessionAudio => "session_audio", Dictation => "dictation" }
);
str_enum!(SourceStatus { Recording => "recording", Recorded => "recorded", Interrupted => "interrupted" });
str_enum!(
    /// A confirmed transcript is read-only; confirming deletes the audio.
    TranscriptStatus { Draft => "draft", Confirmed => "confirmed" }
);
str_enum!(
    /// An extracted fact: waiting for review, dismissed, or appended to the note.
    FactStatus { Proposed => "proposed", Rejected => "rejected", Applied => "applied" }
);
str_enum!(
    /// A Bericht section draft. `superseded`: applied once, then replaced by a later draft.
    DraftStatus { Proposed => "proposed", Applied => "applied", Rejected => "rejected", Superseded => "superseded" }
);
str_enum!(
    /// How a section draft goes into the section: instead of its text or after it.
    ApplyMode { Replace => "replace", Append => "append" }
);
str_enum!(JobStatus { Queued => "queued", Running => "running", Done => "done", Failed => "failed", Cancelled => "cancelled" });
str_enum!(
    /// Who speaks in a segment. Session recordings stay `unknown` until diarization exists.
    Speaker { Therapist => "therapist", Patient => "patient", Unknown => "unknown" }
);

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

/// One grant of consent. Revoking sets `revoked_at`; the row is never deleted (except on erasure).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Consent {
    pub id: String,
    pub patient_id: String,
    pub kind: ConsentKind,
    pub scope: Option<String>,
    pub text_version: String,
    pub text_hash: String,
    pub granted_at: String,
    pub revoked_at: Option<String>,
    pub note: Option<String>,
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
    /// The session's Sitzungsnotiz, if one was started.
    pub note_id: Option<String>,
    pub note_status: Option<ReportStatus>,
    /// The note got a Nachtrag after its last export: the exported file lacks it.
    pub note_export_outdated: bool,
}

/// One capture within a session, with its audio metadata.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionSource {
    pub id: String,
    pub session_id: String,
    pub kind: SourceKind,
    pub consent_id: Option<String>,
    pub status: SourceStatus,
    pub language: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub end_reason: Option<String>,
    /// JSON number; ms durations stay far below 2^53.
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    pub audio_deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Transcript {
    pub id: String,
    pub source_id: String,
    pub engine: String,
    pub model: String,
    pub language: String,
    pub status: TranscriptStatus,
    pub created_at: String,
    pub confirmed_at: Option<String>,
    pub segments: Vec<TranscriptSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TranscriptSegment {
    #[ts(type = "number")]
    pub idx: i64,
    #[ts(type = "number")]
    pub start_ms: i64,
    #[ts(type = "number")]
    pub end_ms: i64,
    pub text: String,
    pub edited: bool,
}

/// Background work (transcription) that survives an app restart.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub source_id: Option<String>,
    pub status: JobStatus,
    #[ts(type = "number")]
    pub progress: i64,
    #[ts(type = "number")]
    pub attempts: i64,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// What every downstream stage consumes, whatever the capture mode (PLAN §6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SourceSegment {
    /// `transcript_segment.id`: what a fact cites.
    pub segment_id: String,
    pub source_id: String,
    pub role: Speaker,
    #[ts(type = "number")]
    pub start_ms: i64,
    #[ts(type = "number")]
    pub end_ms: i64,
    pub text: String,
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
    /// Set for a Sitzungsnotiz.
    pub session_id: Option<String>,
    pub template_key: String,
    pub template_version: String,
    pub kind: String,
    pub status: ReportStatus,
    pub created_at: String,
    pub updated_at: String,
    pub signed_at: Option<String>,
    pub signed_by: Option<String>,
    pub exported_at: Option<String>,
    /// Exported, but a Nachtrag was added afterwards: export again so the file contains it.
    pub export_outdated: bool,
    pub sections: Vec<ReportSection>,
    /// Nachträge to a signed Sitzungsnotiz, oldest first.
    pub addenda: Vec<ReportAddendum>,
}

/// A later correction or addition to a signed note. Signed when written, never changed.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReportAddendum {
    pub id: String,
    #[ts(type = "number")]
    pub seq: i64,
    /// TipTap HTML, like a prose section.
    pub body: String,
    pub created_at: String,
    pub signed_by: String,
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

/// A term the therapist added to the pseudonymizer for one patient (redaction preview).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PseudoTerm {
    pub id: String,
    pub patient_id: String,
    pub kind: crate::pseudo::EntityKind,
    pub text: String,
    pub created_at: String,
}

/// A fact the model extracted from a session's transcript, with the segments it cites.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NoteFact {
    pub id: String,
    pub extraction_id: String,
    pub section_key: String,
    pub text: String,
    pub status: FactStatus,
    /// Text or section changed by the therapist before applying.
    pub edited: bool,
    pub applied_at: Option<String>,
    pub citations: Vec<FactCitation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FactCitation {
    pub segment_id: String,
    pub source_id: String,
    #[ts(type = "number")]
    pub start_ms: i64,
    pub text: String,
}

/// The therapist's version of a fact when applying it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FactEdit {
    pub id: String,
    pub section_key: String,
    pub text: String,
}

/// Provenance of one extraction run (PLAN Â§5: model, provider, prompt version and hash).
#[derive(Debug, Clone)]
pub struct ExtractionMeta {
    pub provider: String,
    pub model: String,
    pub prompt_hash: String,
    pub chunks: usize,
    pub dropped: usize,
}

/// An applied Sitzungsnotiz fact of the episode, as a source for Bericht drafts.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EpisodeFact {
    pub fact_id: String,
    pub session_id: String,
    pub note_id: String,
    /// Appointment or start time of the session.
    pub session_at: Option<String>,
    /// 1-based position of the session among the episode's sessions with facts.
    pub session_no: usize,
    /// Sitzungsnotiz section the fact was filed under.
    pub section_key: String,
    pub text: String,
}

/// A model draft for one Bericht section, sentence by sentence.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SectionDraft {
    pub id: String,
    pub section_key: String,
    pub status: DraftStatus,
    pub mode: Option<ApplyMode>,
    pub provider: String,
    pub model: String,
    pub instructed: bool,
    #[ts(type = "number")]
    pub dropped: i64,
    pub created_at: String,
    pub applied_at: Option<String>,
    pub sentences: Vec<DraftSentenceView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DraftSentenceView {
    pub id: String,
    pub text: String,
    pub sources: Vec<EpisodeFact>,
}

/// Provenance of one drafting run.
#[derive(Debug, Clone)]
pub struct DraftMeta {
    pub provider: String,
    pub model: String,
    pub prompt_hash: String,
    pub instructed: bool,
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("rfc3339 formatting cannot fail")
}

/// Row in the reports queue (dashboard, /reports). Name is shown locally only, never exported.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReportSummary {
    pub id: String,
    pub patient_id: String,
    pub patient_name: String,
    pub chiffre: Option<String>,
    pub method: Option<Method>,
    pub kind: Option<ApplicationKind>,
    pub seq: Option<i64>,
    pub status: ReportStatus,
    pub updated_at: String,
}
