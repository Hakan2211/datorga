import { useEffect, useRef, useState, type ReactNode } from 'react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  ClipboardList,
  Cloud,
  Download,
  FileSignature,
  FileText,
  Laptop,
  Mic,
  MicOff,
  NotebookPen,
  Square,
  Undo2,
  X,
  XCircle,
} from 'lucide-react'
import type { SourceKind } from '@/bindings/SourceKind'
import type { SttTarget } from '@/bindings/SttTarget'
import { Page, PageHeader } from '@/components/app-shell'
import {
  EmptyState,
  ExportOutdatedBadge,
  SectionCard,
  StatusBadge,
  StatusPill,
  Stepper,
  StickyActionBar,
  formatDate,
} from '@/components/bits'
import { useReportExport } from '@/components/report-export'
import { invalidateSessions } from '@/components/session-dialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { TranscriptView } from '@/components/transcript-view'
import { PrepCard } from '@/components/voice-prep'
import { SummaryCard } from '@/components/session-summary'
import { api, errorMessage, jobActive, q, type RecordingStatus, type SessionView, type SourceView } from '@/lib/api'
import { SESSION_FLOW, flowStates, type FlowStep } from '@/lib/calendar'
import { formatDuration, useCaptureDevice } from '@/lib/capture'
import { cn } from '@/lib/utils'

export const Route = createFileRoute('/sessions/$sessionId')({ component: SessionPage })

const FLOW = SESSION_FLOW

/** Where a session stands, from its recordings, transcripts and note. */
function sessionFlow(view: SessionView, recording: RecordingStatus | null | undefined, hasPrep: boolean) {
  const { session, sources } = view
  const finished = sources.filter((s) => s.status !== 'recording')
  const reviewed = (s: SourceView) => s.transcript?.status === 'confirmed' || !!s.audio_deleted_at
  const unreviewed = finished.filter((s) => !reviewed(s)).length
  const states = flowStates({
    hasPrep,
    sources: sources.length,
    finished: finished.length,
    unreviewed,
    hasNote: !!session.note_id,
    noteStatus: session.note_status,
    exportOutdated: session.note_export_outdated,
    recordingHere: recording?.session_id === session.id,
  })
  const current = FLOW.find((k) => states[k] === 'current')
  return { states, current, unreviewed, finished: finished.length }
}

/** A summary needs a confirmed transcript or a note to read. */
function summaryReady({ session, sources }: SessionView) {
  return !!session.note_id || sources.some((s) => s.transcript?.status === 'confirmed')
}

function SessionPage() {
  const { sessionId } = Route.useParams()
  const { t } = useTranslation()
  const view = useQuery(q.session(sessionId))
  const recording = useQuery(q.recording())
  const prep = useQuery(q.prep(sessionId))
  const [viewing, setViewing] = useState<FlowStep | null>(null)
  if (!view.data) return null
  const { session, patient } = view.data
  const flow = sessionFlow(view.data, recording.data, !!prep.data)
  // Without a choice the page follows the work; once everything is done it rests on "Abschließen".
  const shown = viewing ?? flow.current ?? 'finish'

  return (
    <div className="flex min-h-full flex-col">
      <PageHeader
        title={formatDate(session.scheduled_at ?? session.started_at, true)}
        subtitle={
          <span className="flex items-center gap-2">
            <Link to="/patients/$patientId" params={{ patientId: patient.id }} className="hover:underline">
              {patient.first_name} {patient.last_name}
            </Link>
            {patient.chiffre && <StatusPill className="font-mono">{patient.chiffre}</StatusPill>}
          </span>
        }
        actions={
          session.status !== 'cancelled' &&
          session.note_status !== 'signed' &&
          session.note_status !== 'exported' && (
            <Button asChild size="sm" variant="outline">
              <Link to="/sessions/$sessionId/werkstatt" params={{ sessionId: session.id }} data-testid="open-workshop">
                <Mic /> {t('workshop.open')}
              </Link>
            </Button>
          )
        }
      />
      <Page className="flex-1">
        {session.status === 'cancelled' && <CancelledBanner sessionId={session.id} reason={session.cancelled_reason} />}
        <Stepper
          steps={FLOW.map((k) => ({ key: k, label: t(`flow.session.${k}`), state: flow.states[k] }))}
          viewing={shown}
          onSelect={setViewing}
          className="mx-auto max-w-4xl"
        />
        <div data-testid={`step-${shown}`} className="space-y-6">
          {shown === 'prepare' && <PrepCard sessionId={session.id} primary={false} />}
          {shown === 'capture' && <CapturePanel view={view.data} primary={flow.finished === 0} />}
          {shown === 'review' && <ReviewStep view={view.data} onCapture={() => setViewing('capture')} />}
          {shown === 'note' && <NoteCard view={view.data} unreviewed={flow.unreviewed} />}
          {shown === 'finish' && <FinishStep view={view.data} />}
          {(shown === 'note' || shown === 'finish') && session.status !== 'cancelled' && (
            <SummaryCard sessionId={session.id} patientId={patient.id} ready={summaryReady(view.data)} />
          )}
        </div>
      </Page>
      <FlowBar shown={shown} flow={flow} recordingHere={recording.data?.session_id === session.id} onNext={setViewing} />
    </div>
  )
}

/** A cancelled session takes no recording or note until it is planned again. */
function CancelledBanner({ sessionId, reason }: { sessionId: string; reason: string | null }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const reopen = useMutation({
    mutationFn: () => api.reopenSession(sessionId),
    onSuccess: () => invalidateSessions(qc),
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <div className="mx-auto flex max-w-4xl items-center gap-3 rounded-lg border border-dashed px-4 py-3 text-sm" data-testid="cancelled-banner">
      <XCircle className="size-4 text-muted-foreground" />
      <span className="flex-1">{reason ? t('sessions.cancelledReason', { reason }) : t('sessions.cancelled')}</span>
      <Button size="sm" variant="outline" onClick={() => reopen.mutate()} disabled={reopen.isPending}>
        <Undo2 /> {t('calendar.reopen')}
      </Button>
    </div>
  )
}

/** The step's one primary action when it moves the flow on to the next step. */
function FlowBar({
  shown,
  flow,
  recordingHere,
  onNext,
}: {
  shown: FlowStep
  flow: ReturnType<typeof sessionFlow>
  recordingHere: boolean
  onNext: (step: FlowStep) => void
}) {
  const { t } = useTranslation()
  let info: ReactNode = null
  let action: ReactNode = null
  const go = (step: FlowStep, label: string, primary = true) => (
    <Button variant={primary ? 'default' : 'ghost'} onClick={() => onNext(step)} data-testid="flow-next">
      {label} <ArrowRight />
    </Button>
  )
  if (shown === 'prepare') {
    info = t('flow.session.prepareHint')
    action = go('capture', t('flow.session.toCapture'))
  } else if (shown === 'capture' && !recordingHere && flow.finished > 0) {
    action = go('review', t('flow.session.toReview'))
  } else if (shown === 'review') {
    if (flow.unreviewed > 0) {
      info = t('flow.session.unreviewed', { count: flow.unreviewed })
      action = go('note', t('flow.session.skipReview'), false)
    } else if (flow.finished > 0) {
      action = go('note', t('flow.session.toNote'))
    }
  }
  if (!info && !action) return null
  return <StickyActionBar info={info}>{action}</StickyActionBar>
}

function ReviewStep({ view, onCapture }: { view: SessionView; onCapture: () => void }) {
  const { t } = useTranslation()
  const { sources, patient } = view
  if (sources.length === 0) {
    return (
      <EmptyState
        icon={MicOff}
        title={t('flow.session.noSources')}
        description={t('flow.session.noSourcesHint')}
        action={
          <Button variant="outline" onClick={onCapture}>
            <Mic /> {t('flow.session.toCaptureShort')}
          </Button>
        }
      />
    )
  }
  return (
    <SectionCard icon={FileText} title={t('capture.sources')}>
      <ul className="divide-y text-sm">
        {sources.map((s) => (
          <SourceRow key={s.id} source={s} patientId={patient.id} />
        ))}
      </ul>
    </SectionCard>
  )
}

function NoteCard({ view, unreviewed }: { view: SessionView; unreviewed: number }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const { session } = view
  const create = useMutation({
    mutationFn: () => api.createSessionNote(session.id),
    onSuccess: (note) => {
      void qc.invalidateQueries({ queryKey: ['session', session.id] })
      void qc.invalidateQueries({ queryKey: ['sessions'] })
      void navigate({ to: '/reports/$reportId', params: { reportId: note.id } })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const signed = session.note_status === 'signed' || session.note_status === 'exported'

  return (
    <SectionCard
      icon={ClipboardList}
      title={
        <span className="flex items-center gap-3">
          {t('note.title')}
          {session.note_status && <StatusBadge status={session.note_status} />}
          <ExportOutdatedBadge show={session.note_export_outdated} />
        </span>
      }
      actions={
        session.note_id ? (
          <Button asChild size="sm" variant={signed ? 'outline' : 'default'}>
            <Link to="/reports/$reportId" params={{ reportId: session.note_id }}>
              <ClipboardList /> {t(signed ? 'note.open' : 'flow.session.writeNote')}
            </Link>
          </Button>
        ) : (
          <Button size="sm" onClick={() => create.mutate()} disabled={create.isPending}>
            <ClipboardList /> {t('note.create')}
          </Button>
        )
      }
      contentClassName="space-y-1 text-sm text-muted-foreground"
    >
      {!session.note_id && (
        <p>
          {t('note.none')} {t('note.prefillHint')}
        </p>
      )}
      {session.note_id && !signed && <p>{t('flow.session.noteDraftHint')}</p>}
      {signed && <p>{t('flow.session.noteSignedHint')}</p>}
      {unreviewed > 0 && <p className="text-warning-foreground">{t('note.unconfirmed')}</p>}
    </SectionCard>
  )
}

function FinishStep({ view }: { view: SessionView }) {
  const { t } = useTranslation()
  const { session } = view
  const note = useQuery({ ...q.report(session.note_id ?? ''), enabled: !!session.note_id })
  const exportAs = useReportExport(note.data)
  const status = session.note_status
  if (!session.note_id || !status) {
    return <EmptyState icon={ClipboardList} title={t('flow.session.finishNoNote')} description={t('flow.session.finishNoNoteHint')} />
  }
  if (status === 'draft' || status === 'in_review') {
    return (
      <EmptyState
        icon={FileSignature}
        title={t('flow.session.finishUnsigned')}
        description={t('flow.session.finishUnsignedHint')}
        action={
          <Button asChild>
            <Link to="/reports/$reportId" params={{ reportId: session.note_id }}>
              <FileSignature /> {t('flow.session.signNote')}
            </Link>
          </Button>
        }
      />
    )
  }
  const done = status === 'exported' && !session.note_export_outdated
  return (
    <SectionCard
      icon={done ? CheckCircle2 : Download}
      title={done ? t('flow.session.done') : t('flow.session.exportNote')}
      contentClassName="space-y-4 text-sm"
    >
      {session.note_export_outdated && (
        <p className="flex items-start gap-2 text-warning-foreground">
          <AlertTriangle className="mt-0.5 size-4 shrink-0" />
          {t('note.exportOutdatedSince', { date: formatDate(note.data?.report.exported_at, true) })}
        </p>
      )}
      <p className="text-muted-foreground">{done ? t('flow.session.doneHint') : t('flow.session.exportHint')}</p>
      <div className="flex gap-2">
        <Button variant={done ? 'outline' : 'default'} onClick={() => exportAs.mutate('pdf')} disabled={exportAs.isPending || !note.data}>
          <Download /> {t('report.exportPdf')}
        </Button>
        <Button variant="outline" onClick={() => exportAs.mutate('docx')} disabled={exportAs.isPending || !note.data}>
          <Download /> {t('report.exportDocx')}
        </Button>
      </div>
    </SectionCard>
  )
}

function CapturePanel({ view, primary }: { view: SessionView; primary: boolean }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const { session, patient } = view
  const status = useQuery(q.recording())
  const { devices, deviceId, setDeviceId, startId } = useCaptureDevice()

  const refresh = () => {
    void qc.invalidateQueries({ queryKey: ['recording'] })
    void qc.invalidateQueries({ queryKey: ['session', session.id] })
    void qc.invalidateQueries({ queryKey: ['sessions'] })
  }
  const start = useMutation({
    mutationFn: (kind: SourceKind) => api.startRecording(session.id, kind, startId),
    onSuccess: refresh,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const stop = useMutation({
    mutationFn: api.stopRecording,
    onSuccess: refresh,
    onError: (e) => toast.error(errorMessage(e)),
  })

  const active = status.data
  // A recording can also end outside this page (consent revoked, app exit): refresh then.
  const lastSource = useRef<string | null>(null)
  useEffect(() => {
    const current = active?.source_id ?? null
    if (lastSource.current && lastSource.current !== current) {
      void qc.invalidateQueries({ queryKey: ['session', session.id] })
    }
    lastSource.current = current
  }, [active?.source_id, qc, session.id])

  if (active && active.session_id !== session.id) {
    return (
      <EmptyState
        icon={Mic}
        title={t('capture.elsewhere')}
        action={
          <Button asChild size="sm" variant="outline">
            <Link to="/sessions/$sessionId" params={{ sessionId: active.session_id }}>
              {t('capture.goThere')}
            </Link>
          </Button>
        }
      />
    )
  }

  // Without recording consent, dictating is the way forward.
  const consent = patient.consent_recording
  return (
    <SectionCard
      icon={Mic}
      title={t('capture.title')}
      actions={
        <StatusPill tone={consent ? 'primary' : 'sand'}>
          {t(consent ? 'flow.session.consentOk' : 'flow.session.consentMissing')}
        </StatusPill>
      }
      contentClassName="space-y-4"
    >
      {active ? (
        <RecordingControls status={active} onStop={() => stop.mutate()} stopping={stop.isPending} />
      ) : (
        <>
          <div className="flex flex-wrap items-end gap-3">
            <Button
              variant={primary && consent ? 'default' : 'outline'}
              onClick={() => start.mutate('session_audio')}
              disabled={!consent || start.isPending}
            >
              <Mic /> {t('capture.record')}
            </Button>
            <Button
              variant={primary && !consent ? 'default' : 'outline'}
              onClick={() => start.mutate('dictation')}
              disabled={start.isPending}
            >
              <NotebookPen /> {t('capture.dictate')}
            </Button>
            <div className="ml-auto w-72 space-y-1.5">
              <Label>{t('capture.device')}</Label>
              <Select value={deviceId} onValueChange={setDeviceId}>
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="default">{t('capture.defaultDevice')}</SelectItem>
                  {devices.map((d) => (
                    <SelectItem key={d.id} value={d.id}>
                      {d.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          {!consent && (
            <p className="text-sm text-muted-foreground">
              {t('capture.noConsent')}{' '}
              <Link to="/patients/$patientId" params={{ patientId: patient.id }} className="underline">
                {t('capture.consentLink')}
              </Link>
            </p>
          )}
        </>
      )}
      <p className="text-xs text-muted-foreground">{t('capture.encryptedNote')}</p>
    </SectionCard>
  )
}

function RecordingControls({ status, onStop, stopping }: { status: RecordingStatus; onStop: () => void; stopping: boolean }) {
  const { t } = useTranslation()
  // Hint on digital silence only (muted/unplugged mic, Windows privacy setting); room noise is above this.
  const silent = status.duration_ms > 3000 && status.level < 0.0001
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-4">
        <span className="flex items-center gap-2 text-sm font-medium text-destructive">
          <span className="size-2.5 animate-pulse rounded-full bg-destructive" />
          {t(`capture.kind.${status.kind}`)}
        </span>
        <span className="font-mono text-2xl tabular-nums">{formatDuration(status.duration_ms)}</span>
        <LevelMeter level={status.level} />
        <Button variant="destructive" onClick={onStop} disabled={stopping} className="ml-auto">
          <Square /> {t('capture.stop')}
        </Button>
      </div>
      {status.error && (
        <p className="flex items-center gap-2 text-sm text-destructive">
          <AlertTriangle className="size-4" /> {status.error}
        </p>
      )}
      {silent && !status.error && <p className="text-sm text-muted-foreground">{t('capture.silent')}</p>}
      {status.minutes_left !== null && (
        <p className="flex items-center gap-2 text-sm text-warning-foreground">
          <AlertTriangle className="size-4" /> {status.minutes_left === 0 ? t('capture.lowDiskNow') : t('capture.lowDisk', { minutes: status.minutes_left })}
        </p>
      )}
    </div>
  )
}

function LevelMeter({ level }: { level: number }) {
  // dBFS mapped to 0..1 over -60..0 dB, which tracks speech better than linear peak.
  const db = 20 * Math.log10(Math.max(level, 1e-6))
  const pct = Math.min(100, Math.max(0, ((db + 60) / 60) * 100))
  return (
    <div className="h-2.5 w-48 overflow-hidden rounded-full bg-muted" aria-label="Pegel">
      <div
        className={cn('h-full transition-[width] duration-100', pct > 95 ? 'bg-destructive' : 'bg-success')}
        style={{ width: `${pct}%` }}
      />
    </div>
  )
}

const TARGETS: SttTarget[] = ['local', 'cloud']

function SourceRow({ source, patientId }: { source: SourceView; patientId: string }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const { job, transcript, stt } = source
  // Preselected from Settings; the therapist can switch per recording.
  const [picked, setPicked] = useState<SttTarget | null>(null)
  const target = picked ?? stt.default
  const problem = target === 'local' ? stt.local_problem : stt.cloud_problem
  const running = job?.engine ?? stt.default
  const cloudDone = transcript?.engine.startsWith('cloud:')
  // Drafts open by default: they need review. Confirmed ones stay collapsed.
  const [open, setOpen] = useState<boolean | null>(null)
  const expanded = open ?? transcript?.status === 'draft'
  const refresh = () => void qc.invalidateQueries({ queryKey: ['session', source.session_id] })
  const start = useMutation({
    mutationFn: () => api.startTranscription(source.id, target),
    onSuccess: refresh,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const cancel = useMutation({
    mutationFn: (jobId: string) => api.cancelTranscription(jobId),
    onSuccess: refresh,
    onError: (e) => toast.error(errorMessage(e)),
  })

  const reason = source.end_reason ? t(`capture.reason.${source.end_reason}`) : ''
  const finished = source.status !== 'recording'
  const canTranscribe = finished && !source.audio_deleted_at && transcript?.status !== 'confirmed' && !jobActive(job)
  return (
    <li className="space-y-2 py-2">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <span className="font-medium">{t(`capture.kind.${source.kind}`)}</span>
        <span className="text-muted-foreground">{formatDate(source.started_at, true)}</span>
        {source.duration_ms != null && <span className="font-mono tabular-nums">{formatDuration(source.duration_ms)}</span>}
        <Badge variant="secondary" className={cn('font-normal', source.status === 'interrupted' && 'bg-warning/20')}>
          {t(`capture.sourceStatus.${source.status}`)}
        </Badge>
        {transcript && (
          <Badge
            variant="secondary"
            className={cn('font-normal', transcript.status === 'confirmed' ? 'bg-success/15 text-success' : 'bg-warning/20')}
          >
            {t(`transcript.${transcript.status}`)}
          </Badge>
        )}
        {transcript && (
          <Badge variant="outline" className="gap-1 font-normal" data-testid="stt-engine-used">
            {cloudDone ? <Cloud /> : <Laptop />} {cloudDone ? t('transcript.doneCloud') : t('transcript.doneLocal')}
          </Badge>
        )}
        {source.audio_deleted_at && <span className="text-muted-foreground">{t('transcript.audioDeleted')}</span>}
        {reason && <span className="text-muted-foreground">{reason}</span>}
        <span className="ml-auto flex items-center gap-2">
          {transcript && (
            <Button size="sm" variant="ghost" onClick={() => setOpen(!expanded)}>
              <FileText /> {expanded ? t('transcript.hide') : transcript.status === 'draft' ? t('transcript.review') : t('transcript.show')}
            </Button>
          )}
          {canTranscribe && (
            <>
              <span
                className="inline-flex rounded-md border p-0.5"
                role="radiogroup"
                aria-label={t('transcript.targetLabel')}
                data-testid="stt-target"
              >
                {TARGETS.map((tg) => (
                  <button
                    key={tg}
                    type="button"
                    role="radio"
                    aria-checked={target === tg}
                    onClick={() => setPicked(tg)}
                    title={(tg === 'local' ? stt.local_problem : stt.cloud_problem) ?? undefined}
                    className={cn(
                      'inline-flex items-center gap-1 rounded px-2 py-0.5 text-xs',
                      target === tg ? 'bg-primary text-primary-foreground' : 'text-muted-foreground hover:bg-muted',
                    )}
                  >
                    {tg === 'local' ? <Laptop className="size-3.5" /> : <Cloud className="size-3.5" />} {t(`transcript.target.${tg}`)}
                  </button>
                ))}
              </span>
              <Button size="sm" onClick={() => start.mutate()} disabled={start.isPending || !!problem}>
                {transcript || job ? t('transcript.retry') : t('transcript.start')}
              </Button>
            </>
          )}
        </span>
      </div>
      {jobActive(job) && job && (
        <div className="flex items-center gap-3">
          <span className="text-muted-foreground">
            {job.kind === 'diarize'
              ? job.status === 'queued'
                ? t('transcript.speakers.queued')
                : `${t('transcript.speakers.running')} · ${job.progress} %`
              : job.status === 'queued'
                ? t(running === 'cloud' ? 'transcript.queuedCloud' : 'transcript.queuedLocal')
                : `${running === 'cloud' ? t('transcript.runningCloud', { label: stt.cloud_label }) : t('transcript.runningLocal')} · ${job.progress} %`}
          </span>
          <div className="h-2 w-48 overflow-hidden rounded-full bg-muted">
            <div className="h-full bg-primary transition-[width]" style={{ width: `${job.progress}%` }} />
          </div>
          <Button size="sm" variant="ghost" onClick={() => cancel.mutate(job.id)} disabled={cancel.isPending}>
            <X /> {t('transcript.cancel')}
          </Button>
        </div>
      )}
      {job?.status === 'failed' && (!transcript || job.kind === 'diarize') && (
        <p className="flex items-center gap-2 text-destructive">
          <AlertTriangle className="size-4" /> {t(job.kind === 'diarize' ? 'transcript.speakers.failed' : 'transcript.failed')}: {job.last_error}
        </p>
      )}
      {job?.status === 'cancelled' && !transcript && <p className="text-muted-foreground">{t('transcript.cancelled')}</p>}
      {canTranscribe && problem && (
        <p className="flex items-center gap-2 text-muted-foreground" data-testid="stt-problem">
          <AlertTriangle className="size-4 shrink-0" /> {problem}{' '}
          {target === 'cloud' && stt.cloud_needs_consent ? (
            <Link to="/patients/$patientId" params={{ patientId }} className="underline">
              {t('transcript.consentLink')}
            </Link>
          ) : (
            <Link to="/settings" className="underline">
              {t('transcript.settingsLink')}
            </Link>
          )}
        </p>
      )}
      {canTranscribe && !problem && target === 'cloud' && (
        <p className="flex items-center gap-2 text-[13px] text-muted-foreground">
          <Cloud className="size-4 shrink-0" /> {t('transcript.cloudHint', { label: stt.cloud_label })}
        </p>
      )}
      {transcript && expanded && (
        <TranscriptView
          transcript={transcript}
          sessionId={source.session_id}
          playable={!source.audio_deleted_at}
          separation={source.kind === 'session_audio' && !source.audio_deleted_at ? { speakers: source.speakers } : undefined}
          busy={jobActive(job)}
        />
      )}
    </li>
  )
}
