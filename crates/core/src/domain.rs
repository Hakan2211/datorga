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
str_enum!(
    /// Geschlecht as recorded by the therapist; `None` on the patient = keine Angabe.
    Gender { W => "w", M => "m", D => "d" }
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
    /// What a consent covers. `cloud_transcription` adds sending session recordings to a cloud
    /// speech service; without it they are transcribed only on this computer.
    ConsentKind { Recording => "recording", CloudTranscription => "cloud_transcription" }
);
str_enum!(
    /// `session_audio` records the patient and needs consent; `dictation` is the therapist alone.
    SourceKind { SessionAudio => "session_audio", Dictation => "dictation" }
);
str_enum!(
    /// Where a recording is transcribed: whisper.cpp on this computer or a cloud speech service.
    SttTarget { Local => "local", Cloud => "cloud" }
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
    /// What a section draft was written from: the applied Sitzungsnotiz facts (cited per sentence)
    /// or the therapist's Werkstatt dictation.
    DraftSource { Facts => "facts", Dictation => "dictation" }
);
str_enum!(
    /// How the therapist answered a Lotse question: typed, dictated or with a quick chip.
    AnswerSource { Typed => "typed", Dictated => "dictated", Chip => "chip" }
);
str_enum!(
    /// Her decision on one sentence of a proposed draft (focus mode, slice 39).
    SentenceDecision { Accepted => "accepted", Rejected => "rejected" }
);
str_enum!(
    /// How a section draft goes into the section: instead of its text or after it.
    ApplyMode { Replace => "replace", Append => "append" }
);
str_enum!(JobStatus { Queued => "queued", Running => "running", Done => "done", Failed => "failed", Cancelled => "cancelled" });
str_enum!(
    /// Who speaks in a segment. In a session recording `unknown` when speakers weren't separated
    /// or not matched to the therapist's voice.
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
    pub gender: Option<Gender>,
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
    #[serde(default)]
    pub gender: Option<Gender>,
}

/// One tile of the patient overview: the patient plus what the card shows at a glance.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PatientCard {
    pub patient: Patient,
    pub has_photo: bool,
    /// Method, age group and diagnoses of the newest open Behandlungsfall (else the newest one).
    pub method: Option<Method>,
    pub age_group: Option<AgeGroup>,
    pub diagnoses: Vec<Diagnosis>,
    /// Next planned session from now on, and the last one before now (not cancelled).
    pub next_at: Option<String>,
    pub last_at: Option<String>,
    /// Past sessions without a signed note plus Berichte still in draft or review.
    #[ts(type = "number")]
    pub open_items: i64,
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
    /// Working diagnoses, therapist-entered ([`crate::store::Store::set_episode_diagnoses`]).
    pub diagnoses: Vec<Diagnosis>,
    /// Start of the Richtlinientherapie (after the Probatorik); sessions from then on count
    /// against the contingent ([`crate::store::Store::set_episode_stage`]).
    pub therapy_since: Option<String>,
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
    /// `planned` | `in_progress` | `recorded` | `cancelled`.
    pub status: String,
    /// Planned length; `None` = [`DEFAULT_SESSION_MINUTES`].
    #[ts(type = "number | null")]
    pub planned_minutes: Option<i64>,
    pub cancelled_reason: Option<String>,
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
    /// People speaking in a session recording, for speaker separation (2–4; 2 by default).
    pub speakers: u8,
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
    /// The diarization engine whose speaker labels the segments carry; `None`: no speakers.
    pub diarization: Option<String>,
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
    /// `therapist`, `patient`, or `speaker_n` when her voice profile didn't decide.
    pub speaker: Option<String>,
    /// The therapist set the speaker by hand.
    pub speaker_edited: bool,
}

/// The therapist's voice profile (speaker separation, slice 27), without the prints themselves.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VoiceProfile {
    #[ts(type = "number")]
    pub dictations: i64,
    #[ts(type = "number")]
    pub sessions: i64,
    pub seconds: f64,
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
    /// Engine chosen when queued (`None` = what Settings say when it runs).
    pub engine: Option<SttTarget>,
    pub created_at: String,
    pub updated_at: String,
    /// Set for a job about a whole session (kind `autopilot`).
    pub session_id: Option<String>,
}

/// What every downstream stage consumes, whatever the capture mode (PLAN §6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SourceSegment {
    /// `transcript_segment.id`: what a fact cites.
    pub segment_id: String,
    pub source_id: String,
    /// A dictation is the therapist alone; in a session recording `role` says who speaks.
    pub kind: SourceKind,
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
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub planned_minutes: Option<i64>,
}

/// Length of a Richtlinien-Einzelsitzung; used when a session has no planned length.
pub const DEFAULT_SESSION_MINUTES: i64 = 50;

/// Reschedule or change a session that hasn't started.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionUpdate {
    pub scheduled_at: String,
    #[ts(type = "number | null")]
    pub planned_minutes: Option<i64>,
    pub modality: Option<String>,
}

/// A session in the calendar, with what the colours and the "Heute" timeline need.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CalendarSession {
    #[serde(flatten)]
    #[ts(flatten)]
    pub session: Session,
    /// Shown locally only; the .ics export uses the Chiffre.
    pub patient_name: String,
    pub chiffre: Option<String>,
    /// Running number among the patient's sessions that weren't cancelled ("Sitzung 12").
    #[ts(type = "number")]
    pub number: i64,
    pub has_prep: bool,
    /// Sources that finished recording, and those of them still without a confirmed transcript.
    #[ts(type = "number")]
    pub sources_finished: i64,
    #[ts(type = "number")]
    pub sources_unreviewed: i64,
    pub recording: bool,
}

/// A to-do on a day (local date), optionally tied to a patient or session.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Task {
    pub id: String,
    pub date: String,
    pub time: Option<String>,
    pub title: String,
    pub done: bool,
    pub patient_id: Option<String>,
    pub session_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaskInput {
    pub date: String,
    pub time: Option<String>,
    pub title: String,
    pub patient_id: Option<String>,
    pub session_id: Option<String>,
}

str_enum!(
    /// One step of the autopilot after a confirmed transcript (slice 31), in run order.
    AutopilotStepKind { Note => "note", Extract => "extract", Summary => "summary" }
);
str_enum!(
    AutopilotStepStatus { Pending => "pending", Running => "running", Done => "done", Skipped => "skipped", Failed => "failed" }
);
str_enum!(
    /// What queued an autopilot run: a confirmed transcript (all steps) or a signed note that
    /// changed a lot since its summary (summary only).
    AutopilotTrigger { Confirm => "confirm", Signed => "signed" }
);
str_enum!(
    /// Derived "Fällig" items on the Heute page.
    DueKind {
        NoteOpen => "note_open",
        PrepMissing => "prep_missing",
        ReportDraft => "report_draft",
        TranscriptReview => "transcript_review",
        ApplicationDue => "application_due",
    }
);

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DueItem {
    pub kind: DueKind,
    pub patient_id: String,
    pub patient_name: String,
    pub chiffre: Option<String>,
    pub session_id: Option<String>,
    pub report_id: Option<String>,
    /// Session time, or the Bericht's last change.
    pub at: Option<String>,
    pub application_kind: Option<ApplicationKind>,
    #[ts(type = "number | null")]
    pub seq: Option<i64>,
    /// `application_due`: the Behandlungsfall and where its contingent stands.
    pub contingent: Option<crate::contingent::Contingent>,
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
    /// An optional section marked "nicht relevant": no blocking, left out of the export.
    #[serde(default)]
    pub not_relevant: bool,
}

/// ICD-10 diagnosis, entered by the therapist only (never AI-filled; MDR boundary).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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
    /// Bericht sections it belongs to; `None` for a fact from before tagging.
    pub bericht: Option<Vec<String>>,
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
    /// Bericht sections as the therapist left them; `None` keeps the proposed ones.
    #[serde(default)]
    #[ts(optional)]
    pub bericht: Option<Vec<String>>,
}

/// Provenance of one extraction run (PLAN Â§5: model, provider, prompt version and hash).
#[derive(Debug, Clone)]
pub struct ExtractionMeta {
    pub provider: String,
    pub model: String,
    pub prompt_hash: String,
    pub chunks: usize,
    pub dropped: usize,
    /// Repeated facts folded into others (`extract::merge`).
    pub merged: usize,
    /// Bericht tags were asked for (the note belongs to a treatment case). Without, the facts
    /// stay untagged and are offered to every section.
    pub tagged: bool,
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
    /// Bericht sections it belongs to; `None` for a fact from before tagging (every section).
    pub bericht: Option<Vec<String>>,
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
    /// Sessions the draft drew on; `None` = all sessions of the episode.
    pub session_ids: Option<Vec<String>>,
    /// The section's existing text went along ("ergänzen").
    pub with_existing: bool,
    pub source: DraftSource,
    pub sentences: Vec<DraftSentenceView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DraftSentenceView {
    pub id: String,
    pub text: String,
    pub sources: Vec<EpisodeFact>,
    /// Her answers to Lotse questions it rests on ("Ihre Angabe").
    pub answers: Vec<ReportAnswer>,
    /// Sections of the Vorbericht it rests on ("Vorbericht (Datum)").
    pub prior: Vec<PriorSource>,
    /// Taken over or discarded one by one in the focus mode; `None` while undecided.
    pub decision: Option<SentenceDecision>,
    /// Taken over in her own wording.
    pub edited: bool,
}

/// A section of the Vorbericht a draft sentence cites (slice 40).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PriorSource {
    pub report_id: String,
    pub section_key: String,
    pub signed_at: Option<String>,
    /// The start of the section's text.
    pub excerpt: String,
}

/// A Vorbericht section a draft run offered, in the order of its input (see `Store::save_drafts`).
#[derive(Debug, Clone)]
pub struct PriorRef {
    pub report_id: String,
    pub section_key: String,
}

/// The therapist's answer to one Lotse question of a Bericht (slice 38).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReportAnswer {
    pub id: String,
    pub section_key: String,
    pub ask_key: String,
    /// The question as it was asked.
    pub question: String,
    pub text: String,
    pub source: AnswerSource,
    pub updated_at: String,
}

/// Provenance of one drafting run.
#[derive(Debug, Clone)]
pub struct DraftMeta {
    pub provider: String,
    pub model: String,
    pub prompt_hash: String,
    pub instructed: bool,
    /// `None` = all sessions of the episode.
    pub session_ids: Option<Vec<String>>,
    pub source: DraftSource,
}

/// One live dictation, for the audit log (no text).
#[derive(Debug, Clone, Default)]
pub struct DictationLog {
    /// `None` = transcribed on this computer.
    pub provider: Option<String>,
    pub model: String,
    /// Seconds of speech transcribed.
    pub seconds: f64,
    pub pieces: usize,
    pub requests: usize,
    pub cost: Option<f64>,
    /// Why the dictation stopped early, if it did (no text).
    pub error: Option<String>,
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PrepSpeaker {
    Therapist,
    Assistant,
}

/// One turn of a voice prep conversation, tokens already restored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PrepTurn {
    pub speaker: PrepSpeaker,
    pub text: String,
}

/// A voice prep before a session (slice 26).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SessionPrep {
    pub session_id: String,
    pub transcript: Vec<PrepTurn>,
    /// Billed voice duration.
    pub seconds: Option<f64>,
    /// Prep sheet HTML (editable); `None` until generated.
    pub sheet: Option<String>,
    pub sheet_model: Option<String>,
    pub created_at: String,
    pub updated_at: String,
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
    /// Sections with text (or a diagnosis) out of all the Bericht's sections.
    #[ts(type = "number")]
    pub sections_filled: i64,
    #[ts(type = "number")]
    pub sections_total: i64,
    /// Open Berichte only, while the case is still in the stage the application leads out of:
    /// sessions left in the approved contingent (≤ 0 = used up) …
    #[ts(type = "number | null")]
    pub sessions_left: Option<i64>,
    /// … and the planned session that uses up the last one, when the calendar has it.
    pub due_by: Option<String>,
}

/// A row in the Sitzungen work queue: the calendar row plus what the list shows besides it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueueSession {
    #[serde(flatten)]
    #[ts(flatten)]
    pub row: CalendarSession,
    pub first_name: String,
    pub last_name: String,
    pub has_photo: bool,
    /// The session summary's first key point.
    pub summary_head: Option<String>,
}

// ---------- assistant (slice 25) ----------

str_enum!(
    /// What an assistant thread is about: everything, or one patient / one session (its tools
    /// then see only that patient).
    ChatScope { Global => "global", Patient => "patient", Session => "session" }
);

str_enum!(ChatRole { User => "user", Assistant => "assistant", Tool => "tool" });

str_enum!(
    /// What an accepted proposal does, each through the normal store command.
    ProposalKind {
        Session => "session",
        Reschedule => "reschedule",
        Task => "task",
        SectionText => "section_text",
        NoteText => "note_text",
        DayPlan => "day_plan",
    }
);

str_enum!(ProposalStatus { Open => "open", Applied => "applied", Discarded => "discarded" });

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChatThread {
    pub id: String,
    pub scope: ChatScope,
    pub ref_id: Option<String>,
    /// The first question, shortened; `None` until she asked something.
    pub title: Option<String>,
    /// The scoped patient's name (shown locally only).
    pub patient_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A tool call as stored and shown ("Was ich getan habe"). Arguments are restored.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChatToolCall {
    pub id: String,
    pub name: String,
    #[ts(type = "unknown")]
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChatMessage {
    pub id: String,
    #[ts(type = "number")]
    pub seq: i64,
    pub role: ChatRole,
    /// Restored text. For a tool message: what the tool returned.
    pub content: String,
    pub tool_calls: Vec<ChatToolCall>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentProposal {
    pub id: String,
    pub thread_id: String,
    /// The tool message that created it.
    pub message_id: String,
    pub kind: ProposalKind,
    pub patient_id: Option<String>,
    pub summary: String,
    #[ts(type = "unknown")]
    pub payload: serde_json::Value,
    pub status: ProposalStatus,
    /// What applying created or changed (session, task, report id; the date of a day plan).
    pub result_id: Option<String>,
    pub created_at: String,
    pub decided_at: Option<String>,
}
