// Typed wrappers over the Tauri commands in crates/app/src/lib.rs.
// Types come from ts-rs (`cargo test -p thera-core` regenerates src/bindings).
import { invoke } from '@tauri-apps/api/core'
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
import type { Patient } from '@/bindings/Patient'
import type { PatientInput } from '@/bindings/PatientInput'
import type { Report } from '@/bindings/Report'
import type { ReportStatus } from '@/bindings/ReportStatus'
import type { ReportSummary } from '@/bindings/ReportSummary'
import type { Session } from '@/bindings/Session'
import type { SessionInput } from '@/bindings/SessionInput'
import type { SessionSource } from '@/bindings/SessionSource'
import type { SourceKind } from '@/bindings/SourceKind'
import type { Template } from '@/bindings/Template'

export type InitResult = {
  status: 'ready' | 'recovery_required'
  recovery_key: string | null
  encrypted: boolean
}

export type ReportView = {
  report: Report
  template: Template
  patient: Patient
  episode: Episode | null
  application: Application | null
}

export type SessionView = {
  session: Session
  patient: Patient
  sources: SessionSource[]
}

export type InputDevice = { id: string; name: string; is_default: boolean }

export type RecordingStatus = {
  source_id: string
  session_id: string
  kind: SourceKind
  duration_ms: number
  level: number
  error: string | null
}

export const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export const api = {
  init: () => invoke<InitResult>('app_init'),
  recover: (recoveryKey: string) => invoke<void>('app_recover', { recoveryKey }),

  patients: (query?: string) => invoke<Patient[]>('patients_list', { query: query || null }),
  patient: (id: string) => invoke<Patient>('patient_get', { id }),
  createPatient: (input: PatientInput) => invoke<Patient>('patient_create', { input }),
  updatePatient: (id: string, input: PatientInput) => invoke<Patient>('patient_update', { id, input }),
  erasePatient: (id: string) => invoke<void>('patient_erase', { id }),

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

  audioDevices: () => invoke<InputDevice[]>('audio_devices'),
  startRecording: (sessionId: string, kind: SourceKind, deviceId: string | null) =>
    invoke<SessionSource>('recording_start', { sessionId, kind, deviceId }),
  recordingStatus: () => invoke<RecordingStatus | null>('recording_status'),
  stopRecording: () => invoke<SessionSource>('recording_stop'),

  templates: () => invoke<Template[]>('templates_list'),

  reports: (status?: ReportStatus) => invoke<ReportSummary[]>('reports_list', { status: status ?? null }),
  report: (id: string) => invoke<ReportView>('report_get', { id }),
  saveSection: (id: string, key: string, body: string) => invoke<void>('report_save_section', { id, key, body }),
  checklist: (id: string) => invoke<CheckItem[]>('report_checklist', { id }),
  sign: (id: string, signedBy: string) => invoke<Report>('report_sign', { id, signedBy }),
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
  session: (id: string) => queryOptions({ queryKey: ['session', id], queryFn: () => api.session(id) }),
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
