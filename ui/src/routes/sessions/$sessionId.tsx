import { useEffect, useRef, useState } from 'react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { AlertTriangle, ClipboardList, FileText, Mic, NotebookPen, Square, X } from 'lucide-react'
import type { SourceKind } from '@/bindings/SourceKind'
import { Page, PageHeader } from '@/components/app-shell'
import { StatusBadge, formatDate } from '@/components/bits'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { TranscriptView } from '@/components/transcript-view'
import { api, errorMessage, jobActive, q, type RecordingStatus, type SessionView, type SourceView, type SttOverview } from '@/lib/api'
import { cn } from '@/lib/utils'

export const Route = createFileRoute('/sessions/$sessionId')({ component: SessionPage })

function SessionPage() {
  const { sessionId } = Route.useParams()
  const { t } = useTranslation()
  const view = useQuery(q.session(sessionId))
  const stt = useQuery(q.stt())
  if (!view.data) return null
  const { session, patient, sources } = view.data

  return (
    <>
      <PageHeader
        title={formatDate(session.scheduled_at ?? session.started_at, true)}
        subtitle={
          <Link to="/patients/$patientId" params={{ patientId: patient.id }} className="hover:underline">
            {patient.first_name} {patient.last_name}
          </Link>
        }
      />
      <Page>
        <NoteCard view={view.data} />
        <CapturePanel view={view.data} />
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('capture.sources')}</CardTitle>
          </CardHeader>
          <CardContent>
            {sources.length === 0 ? (
              <p className="text-sm text-muted-foreground">{t('capture.none')}</p>
            ) : (
              <ul className="divide-y text-sm">
                {sources.map((s) => (
                  <SourceRow key={s.id} source={s} sttReady={sttReady(stt.data)} />
                ))}
              </ul>
            )}
          </CardContent>
        </Card>
      </Page>
    </>
  )
}

function NoteCard({ view }: { view: SessionView }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const { session, sources } = view
  const create = useMutation({
    mutationFn: () => api.createSessionNote(session.id),
    onSuccess: (note) => {
      void qc.invalidateQueries({ queryKey: ['session', session.id] })
      void qc.invalidateQueries({ queryKey: ['sessions'] })
      void navigate({ to: '/reports/$reportId', params: { reportId: note.id } })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const unconfirmed = sources.some((s) => s.transcript?.status === 'draft' || jobActive(s.job))

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0">
        <CardTitle className="flex items-center gap-3 text-base">
          {t('note.title')}
          {session.note_status && <StatusBadge status={session.note_status} />}
        </CardTitle>
        {session.note_id ? (
          <Button asChild size="sm">
            <Link to="/reports/$reportId" params={{ reportId: session.note_id }}>
              <ClipboardList /> {t('note.open')}
            </Link>
          </Button>
        ) : (
          <Button size="sm" onClick={() => create.mutate()} disabled={create.isPending}>
            <ClipboardList /> {t('note.create')}
          </Button>
        )}
      </CardHeader>
      {(!session.note_id || unconfirmed) && (
        <CardContent className="space-y-1 text-sm text-muted-foreground">
          {!session.note_id && <p>{t('note.none')} {t('note.prefillHint')}</p>}
          {unconfirmed && <p>{t('note.unconfirmed')}</p>}
        </CardContent>
      )}
    </Card>
  )
}

function CapturePanel({ view }: { view: SessionView }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const { session, patient } = view
  const status = useQuery(q.recording())
  const devices = useQuery(q.audioDevices())
  // Remembered across sessions; a device that disappeared falls back to the system default.
  const savedDevice = useQuery(q.setting<string>('audio_device'))
  const [picked, setPicked] = useState<string | null>(null)
  const preferred = picked ?? savedDevice.data ?? 'default'
  const deviceId = devices.data?.some((d) => d.id === preferred) ? preferred : 'default'
  const setDeviceId = (id: string) => {
    setPicked(id)
    void api.setSetting('audio_device', id).then(() => qc.invalidateQueries({ queryKey: ['setting', 'audio_device'] }))
  }

  const refresh = () => {
    void qc.invalidateQueries({ queryKey: ['recording'] })
    void qc.invalidateQueries({ queryKey: ['session', session.id] })
    void qc.invalidateQueries({ queryKey: ['sessions'] })
  }
  const start = useMutation({
    mutationFn: (kind: SourceKind) => api.startRecording(session.id, kind, deviceId === 'default' ? null : deviceId),
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
      <Card>
        <CardContent className="flex items-center justify-between pt-6 text-sm">
          {t('capture.elsewhere')}
          <Button asChild size="sm" variant="outline">
            <Link to="/sessions/$sessionId" params={{ sessionId: active.session_id }}>
              {t('capture.goThere')}
            </Link>
          </Button>
        </CardContent>
      </Card>
    )
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t('capture.title')}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        {active ? (
          <RecordingControls status={active} onStop={() => stop.mutate()} stopping={stop.isPending} />
        ) : (
          <>
            <div className="flex flex-wrap items-end gap-3">
              <Button
                onClick={() => start.mutate('session_audio')}
                disabled={!patient.consent_recording || start.isPending}
              >
                <Mic /> {t('capture.record')}
              </Button>
              <Button variant="outline" onClick={() => start.mutate('dictation')} disabled={start.isPending}>
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
                    {devices.data?.map((d) => (
                      <SelectItem key={d.id} value={d.id}>
                        {d.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>
            {!patient.consent_recording && (
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
      </CardContent>
    </Card>
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
        <p className="flex items-center gap-2 text-sm text-warning">
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

const sttReady = (o: SttOverview | undefined) => !!o?.engine && !!o.models.find((m) => m.id === o.active)?.installed

function SourceRow({ source, sttReady }: { source: SourceView; sttReady: boolean }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const { job, transcript } = source
  // Drafts open by default: they need review. Confirmed ones stay collapsed.
  const [open, setOpen] = useState<boolean | null>(null)
  const expanded = open ?? transcript?.status === 'draft'
  const refresh = () => void qc.invalidateQueries({ queryKey: ['session', source.session_id] })
  const start = useMutation({
    mutationFn: () => api.startTranscription(source.id),
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
        {source.audio_deleted_at && <span className="text-muted-foreground">{t('transcript.audioDeleted')}</span>}
        {reason && <span className="text-muted-foreground">{reason}</span>}
        <span className="ml-auto flex items-center gap-2">
          {transcript && (
            <Button size="sm" variant="ghost" onClick={() => setOpen(!expanded)}>
              <FileText /> {expanded ? t('transcript.hide') : transcript.status === 'draft' ? t('transcript.review') : t('transcript.show')}
            </Button>
          )}
          {canTranscribe && sttReady && (
            <Button size="sm" variant="outline" onClick={() => start.mutate()} disabled={start.isPending}>
              {transcript || job ? t('transcript.retry') : t('transcript.start')}
            </Button>
          )}
        </span>
      </div>
      {jobActive(job) && job && (
        <div className="flex items-center gap-3">
          <span className="text-muted-foreground">
            {job.status === 'queued' ? t('transcript.queued') : `${t('transcript.running')} · ${job.progress} %`}
          </span>
          <div className="h-2 w-48 overflow-hidden rounded-full bg-muted">
            <div className="h-full bg-primary transition-[width]" style={{ width: `${job.progress}%` }} />
          </div>
          <Button size="sm" variant="ghost" onClick={() => cancel.mutate(job.id)} disabled={cancel.isPending}>
            <X /> {t('transcript.cancel')}
          </Button>
        </div>
      )}
      {job?.status === 'failed' && !transcript && (
        <p className="flex items-center gap-2 text-destructive">
          <AlertTriangle className="size-4" /> {t('transcript.failed')}: {job.last_error}
        </p>
      )}
      {job?.status === 'cancelled' && !transcript && <p className="text-muted-foreground">{t('transcript.cancelled')}</p>}
      {canTranscribe && !sttReady && !transcript && (
        <p className="text-muted-foreground">
          {t('transcript.noModel')}{' '}
          <Link to="/settings" className="underline">
            {t('transcript.settingsLink')}
          </Link>
        </p>
      )}
      {transcript && expanded && <TranscriptView transcript={transcript} sessionId={source.session_id} />}
    </li>
  )
}

function formatDuration(ms: number): string {
  const s = Math.floor(ms / 1000)
  const h = Math.floor(s / 3600)
  const mm = String(Math.floor((s % 3600) / 60)).padStart(2, '0')
  const ss = String(s % 60).padStart(2, '0')
  return h ? `${h}:${mm}:${ss}` : `${mm}:${ss}`
}
