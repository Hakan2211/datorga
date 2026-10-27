import { useEffect, useRef, useState } from 'react'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { AlertTriangle, Mic, NotebookPen, Square } from 'lucide-react'
import type { SessionSource } from '@/bindings/SessionSource'
import type { SourceKind } from '@/bindings/SourceKind'
import { Page, PageHeader } from '@/components/app-shell'
import { formatDate } from '@/components/bits'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { api, errorMessage, q, type RecordingStatus, type SessionView } from '@/lib/api'
import { cn } from '@/lib/utils'

export const Route = createFileRoute('/sessions/$sessionId')({ component: SessionPage })

function SessionPage() {
  const { sessionId } = Route.useParams()
  const { t } = useTranslation()
  const view = useQuery(q.session(sessionId))
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
                  <SourceRow key={s.id} source={s} />
                ))}
              </ul>
            )}
          </CardContent>
        </Card>
      </Page>
    </>
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

function SourceRow({ source }: { source: SessionSource }) {
  const { t } = useTranslation()
  const reason = source.end_reason ? t(`capture.reason.${source.end_reason}`) : ''
  return (
    <li className="flex flex-wrap items-center gap-x-3 py-2">
      <span className="font-medium">{t(`capture.kind.${source.kind}`)}</span>
      <span className="text-muted-foreground">{formatDate(source.started_at, true)}</span>
      {source.duration_ms != null && <span className="font-mono tabular-nums">{formatDuration(source.duration_ms)}</span>}
      <Badge variant="secondary" className={cn('font-normal', source.status === 'interrupted' && 'bg-warning/20')}>
        {t(`capture.sourceStatus.${source.status}`)}
      </Badge>
      {reason && <span className="text-muted-foreground">{reason}</span>}
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
