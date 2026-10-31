// Typed wrappers over the Tauri commands in crates/app/src/lib.rs.
// Types come from ts-rs (`cargo test -p thera-core` regenerates src/bindings).
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { queryOptions } from '@tanstack/react-query'
import type { Application } from '@/bindings/Application'
import type { ApplicationKind } from '@/bindings/ApplicationKind'
import type { AuditEvent } from '@/bindings/AuditEvent'
import type { BerichtCandidate } from '@/bindings/BerichtCandidate'
import type { CheckItem } from '@/bindings/CheckItem'
import type { ReportReview } from '@/bindings/ReportReview'
import type { Consent } from '@/bindings/Consent'
import type { DeleteOutcome } from '@/bindings/DeleteOutcome'
import type { TrashItem } from '@/bindings/TrashItem'
import type { ConsentKind } from '@/bindings/ConsentKind'
import type { ConsentText } from '@/bindings/ConsentText'
import type { Episode } from '@/bindings/Episode'
import type { EpisodeInput } from '@/bindings/EpisodeInput'
import type { Job } from '@/bindings/Job'
import type { Patient } from '@/bindings/Patient'
import type { PatientInput } from '@/bindings/PatientInput'
import type { PatientCard } from '@/bindings/PatientCard'
import type { QueueSession } from '@/bindings/QueueSession'
import type { Diagnosis } from '@/bindings/Diagnosis'
import type { Report } from '@/bindings/Report'
import type { ReportStatus } from '@/bindings/ReportStatus'
import type { ReportSummary } from '@/bindings/ReportSummary'
import type { Session } from '@/bindings/Session'
import type { SessionInput } from '@/bindings/SessionInput'
import type { SessionSource } from '@/bindings/SessionSource'
import type { SessionSummary } from '@/bindings/SessionSummary'
import type { SummaryPoint } from '@/bindings/SummaryPoint'
import type { TimelineGroup } from '@/bindings/TimelineGroup'
import type { AutopilotRun } from '@/bindings/AutopilotRun'
import type { AutopilotStepKind } from '@/bindings/AutopilotStepKind'
import type { Contingent } from '@/bindings/Contingent'
import type { Stage } from '@/bindings/Stage'
import type { SourceKind } from '@/bindings/SourceKind'
import type { SourceSegment } from '@/bindings/SourceSegment'
import type { Template } from '@/bindings/Template'
import type { Transcript } from '@/bindings/Transcript'
import type { DiarizeConfig } from '@/bindings/DiarizeConfig'
import type { VoiceProfile } from '@/bindings/VoiceProfile'
import type { BackupSummary } from '@/bindings/BackupSummary'
import type { CloudModel } from '@/bindings/CloudModel'
import type { LlmConfig } from '@/bindings/LlmConfig'
import type { LlmSpend } from '@/bindings/LlmSpend'
import type { SpendReport } from '@/bindings/SpendReport'
import type { DaySpend } from '@/bindings/DaySpend'
import type { KeyInfo } from '@/bindings/KeyInfo'
import type { Credits } from '@/bindings/Credits'
import type { Tier } from '@/bindings/Tier'
import type { ProviderId } from '@/bindings/ProviderId'
import type { ProviderInfo } from '@/bindings/ProviderInfo'
import type { RemoteModel } from '@/bindings/RemoteModel'
import type { Fit } from '@/bindings/Fit'
import type { Hardware } from '@/bindings/Hardware'
import type { Recommendation } from '@/bindings/Recommendation'
import type { EntityKind } from '@/bindings/EntityKind'
import type { FactEdit } from '@/bindings/FactEdit'
import type { NoteFact } from '@/bindings/NoteFact'
import type { Preview } from '@/bindings/Preview'
import type { PseudoTerm } from '@/bindings/PseudoTerm'
import type { ApplyMode } from '@/bindings/ApplyMode'
import type { EpisodeFact } from '@/bindings/EpisodeFact'
import type { ReportAnswer } from '@/bindings/ReportAnswer'
import type { AnswerSource } from '@/bindings/AnswerSource'
import type { SectionDef } from '@/bindings/SectionDef'
import type { SectionDraft } from '@/bindings/SectionDraft'
import type { SentenceChoice } from '@/bindings/SentenceChoice'
import type { SttConfig } from '@/bindings/SttConfig'
import type { CloudSttModel } from '@/bindings/CloudSttModel'
import type { SttTarget } from '@/bindings/SttTarget'
import type { TtsConfig } from '@/bindings/TtsConfig'
import type { TtsVoice } from '@/bindings/TtsVoice'
import type { LiveConfig } from '@/bindings/LiveConfig'
import type { LiveVoice } from '@/bindings/LiveVoice'
import type { LiveSession } from '@/bindings/LiveSession'
import type { BriefStats } from '@/bindings/BriefStats'
import type { PrepTurn } from '@/bindings/PrepTurn'
import type { SessionPrep } from '@/bindings/SessionPrep'
import type { CalendarSession } from '@/bindings/CalendarSession'
import type { DueItem } from '@/bindings/DueItem'
import type { AgentProposal } from '@/bindings/AgentProposal'
import type { ChatMessage } from '@/bindings/ChatMessage'
import type { ChatScope } from '@/bindings/ChatScope'
import type { ChatThread } from '@/bindings/ChatThread'
import type { Zone } from '@/bindings/Zone'
import type { SessionUpdate } from '@/bindings/SessionUpdate'
import type { Task } from '@/bindings/Task'
import type { TaskInput } from '@/bindings/TaskInput'

export type InitResult = {
  status: 'ready' | 'recovery_required' | 'new'
  recovery_key: string | null
  recovery_confirmed: boolean
  encrypted: boolean
  /** This window shows the Demo-Praxis (fictional data in its own folder). */
  demo: boolean
}

export type UpdateOffer = { version: string; notes: string | null }
export type UpdateView = { current: string; available: UpdateOffer | null }
export type UpdateProgress = { received: number; total: number | null }
export type BackupHeaderView = { created_at: string; app_version: string; keychain_matches: boolean }
export type BackupDone = { path: string; summary: BackupSummary }
export type RestoreDone = { summary: BackupSummary; previous: string | null }
export type Rotated = { recovery_key: string; rekeyed: number; rekey_failed: number }
export type LastBackup = { at: string; file: string; bytes: number }

export type ReportView = {
  report: Report
  template: Template
  patient: Patient
  episode: Episode | null
  application: Application | null
  /** Sitzungsnotiz only: its session and the confirmed transcript segments to pick from. */
  session: Session | null
  segments: SourceSegment[]
}

/** What a recording can be transcribed with right now (choice in the session). */
export type SourceStt = {
  default: SttTarget
  local_problem: string | null
  cloud_problem: string | null
  cloud_needs_consent: boolean
  cloud_label: string
}
export type SourceView = SessionSource & { job: Job | null; transcript: Transcript | null; stt: SourceStt }

export type SessionView = {
  session: Session
  patient: Patient
  sources: SourceView[]
}

export type Transfer = { received: number; total: number; error: string | null }

export type SttOverview = {
  engine: string | null
  active: string
  models: { id: string; file: string; bytes: number; installed: boolean; transfer: Transfer | null }[]
  config: SttConfig
  cloud_models: CloudSttModel[]
  /** Why the cloud can't be used right now, or null. */
  cloud_problem: string | null
  local_ready: boolean
}

export type DiarizeOverview = {
  /** null: sherpa-onnx or the speaker models are missing from this build. */
  engine: string | null
  config: DiarizeConfig
  profile: VoiceProfile
}

export type LlmOverview = { config: LlmConfig; providers: (ProviderInfo & { has_key: boolean })[] }
export type LlmTestResult = { model: string; latency_ms: number; answer: string }
export type LlmCatalog = { models: CloudModel[]; recommended: Tier }
export type LlmSpendView = { month: LlmSpend; total: LlmSpend }
export type LlmCostsView = LlmSpendView & { breakdown: SpendReport; days: DaySpend[] }
export type BalanceView = {
  key: KeyInfo
  has_mgmt_key: boolean
  /** The account balance; only with a management key. */
  credits: Credits | null
  credits_error: string | null
  mgmt_key_url: string
}
export type TtsOverview = {
  config: TtsConfig
  voices: TtsVoice[]
  default_models: Partial<Record<ProviderId, string>>
  problem: string | null
}
export type LiveOverview = {
  config: LiveConfig
  voices: LiveVoice[]
  usd_per_minute: number
  problem: string | null
  eu: boolean
  no_backend: string
}
export type PrepPreview = Preview & { stats: BriefStats; terms: PseudoTerm[] }
/** OpenAI's EU data-residency host (`llm::OPENAI_EU_BASE_URL`). */
export const OPENAI_EU_BASE_URL = 'https://eu.api.openai.com/v1'
/** Redaction preview: the outgoing text, the local token table and where it would go. */
export type ExtractPreview = Preview & { terms: PseudoTerm[]; provider: ProviderInfo | null; model: string | null }

/** Which engines a live dictation can use right now (the therapist's own voice). */
export type DictationOverview = {
  default: SttTarget
  local_problem: string | null
  cloud_problem: string | null
  cloud_label: string
}
export type DictationStatus = {
  id: string
  /** `null`: a question to the assistant. */
  report_id: string | null
  engine: SttTarget
  duration_ms: number
  level: number
  /** An utterance is in progress. */
  speaking: boolean
  /** Pieces cut but not transcribed yet. */
  pending: number
  /** The local speech model is loading. */
  loading: boolean
  error: string | null
}
/** One transcribed piece (`text: null`: nothing was said). */
export type DictationPiece = { dictation_id: string; report_id: string | null; seq: number; text: string | null }
export type DictationDone = { dictation_id: string; report_id: string | null; error: string | null }
/** "Entwurf erstellen" in the Werkstatt. */
export type WorkshopChoice = { instruction: string | null; withExisting: boolean }
/** What the therapist chose in the draft dialog (`sessionIds: null` = all sessions). */
export type DraftChoice = {
  sections: string[]
  instruction: string | null
  sessionIds: string[] | null
  /** Send the sections' current text along so the draft only adds to it. */
  withExisting: boolean
  /** Ignore the Bericht tags: every fact to every section. */
  allFacts: boolean
  /** Leave the Vorbericht out. */
  withoutPrior?: boolean
}
/** `skipped`: sections no fact is tagged for (nothing sent); `untagged`: facts from before tagging. */
export type DraftPreview = ExtractPreview & { skipped: string[]; untagged: number }
/** The last signed Bericht of the case, which drafts see too (`sections`: those with text they draw on). */
export type PriorInfo = { report_id: string; kind: ApplicationKind | null; seq: number | null; signed_at: string | null; sections: number }
/** `since`: when the previous application was created (sessions after it are the default choice). */
export type DraftSources = { facts: EpisodeFact[]; since: string | null; answers: number; prior: PriorInfo | null }
/** The Gutachter-Check of step 4 (`stale`: the text changed since; `sections`: what a check would read now). */
export type ReviewView = { review: ReportReview | null; stale: boolean; sections: number }
/** One question of the Lotse "Fragen" step (slice 38). */
export type QuestionView = {
  section_key: string
  number: string | null
  heading: string
  ask_key: string
  question: string
  chips: string[]
  /** From the patient record ("41 Jahre, weiblich"), offered as the first chip. */
  record: string | null
  answer: ReportAnswer | null
  /** Session facts that answer it, by the last check; empty once she answered herself. */
  covered_by: EpisodeFact[]
}
export type ReportQuestions = {
  questions: QuestionView[]
  check: { model: string; checked_at: string; stale: boolean } | null
  /** Questions a check would send now. */
  checkable: number
  /** Session facts a check would read. */
  facts: number
}
/** `chunks` < `total` when cancelled: facts from the finished chunks were kept. */
export type ExtractResult = { facts: NoteFact[]; dropped: number; chunks: number; total: number; cancelled: boolean }
export type SummaryResult = { summary: SessionSummary | null; dropped: number; cancelled: boolean }
/** `done` < `total` when cancelled: the sections drafted before were kept. */
export type DraftResult = {
  drafts: SectionDraft[]
  sentences: number
  dropped: number
  done: number
  total: number
  cancelled: boolean
}
/** A piece of streamed answer text, still tokenized. Shown as progress only. */
export type LlmDelta = { request_id: string; text: string }
/** An assistant thread with everything the chat shows. `steps`: a German line per tool call id. */
export type ChatView = {
  thread: ChatThread
  messages: ChatMessage[]
  steps: Record<string, string>
  proposals: AgentProposal[]
}
export type ChatEnd = 'answered' | 'cancelled' | 'step_limit'
/** The assistant's answer so far, restored (`agent:text`). */
export type AgentText = { request_id: string; thread_id: string; text: string }
export type ExtractProgress = { report_id: string; stage: 'loading' | 'running'; done: number; total: number }

export type LocalModelView = {
  id: string
  label: string
  bytes: number
  fit: Fit
  experimental: boolean
  installed: boolean
  transfer: Transfer | null
}
export type LocalOverview = {
  engine: string | null
  hardware: Hardware
  recommendation: Recommendation
  models: LocalModelView[]
  loaded: string | null
  loading: string | null
}

export const jobActive = (job: Job | null | undefined) => job?.status === 'queued' || job?.status === 'running'

export type InputDevice = { id: string; name: string; is_default: boolean }

export type RecordingStatus = {
  source_id: string
  session_id: string
  kind: SourceKind
  duration_ms: number
  level: number
  error: string | null
  minutes_left: number | null
}

export const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export const api = {
  init: () => invoke<InitResult>('app_init'),
  recover: (recoveryKey: string) => invoke<void>('app_recover', { recoveryKey }),
  exportRecoveryKit: (recoveryKey: string, rotated: boolean, path: string) =>
    invoke<void>('recovery_kit_export', { recoveryKey, rotated, path }),
  rotateRecoveryKey: () => invoke<Rotated>('recovery_key_rotate'),
  setup: () => invoke<InitResult>('app_setup'),
  /** Restarts the app in the Demo-Praxis (fresh fictional data in its own folder) or back. */
  demoEnter: () => invoke<void>('demo_enter'),
  demoLeave: () => invoke<void>('demo_leave'),
  /** In the demo: the real practice's AI setup it may use (provider label, model), if any. */
  demoAiInfo: () => invoke<{ provider: string; model: string | null; active: boolean } | null>('demo_ai_info'),
  demoAiUse: () => invoke<void>('demo_ai_use'),
  /** Fetches only `latest.json` from the release page. */
  updateCheck: () => invoke<UpdateView>('update_check'),
  /** Downloads, verifies and starts the installer; the app closes and restarts on success. */
  updateInstall: () => invoke<void>('update_install'),
  onUpdateProgress: (cb: (p: UpdateProgress) => void): Promise<UnlistenFn> =>
    listen<UpdateProgress>('update:progress', (e) => cb(e.payload)),
  createBackup: () => invoke<BackupDone>('backup_create'),
  inspectBackup: (path: string) => invoke<BackupHeaderView>('backup_inspect', { path }),
  restoreBackup: (path: string, recoveryKey: string | null) =>
    invoke<RestoreDone>('backup_restore', { path, recoveryKey }),
  confirmRecoveryKit: () => invoke<void>('recovery_kit_confirm'),

  patients: (query?: string) => invoke<Patient[]>('patients_list', { query: query || null }),
  patient: (id: string) => invoke<Patient>('patient_get', { id }),
  createPatient: (input: PatientInput) => invoke<Patient>('patient_create', { input }),
  updatePatient: (id: string, input: PatientInput) => invoke<Patient>('patient_update', { id, input }),
  erasePatient: (id: string) => invoke<void>('patient_erase', { id }),
  patientCards: (query: string, now: string) => invoke<PatientCard[]>('patient_cards', { query: query || null, now }),
  patientPhoto: (id: string) => invoke<string | null>('patient_photo', { id }),
  setPatientPhoto: (id: string, path: string) => invoke<void>('patient_photo_set', { id, path }),
  patientPhotoPreview: (path: string) => invoke<string>('patient_photo_preview', { path }),
  removePatientPhoto: (id: string) => invoke<void>('patient_photo_remove', { id }),

  exportConsentForm: (patientId: string, kind: ConsentKind, path: string) =>
    invoke<void>('consent_form_export', { patientId, kind, path }),
  consentText: (kind: ConsentKind) => invoke<ConsentText>('consent_text', { kind }),
  consents: (patientId: string) => invoke<Consent[]>('consents_list', { patientId }),
  grantConsent: (patientId: string, kind: ConsentKind, note: string) =>
    invoke<Consent>('consent_grant', { patientId, kind, note: note || null }),
  revokeConsent: (id: string) => invoke<Consent>('consent_revoke', { id }),

  episodes: (patientId: string) => invoke<Episode[]>('episodes_list', { patientId }),
  createEpisode: (input: EpisodeInput) => invoke<Episode>('episode_create', { input }),
  setEpisodeDiagnoses: (episodeId: string, diagnoses: Diagnosis[]) =>
    invoke<Episode>('episode_diagnoses_set', { episodeId, diagnoses }),
  applications: (episodeId: string) => invoke<Application[]>('applications_list', { episodeId }),
  createApplication: (episodeId: string, kind: ApplicationKind) =>
    invoke<Application>('application_create', { episodeId, kind }),
  changeApplicationKind: (id: string, kind: ApplicationKind) => invoke<Application>('application_change_kind', { id, kind }),
  berichtCandidates: (episodeId?: string) =>
    invoke<BerichtCandidate[]>('bericht_candidates', { episodeId: episodeId ?? null }),

  sessions: (patientId?: string) => invoke<Session[]>('sessions_list', { patientId: patientId ?? null }),
  createSession: (input: SessionInput) => invoke<Session>('session_create', { input }),
  session: (id: string) => invoke<SessionView>('session_get', { id }),
  createSessionNote: (sessionId: string) => invoke<Report>('session_note_create', { sessionId }),
  createSessions: (inputs: SessionInput[]) => invoke<Session[]>('sessions_create_series', { inputs }),
  updateSession: (id: string, update: SessionUpdate) => invoke<Session>('session_update', { id, update }),
  cancelSession: (id: string, reason: string | null) => invoke<Session>('session_cancel', { id, reason }),
  reopenSession: (id: string) => invoke<Session>('session_reopen', { id }),
  deleteSession: (id: string) => invoke<DeleteOutcome>('session_delete', { id }),

  // Calendar: `from`/`to` are instants (RFC 3339), task and note dates are local 'YYYY-MM-DD'.
  calendar: (from: string, to: string) => invoke<CalendarSession[]>('calendar_range', { from, to }),
  sessionQueue: () => invoke<QueueSession[]>('session_queue'),
  exportIcs: (from: string, to: string, path: string) => invoke<number>('calendar_export_ics', { from, to, path }),
  dueItems: (from: string, dayStart: string, until: string) => invoke<DueItem[]>('due_items', { from, dayStart, until }),
  tasks: (from: string, to: string, openBefore: boolean) => invoke<Task[]>('tasks_list', { from, to, openBefore }),
  createTask: (input: TaskInput) => invoke<Task>('task_create', { input }),
  updateTask: (id: string, input: TaskInput) => invoke<Task>('task_update', { id, input }),
  setTaskDone: (id: string, done: boolean) => invoke<Task>('task_set_done', { id, done }),
  deleteTask: (id: string) => invoke<void>('task_delete', { id }),
  dayNote: (date: string) => invoke<string | null>('day_note_get', { date }),
  setDayNote: (date: string, body: string) => invoke<void>('day_note_set', { date, body }),

  audioDevices: () => invoke<InputDevice[]>('audio_devices'),
  startRecording: (sessionId: string, kind: SourceKind, deviceId: string | null) =>
    invoke<SessionSource>('recording_start', { sessionId, kind, deviceId }),
  recordingStatus: () => invoke<RecordingStatus | null>('recording_status'),
  stopRecording: () => invoke<SessionSource>('recording_stop'),
  dictationOverview: () => invoke<DictationOverview>('dictation_overview'),
  /** Live dictation into a report's editor or Werkstatt (`null`: the assistant's question box);
   *  pieces arrive as `dictation:piece`. */
  dictationStart: (reportId: string | null, engine: SttTarget | null, deviceId: string | null) =>
    invoke<{ id: string; engine: SttTarget }>('dictation_start', { reportId, engine, deviceId }),
  dictationStatus: () => invoke<DictationStatus | null>('dictation_status'),
  /** The utterance in progress is still transcribed unless `discard`. */
  dictationStop: (discard = false) => invoke<void>('dictation_stop', { discard }),
  onDictationPiece: (cb: (p: DictationPiece) => void): Promise<UnlistenFn> =>
    listen<DictationPiece>('dictation:piece', (e) => cb(e.payload)),
  onDictationDone: (cb: (d: DictationDone) => void): Promise<UnlistenFn> =>
    listen<DictationDone>('dictation:done', (e) => cb(e.payload)),
  workshopGet: (reportId: string) => invoke<string>('workshop_get', { reportId }),
  workshopSet: (reportId: string, body: string) => invoke<void>('workshop_set', { reportId, body }),
  workshopPreview: (reportId: string, choice: WorkshopChoice) =>
    invoke<ExtractPreview>('workshop_preview', { reportId, choice }),
  workshopRun: (reportId: string, choice: WorkshopChoice, digest: string, requestId: string) =>
    invoke<DraftResult>('workshop_run', { reportId, choice, digest, requestId }),
  /** WAV bytes of `fromMs..toMs`, decrypted in memory; only until the transcript is confirmed. */
  sourceAudio: (sourceId: string, fromMs: number, toMs: number) =>
    invoke<ArrayBuffer>('source_audio_wav', { sourceId, fromMs, toMs }),

  startTranscription: (sourceId: string, engine: SttTarget | null) =>
    invoke<Job>('transcription_start', { sourceId, engine }),
  cancelTranscription: (jobId: string) => invoke<void>('transcription_cancel', { jobId }),
  editSegment: (transcriptId: string, idx: number, text: string) =>
    invoke<void>('transcript_edit_segment', { transcriptId, idx, text }),
  confirmTranscript: (id: string) => invoke<Transcript>('transcript_confirm', { id }),
  setSegmentSpeaker: (transcriptId: string, idx: number, label: string) =>
    invoke<void>('transcript_set_speaker', { transcriptId, idx, label }),
  rediarize: (sourceId: string, speakers: number) => invoke<Job>('transcript_rediarize', { sourceId, speakers }),
  swapSpeakers: (transcriptId: string) => invoke<Transcript>('transcript_swap_speakers', { transcriptId }),
  assignTherapist: (transcriptId: string, label: string) =>
    invoke<Transcript>('transcript_assign_therapist', { transcriptId, label }),
  diarize: () => invoke<DiarizeOverview>('diarize_overview'),
  setDiarizeConfig: (config: DiarizeConfig) => invoke<void>('diarize_config_set', { config }),
  deleteVoiceProfile: () => invoke<number>('voice_profile_delete'),

  stt: () => invoke<SttOverview>('stt_overview'),
  selectModel: (id: string) => invoke<void>('stt_model_select', { id }),
  setSttConfig: (config: SttConfig) => invoke<void>('stt_config_set', { config }),
  deleteModel: (id: string) => invoke<void>('stt_model_delete', { id }),
  downloadModel: (id: string) => invoke<void>('stt_model_download', { id }),
  importModel: (id: string, path: string) => invoke<void>('stt_model_import', { id, path }),

  llm: () => invoke<LlmOverview>('llm_overview'),
  setLlmConfig: (config: LlmConfig) => invoke<void>('llm_config_set', { config }),
  setLlmKey: (provider: ProviderId, key: string | null) => invoke<void>('llm_key_set', { provider, key }),
  llmModels: (provider: ProviderId) => invoke<RemoteModel[]>('llm_models', { provider }),
  testLlm: (provider: ProviderId) => invoke<LlmTestResult>('llm_test', { provider }),
  llmCatalog: () => invoke<LlmCatalog>('llm_catalog'),
  llmSpend: () => invoke<LlmSpendView>('llm_spend'),
  llmCosts: (all: boolean) => invoke<LlmCostsView>('llm_costs', { all }),
  /** OpenRouter only. */
  llmBalance: () => invoke<BalanceView>('llm_balance'),
  setMgmtKey: (key: string | null) => invoke<void>('llm_mgmt_key_set', { key }),
  /** Checks the stored key without a model and without sending text (key guide). */
  checkLlmKey: (provider: ProviderId) => invoke<{ free_tier: boolean }>('llm_key_check', { provider }),
  tts: () => invoke<TtsOverview>('tts_overview'),
  setTtsConfig: (config: TtsConfig) => invoke<void>('tts_config_set', { config }),
  /** MP3 of a fixed sample sentence with the saved read-aloud settings. */
  ttsSample: () => invoke<ArrayBuffer>('tts_sample'),
  live: () => invoke<LiveOverview>('live_overview'),
  setLiveConfig: (config: LiveConfig) => invoke<void>('live_config_set', { config }),
  prepPreview: (sessionId: string) => invoke<PrepPreview>('prep_preview', { sessionId }),
  prepStart: (sessionId: string, digest: string, sdp: string) => invoke<LiveSession>('prep_start', { sessionId, digest, sdp }),
  prepFinish: (sessionId: string, turns: PrepTurn[], seconds: number | null, reason: string) =>
    invoke<SessionPrep>('prep_finish', { sessionId, turns, seconds, reason }),
  prepAbort: (sessionId: string, seconds: number | null, reason: string) =>
    invoke<void>('prep_abort', { sessionId, seconds, reason }),
  prep: (sessionId: string) => invoke<SessionPrep | null>('prep_get', { sessionId }),
  setPrepSheet: (sessionId: string, sheet: string) => invoke<void>('prep_sheet_set', { sessionId, sheet }),
  prepSheetRun: (sessionId: string, requestId: string) => invoke<SessionPrep | null>('prep_sheet_run', { sessionId, requestId }),

  extractPreview: (reportId: string) => invoke<ExtractPreview>('extract_preview', { reportId }),
  extractRun: (reportId: string, digest: string, requestId: string) =>
    invoke<ExtractResult>('extract_run', { reportId, digest, requestId }),
  addPseudoTerm: (patientId: string, kind: EntityKind, text: string) =>
    invoke<PseudoTerm>('pseudo_term_add', { patientId, kind, text }),
  removePseudoTerm: (id: string) => invoke<void>('pseudo_term_remove', { id }),
  noteFacts: (reportId: string) => invoke<NoteFact[]>('note_facts', { reportId }),
  sessionSummary: (sessionId: string) => invoke<SessionSummary | null>('session_summary', { sessionId }),
  summaryPreview: (sessionId: string) => invoke<ExtractPreview>('summary_preview', { sessionId }),
  summaryRun: (sessionId: string, digest: string, requestId: string) =>
    invoke<SummaryResult>('summary_run', { sessionId, digest, requestId }),
  summaryEdit: (sessionId: string, points: SummaryPoint[]) => invoke<SessionSummary>('summary_edit', { sessionId, points }),
  patientTimeline: (patientId: string) => invoke<TimelineGroup[]>('patient_timeline', { patientId }),

  autopilotEnabled: () => invoke<boolean>('autopilot_get'),
  setAutopilot: (on: boolean) => invoke<void>('autopilot_set', { on }),
  autopilotRun: (sessionId: string) => invoke<AutopilotRun | null>('autopilot_run', { sessionId }),
  /** "Was wurde gesendet?": the pseudonymized requests of one autopilot step and the local token table. */
  autopilotSent: (jobId: string, step: AutopilotStepKind) => invoke<Preview | null>('autopilot_sent', { jobId, step }),
  retryAutopilot: (jobId: string) => invoke<Job>('autopilot_retry', { jobId }),
  onTranscriptReady: (cb: (e: { session_id: string; patient_name: string }) => void): Promise<UnlistenFn> =>
    listen<{ session_id: string; patient_name: string }>('transcript-ready', (e) => cb(e.payload)),
  onAutopilotChanged: (cb: (sessionId: string) => void): Promise<UnlistenFn> =>
    listen<{ session_id: string }>('autopilot-changed', (e) => cb(e.payload.session_id)),
  setEpisodeStage: (episodeId: string, stage: Stage, therapySince: string | null) =>
    invoke<Episode>('episode_stage_set', { episodeId, stage, therapySince }),
  contingent: (episodeId: string) => invoke<Contingent>('episode_contingent', { episodeId }),
  rejectFacts: (reportId: string, ids: string[]) => invoke<NoteFact[]>('note_facts_reject', { reportId, ids }),
  applyFacts: (reportId: string, facts: FactEdit[]) => invoke<Report>('note_facts_apply', { reportId, facts }),
  factTagSections: (reportId: string) => invoke<SectionDef[]>('fact_tag_sections', { reportId }),
  draftSources: (reportId: string) => invoke<DraftSources>('draft_sources', { reportId }),
  reportQuestions: (reportId: string) => invoke<ReportQuestions>('report_questions', { reportId }),
  setAnswer: (reportId: string, sectionKey: string, askKey: string, text: string, source: AnswerSource) =>
    invoke<ReportAnswer | null>('report_answer_set', { reportId, sectionKey, askKey, text, source }),
  gapsPreview: (reportId: string) => invoke<ExtractPreview>('gaps_preview', { reportId }),
  gapsRun: (reportId: string, digest: string, requestId: string) =>
    invoke<ReportQuestions>('gaps_run', { reportId, digest, requestId }),
  review: (reportId: string) => invoke<ReviewView>('report_review_get', { reportId }),
  reviewIgnore: (reportId: string, hintId: string, ignored: boolean) =>
    invoke<ReviewView>('report_review_ignore', { reportId, hintId, ignored }),
  reviewPreview: (reportId: string) => invoke<ExtractPreview>('review_preview', { reportId }),
  reviewRun: (reportId: string, digest: string, requestId: string) =>
    invoke<ReviewView>('review_run', { reportId, digest, requestId }),
  draftPreview: (reportId: string, choice: DraftChoice) => invoke<DraftPreview>('draft_preview', { reportId, choice }),
  draftRun: (reportId: string, choice: DraftChoice, digest: string, requestId: string) =>
    invoke<DraftResult>('draft_run', { reportId, choice, digest, requestId }),
  /** Stops a running extraction or draft; what finished before is kept. */
  llmCancel: (requestId: string) => invoke<void>('llm_cancel', { requestId }),
  /** Streamed answer text of one request (`llm:delta` events from the backend). */
  onLlmDelta: (requestId: string, cb: (text: string) => void): Promise<UnlistenFn> =>
    listen<LlmDelta>('llm:delta', (e) => {
      if (e.payload.request_id === requestId) cb(e.payload.text)
    }),
  reportDrafts: (reportId: string) => invoke<SectionDraft[]>('report_drafts', { reportId }),
  rejectDraft: (reportId: string, draftId: string) => invoke<SectionDraft[]>('report_draft_reject', { reportId, draftId }),
  applyDraft: (reportId: string, draftId: string, mode: ApplyMode) =>
    invoke<Report>('report_draft_apply', { reportId, draftId, mode }),
  /** Focus mode: sentences taken over (maybe in her wording) or discarded one by one. */
  decideSentences: (reportId: string, draftId: string, choices: SentenceChoice[]) =>
    invoke<Report>('report_draft_decide', { reportId, draftId, choices }),
  /** The Bericht sections an applied fact belongs to (Erst-/Umwandlungsbericht keys). */
  setFactSections: (reportId: string, factId: string, sections: string[]) =>
    invoke<void>('fact_sections_set', { reportId, factId, sections }),

  local: () => invoke<LocalOverview>('local_overview'),
  downloadLocalModel: (id: string) => invoke<void>('local_model_download', { id }),
  importLocalModel: (id: string, path: string) => invoke<void>('local_model_import', { id, path }),
  deleteLocalModel: (id: string) => invoke<void>('local_model_delete', { id }),
  unloadLocal: () => invoke<void>('local_unload'),

  templates: () => invoke<Template[]>('templates_list'),

  reports: (status?: ReportStatus) => invoke<ReportSummary[]>('reports_list', { status: status ?? null }),
  report: (id: string) => invoke<ReportView>('report_get', { id }),
  deleteReport: (id: string) => invoke<DeleteOutcome>('report_delete', { id }),
  trash: () => invoke<TrashItem[]>('trash_list'),
  trashRestore: (id: string) => invoke<void>('trash_restore', { id }),
  /** One item, or without `id` the whole Papierkorb. */
  trashPurge: (id?: string) => invoke<number>('trash_purge', { id: id ?? null }),
  saveSection: (id: string, key: string, body: string) => invoke<void>('report_save_section', { id, key, body }),
  setSectionNotRelevant: (id: string, key: string, on: boolean) =>
    invoke<Report>('report_section_not_relevant', { id, key, on }),
  checklist: (id: string) => invoke<CheckItem[]>('report_checklist', { id }),
  sign: (id: string, signedBy: string) => invoke<Report>('report_sign', { id, signedBy }),
  addAddendum: (id: string, body: string, signedBy: string) =>
    invoke<Report>('report_add_addendum', { id, body, signedBy }),
  exportReport: (id: string, format: 'pdf' | 'docx', path: string) =>
    invoke<void>('report_export', { id, format, path }),
  /** The pages an export would write now, as PNG data URLs (`scale` pixels per point). */
  previewReport: (id: string, scale: number) => invoke<string[]>('report_preview', { id, scale }),

  chatThreads: (scope?: ChatScope, refId?: string) =>
    invoke<ChatThread[]>('chat_threads', { scope: scope ?? null, refId: refId ?? null }),
  createChatThread: (scope: ChatScope, refId: string | null) => invoke<ChatThread>('chat_thread_create', { scope, refId }),
  chat: (id: string) => invoke<ChatView>('chat_get', { id }),
  deleteChatThread: (id: string) => invoke<void>('chat_thread_delete', { id }),
  /** One question; resolves when the assistant answered, was cancelled (`llmCancel`) or gave up. */
  chatSend: (threadId: string, text: string, zone: Zone, requestId: string) =>
    invoke<ChatEnd>('chat_send', { threadId, text, zone, requestId }),
  applyProposal: (id: string) => invoke<AgentProposal>('proposal_apply', { id }),
  discardProposal: (id: string) => invoke<AgentProposal>('proposal_discard', { id }),
  agentToolsSupport: () => invoke<boolean | null>('agent_tools_support'),
  /** MP3 of an answer, read aloud with the saved read-aloud settings. */
  chatSpeak: (threadId: string, messageId: string) => invoke<ArrayBuffer>('chat_speak', { threadId, messageId }),
  onAgentText: (requestId: string, cb: (text: string) => void): Promise<UnlistenFn> =>
    listen<AgentText>('agent:text', (e) => {
      if (e.payload.request_id === requestId) cb(e.payload.text)
    }),
  onAgentChanged: (threadId: string, cb: () => void): Promise<UnlistenFn> =>
    listen<{ thread_id: string }>('agent:changed', (e) => {
      if (e.payload.thread_id === threadId) cb()
    }),

  audit: (limit = 200) => invoke<AuditEvent[]>('audit_list', { limit }),
  auditVerify: () => invoke<number | null>('audit_verify'),

  setting: <T = unknown>(key: string) => invoke<T | null>('setting_get', { key }),
  setSetting: (key: string, value: unknown) => invoke<void>('setting_set', { key, value }),
}

// Query keys/options in one place so invalidation stays consistent.
export const q = {
  patients: (query = '') => queryOptions({ queryKey: ['patients', query], queryFn: () => api.patients(query) }),
  patient: (id: string) => queryOptions({ queryKey: ['patient', id], queryFn: () => api.patient(id) }),
  // `now` is rounded to the minute by the caller so the key stays stable between renders.
  patientCards: (query: string, now: string) =>
    queryOptions({ queryKey: ['patients', 'cards', query, now], queryFn: () => api.patientCards(query, now) }),
  patientPhoto: (id: string) =>
    queryOptions({ queryKey: ['patient', id, 'photo'], queryFn: () => api.patientPhoto(id), staleTime: Infinity }),
  consentText: (kind: ConsentKind) =>
    queryOptions({ queryKey: ['consentText', kind], queryFn: () => api.consentText(kind), staleTime: Infinity }),
  consents: (patientId: string) =>
    queryOptions({ queryKey: ['consents', patientId], queryFn: () => api.consents(patientId) }),
  episodes: (patientId: string) =>
    queryOptions({ queryKey: ['episodes', patientId], queryFn: () => api.episodes(patientId) }),
  applications: (episodeId: string) =>
    queryOptions({ queryKey: ['applications', episodeId], queryFn: () => api.applications(episodeId) }),
  berichtCandidates: (episodeId?: string) =>
    queryOptions({ queryKey: ['berichtCandidates', episodeId ?? 'all'], queryFn: () => api.berichtCandidates(episodeId) }),
  sessions: (patientId?: string) =>
    queryOptions({ queryKey: ['sessions', patientId ?? 'all'], queryFn: () => api.sessions(patientId) }),
  // Polled so the colours follow a running recording or transcription elsewhere in the app.
  sessionQueue: () => queryOptions({ queryKey: ['sessions', 'queue'], queryFn: api.sessionQueue, refetchInterval: 15_000 }),
  calendar: (from: string, to: string) =>
    queryOptions({ queryKey: ['calendar', from, to], queryFn: () => api.calendar(from, to), refetchInterval: 15_000 }),
  dueItems: (from: string, dayStart: string, until: string) =>
    queryOptions({ queryKey: ['calendar', 'due', from, dayStart, until], queryFn: () => api.dueItems(from, dayStart, until) }),
  tasks: (from: string, to: string, openBefore: boolean) =>
    queryOptions({ queryKey: ['tasks', from, to, openBefore], queryFn: () => api.tasks(from, to, openBefore) }),
  dayNote: (date: string) => queryOptions({ queryKey: ['dayNote', date], queryFn: () => api.dayNote(date) }),
  // Polled while a transcription is queued or running (progress bar).
  session: (id: string) =>
    queryOptions({
      queryKey: ['session', id],
      queryFn: () => api.session(id),
      refetchInterval: (query) => (query.state.data?.sources.some((s) => jobActive(s.job)) ? 1000 : false),
    }),
  stt: () =>
    queryOptions({
      queryKey: ['stt'],
      queryFn: api.stt,
      refetchInterval: (query) =>
        query.state.data?.models.some((m) => m.transfer && !m.transfer.error) ? 500 : false,
    }),
  llm: () => queryOptions({ queryKey: ['llm'], queryFn: api.llm }),
  llmCatalog: () => queryOptions({ queryKey: ['llmCatalog'], queryFn: api.llmCatalog, staleTime: Infinity }),
  llmSpend: () => queryOptions({ queryKey: ['llm', 'spend'], queryFn: api.llmSpend }),
  llmCosts: (all: boolean) => queryOptions({ queryKey: ['llm', 'spend', 'costs', all], queryFn: () => api.llmCosts(all) }),
  /** Cached 5 min (Settings and the low-balance banner share it); the refresh button refetches. */
  llmBalance: (enabled: boolean) =>
    queryOptions({ queryKey: ['balance'], queryFn: api.llmBalance, enabled, staleTime: 5 * 60_000, retry: false }),
  tts: () => queryOptions({ queryKey: ['llm', 'tts'], queryFn: api.tts }),
  live: () => queryOptions({ queryKey: ['llm', 'live'], queryFn: api.live }),
  prep: (sessionId: string) => queryOptions({ queryKey: ['prep', sessionId], queryFn: () => api.prep(sessionId) }),
  noteFacts: (reportId: string) => queryOptions({ queryKey: ['noteFacts', reportId], queryFn: () => api.noteFacts(reportId) }),
  sessionSummary: (sessionId: string) =>
    queryOptions({ queryKey: ['sessionSummary', sessionId], queryFn: () => api.sessionSummary(sessionId) }),
  timeline: (patientId: string) => queryOptions({ queryKey: ['timeline', patientId], queryFn: () => api.patientTimeline(patientId) }),
  autopilot: () => queryOptions({ queryKey: ['setting', 'autopilot'], queryFn: api.autopilotEnabled }),
  // One check per start; Einstellungen refetches on demand.
  update: (enabled = true) =>
    queryOptions({ queryKey: ['update'], queryFn: api.updateCheck, enabled, staleTime: Infinity, retry: false }),
  // Polled while the run is queued or running (the backend also announces each step).
  autopilotRun: (sessionId: string) =>
    queryOptions({
      queryKey: ['autopilot', sessionId],
      queryFn: () => api.autopilotRun(sessionId),
      refetchInterval: (query) => (jobActive(query.state.data?.job) ? 1500 : false),
    }),
  contingent: (episodeId: string) => queryOptions({ queryKey: ['contingent', episodeId], queryFn: () => api.contingent(episodeId) }),
  factTagSections: (reportId: string) =>
    queryOptions({ queryKey: ['factTagSections', reportId], queryFn: () => api.factTagSections(reportId), staleTime: Infinity }),
  draftSources: (reportId: string) =>
    queryOptions({ queryKey: ['draftSources', reportId], queryFn: () => api.draftSources(reportId) }),
  reportQuestions: (reportId: string) =>
    queryOptions({ queryKey: ['reportQuestions', reportId], queryFn: () => api.reportQuestions(reportId) }),
  review: (reportId: string) => queryOptions({ queryKey: ['review', reportId], queryFn: () => api.review(reportId) }),
  reportDrafts: (reportId: string) =>
    queryOptions({ queryKey: ['reportDrafts', reportId], queryFn: () => api.reportDrafts(reportId) }),
  // Polled during a download/import and while a model loads (`busy`: a request may be loading one).
  local: (busy = false) =>
    queryOptions({
      queryKey: ['local'],
      queryFn: api.local,
      refetchInterval: (query) =>
        busy || query.state.data?.loading || query.state.data?.models.some((m) => m.transfer && !m.transfer.error)
          ? 500
          : false,
    }),
  audioDevices: () => queryOptions({ queryKey: ['audioDevices'], queryFn: api.audioDevices }),
  // Polled for the timer and level meter; cheap (no DB access).
  recording: () =>
    queryOptions({ queryKey: ['recording'], queryFn: api.recordingStatus, refetchInterval: 150 }),
  templates: () => queryOptions({ queryKey: ['templates'], queryFn: api.templates }),
  reports: (status?: ReportStatus) =>
    queryOptions({ queryKey: ['reports', status ?? 'all'], queryFn: () => api.reports(status) }),
  report: (id: string) => queryOptions({ queryKey: ['report', id], queryFn: () => api.report(id) }),
  checklist: (id: string) => queryOptions({ queryKey: ['checklist', id], queryFn: () => api.checklist(id) }),
  audit: () => queryOptions({ queryKey: ['audit'], queryFn: () => api.audit() }),
  chatThreads: (scope?: ChatScope, refId?: string) =>
    queryOptions({ queryKey: ['chat', 'threads', scope ?? 'all', refId ?? ''], queryFn: () => api.chatThreads(scope, refId) }),
  chat: (id: string) => queryOptions({ queryKey: ['chat', id], queryFn: () => api.chat(id) }),
  // Asks the provider's model list; no patient data.
  agentTools: () => queryOptions({ queryKey: ['llm', 'agentTools'], queryFn: api.agentToolsSupport, staleTime: 10 * 60_000, retry: false }),
  setting: <T,>(key: string) => queryOptions({ queryKey: ['setting', key], queryFn: () => api.setting<T>(key) }),
}

export function errorMessage(e: unknown): string {
  return typeof e === 'string' ? e : e instanceof Error ? e.message : JSON.stringify(e)
}
