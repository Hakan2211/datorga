// Typed wrappers over the Tauri commands in crates/app/src/lib.rs.
// Types come from ts-rs (`cargo test -p thera-core` regenerates src/bindings).
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { queryOptions } from '@tanstack/react-query'
import type { Application } from '@/bindings/Application'
import type { ApplicationKind } from '@/bindings/ApplicationKind'
import type { AuditEvent } from '@/bindings/AuditEvent'
import type { CheckItem } from '@/bindings/CheckItem'
import type { Consent } from '@/bindings/Consent'
import type { ConsentKind } from '@/bindings/ConsentKind'
import type { ConsentText } from '@/bindings/ConsentText'
import type { Episode } from '@/bindings/Episode'
import type { EpisodeInput } from '@/bindings/EpisodeInput'
import type { Job } from '@/bindings/Job'
import type { Patient } from '@/bindings/Patient'
import type { PatientInput } from '@/bindings/PatientInput'
import type { Report } from '@/bindings/Report'
import type { ReportStatus } from '@/bindings/ReportStatus'
import type { ReportSummary } from '@/bindings/ReportSummary'
import type { Session } from '@/bindings/Session'
import type { SessionInput } from '@/bindings/SessionInput'
import type { SessionSource } from '@/bindings/SessionSource'
import type { SourceKind } from '@/bindings/SourceKind'
import type { SourceSegment } from '@/bindings/SourceSegment'
import type { Template } from '@/bindings/Template'
import type { Transcript } from '@/bindings/Transcript'
import type { BackupSummary } from '@/bindings/BackupSummary'
import type { CloudModel } from '@/bindings/CloudModel'
import type { LlmConfig } from '@/bindings/LlmConfig'
import type { LlmSpend } from '@/bindings/LlmSpend'
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
import type { SectionDraft } from '@/bindings/SectionDraft'

export type InitResult = {
  status: 'ready' | 'recovery_required' | 'new'
  recovery_key: string | null
  recovery_confirmed: boolean
  encrypted: boolean
}

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

export type SourceView = SessionSource & { job: Job | null; transcript: Transcript | null }

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
}

export type LlmOverview = { config: LlmConfig; providers: (ProviderInfo & { has_key: boolean })[] }
export type LlmTestResult = { model: string; latency_ms: number; answer: string }
export type LlmCatalog = { models: CloudModel[]; recommended: Tier }
export type LlmSpendView = { month: LlmSpend; total: LlmSpend }
/** Redaction preview: the outgoing text, the local token table and where it would go. */
export type ExtractPreview = Preview & { terms: PseudoTerm[]; provider: ProviderInfo | null; model: string | null }
/** `chunks` < `total` when cancelled: facts from the finished chunks were kept. */
export type ExtractResult = { facts: NoteFact[]; dropped: number; chunks: number; total: number; cancelled: boolean }
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

  exportConsentForm: (patientId: string, kind: 'recording', path: string) =>
    invoke<void>('consent_form_export', { patientId, kind, path }),
  consentText: (kind: ConsentKind) => invoke<ConsentText>('consent_text', { kind }),
  consents: (patientId: string) => invoke<Consent[]>('consents_list', { patientId }),
  grantConsent: (patientId: string, kind: ConsentKind, note: string) =>
    invoke<Consent>('consent_grant', { patientId, kind, note: note || null }),
  revokeConsent: (id: string) => invoke<Consent>('consent_revoke', { id }),

  episodes: (patientId: string) => invoke<Episode[]>('episodes_list', { patientId }),
  createEpisode: (input: EpisodeInput) => invoke<Episode>('episode_create', { input }),
  applications: (episodeId: string) => invoke<Application[]>('applications_list', { episodeId }),
  createApplication: (episodeId: string, kind: ApplicationKind) =>
    invoke<Application>('application_create', { episodeId, kind }),

  sessions: (patientId?: string) => invoke<Session[]>('sessions_list', { patientId: patientId ?? null }),
  createSession: (input: SessionInput) => invoke<Session>('session_create', { input }),
  session: (id: string) => invoke<SessionView>('session_get', { id }),
  createSessionNote: (sessionId: string) => invoke<Report>('session_note_create', { sessionId }),

  audioDevices: () => invoke<InputDevice[]>('audio_devices'),
  startRecording: (sessionId: string, kind: SourceKind, deviceId: string | null) =>
    invoke<SessionSource>('recording_start', { sessionId, kind, deviceId }),
  recordingStatus: () => invoke<RecordingStatus | null>('recording_status'),
  stopRecording: () => invoke<SessionSource>('recording_stop'),

  startTranscription: (sourceId: string) => invoke<Job>('transcription_start', { sourceId }),
  cancelTranscription: (jobId: string) => invoke<void>('transcription_cancel', { jobId }),
  editSegment: (transcriptId: string, idx: number, text: string) =>
    invoke<void>('transcript_edit_segment', { transcriptId, idx, text }),
  confirmTranscript: (id: string) => invoke<Transcript>('transcript_confirm', { id }),

  stt: () => invoke<SttOverview>('stt_overview'),
  selectModel: (id: string) => invoke<void>('stt_model_select', { id }),
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

  extractPreview: (reportId: string) => invoke<ExtractPreview>('extract_preview', { reportId }),
  extractRun: (reportId: string, digest: string, requestId: string) =>
    invoke<ExtractResult>('extract_run', { reportId, digest, requestId }),
  addPseudoTerm: (patientId: string, kind: EntityKind, text: string) =>
    invoke<PseudoTerm>('pseudo_term_add', { patientId, kind, text }),
  removePseudoTerm: (id: string) => invoke<void>('pseudo_term_remove', { id }),
  noteFacts: (reportId: string) => invoke<NoteFact[]>('note_facts', { reportId }),
  rejectFacts: (reportId: string, ids: string[]) => invoke<NoteFact[]>('note_facts_reject', { reportId, ids }),
  applyFacts: (reportId: string, facts: FactEdit[]) => invoke<Report>('note_facts_apply', { reportId, facts }),
  draftSources: (reportId: string) => invoke<EpisodeFact[]>('draft_sources', { reportId }),
  draftPreview: (reportId: string, sections: string[], instruction: string | null) =>
    invoke<ExtractPreview>('draft_preview', { reportId, sections, instruction }),
  draftRun: (reportId: string, sections: string[], instruction: string | null, digest: string, requestId: string) =>
    invoke<DraftResult>('draft_run', { reportId, sections, instruction, digest, requestId }),
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

  local: () => invoke<LocalOverview>('local_overview'),
  downloadLocalModel: (id: string) => invoke<void>('local_model_download', { id }),
  importLocalModel: (id: string, path: string) => invoke<void>('local_model_import', { id, path }),
  deleteLocalModel: (id: string) => invoke<void>('local_model_delete', { id }),
  unloadLocal: () => invoke<void>('local_unload'),

  templates: () => invoke<Template[]>('templates_list'),

  reports: (status?: ReportStatus) => invoke<ReportSummary[]>('reports_list', { status: status ?? null }),
  report: (id: string) => invoke<ReportView>('report_get', { id }),
  saveSection: (id: string, key: string, body: string) => invoke<void>('report_save_section', { id, key, body }),
  checklist: (id: string) => invoke<CheckItem[]>('report_checklist', { id }),
  sign: (id: string, signedBy: string) => invoke<Report>('report_sign', { id, signedBy }),
  addAddendum: (id: string, body: string, signedBy: string) =>
    invoke<Report>('report_add_addendum', { id, body, signedBy }),
  exportReport: (id: string, format: 'pdf' | 'docx', path: string) =>
    invoke<void>('report_export', { id, format, path }),

  audit: (limit = 200) => invoke<AuditEvent[]>('audit_list', { limit }),
  auditVerify: () => invoke<number | null>('audit_verify'),

  setting: <T = unknown>(key: string) => invoke<T | null>('setting_get', { key }),
  setSetting: (key: string, value: unknown) => invoke<void>('setting_set', { key, value }),
}

// Query keys/options in one place so invalidation stays consistent.
export const q = {
  patients: (query = '') => queryOptions({ queryKey: ['patients', query], queryFn: () => api.patients(query) }),
  patient: (id: string) => queryOptions({ queryKey: ['patient', id], queryFn: () => api.patient(id) }),
  consentText: (kind: ConsentKind) =>
    queryOptions({ queryKey: ['consentText', kind], queryFn: () => api.consentText(kind), staleTime: Infinity }),
  consents: (patientId: string) =>
    queryOptions({ queryKey: ['consents', patientId], queryFn: () => api.consents(patientId) }),
  episodes: (patientId: string) =>
    queryOptions({ queryKey: ['episodes', patientId], queryFn: () => api.episodes(patientId) }),
  applications: (episodeId: string) =>
    queryOptions({ queryKey: ['applications', episodeId], queryFn: () => api.applications(episodeId) }),
  sessions: (patientId?: string) =>
    queryOptions({ queryKey: ['sessions', patientId ?? 'all'], queryFn: () => api.sessions(patientId) }),
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
  noteFacts: (reportId: string) => queryOptions({ queryKey: ['noteFacts', reportId], queryFn: () => api.noteFacts(reportId) }),
  draftSources: (reportId: string) =>
    queryOptions({ queryKey: ['draftSources', reportId], queryFn: () => api.draftSources(reportId) }),
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
  setting: <T,>(key: string) => queryOptions({ queryKey: ['setting', key], queryFn: () => api.setting<T>(key) }),
}

export function errorMessage(e: unknown): string {
  return typeof e === 'string' ? e : e instanceof Error ? e.message : JSON.stringify(e)
}
