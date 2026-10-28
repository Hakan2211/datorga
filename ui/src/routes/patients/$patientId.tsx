import { useState } from 'react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { FilePlus2, Pencil, Plus, Trash2 } from 'lucide-react'
import type { AgeGroup } from '@/bindings/AgeGroup'
import type { ApplicationKind } from '@/bindings/ApplicationKind'
import type { Episode } from '@/bindings/Episode'
import type { Method } from '@/bindings/Method'
import type { Setting } from '@/bindings/Setting'
import { Page, PageHeader } from '@/components/app-shell'
import { ExportOutdatedBadge, StatusBadge, formatDate } from '@/components/bits'
import { ConsentCard } from '@/components/consent-card'
import { PatientFormDialog } from '@/components/patient-form'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { api, errorMessage, q } from '@/lib/api'

export const Route = createFileRoute('/patients/$patientId')({ component: PatientDetail })

function PatientDetail() {
  const { patientId } = Route.useParams()
  const { t } = useTranslation()
  const navigate = useNavigate()
  const qc = useQueryClient()
  const patient = useQuery(q.patient(patientId))
  const episodes = useQuery(q.episodes(patientId))

  const erase = useMutation({
    mutationFn: () => api.erasePatient(patientId),
    onSuccess: () => {
      void qc.invalidateQueries()
      void navigate({ to: '/patients' })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  if (!patient.data) return null
  const p = patient.data

  return (
    <>
      <PageHeader
        title={`${p.first_name} ${p.last_name}`}
        subtitle={
          <span className="flex items-center gap-3">
            {formatDate(p.birth_date)}
            <Badge variant="outline" className="font-mono">
              {p.chiffre ?? t('patients.noChiffre')}
            </Badge>
          </span>
        }
        actions={
          <>
            <PatientFormDialog
              patient={p}
              trigger={
                <Button variant="outline">
                  <Pencil /> {t('common.edit')}
                </Button>
              }
            />
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button variant="ghost" className="text-destructive">
                  <Trash2 /> {t('patients.erase')}
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>{t('patients.erase')}</AlertDialogTitle>
                  <AlertDialogDescription>{t('patients.eraseConfirm')}</AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
                  <AlertDialogAction className="bg-destructive hover:bg-destructive/90" onClick={() => erase.mutate()}>
                    {t('common.delete')}
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          </>
        }
      />
      <Page>
        <ConsentCard patientId={patientId} kind="recording" />
        <ConsentCard patientId={patientId} kind="cloud_transcription" />
        <div className="flex items-center justify-between">
          <h2 className="text-base font-semibold">{t('episodes.title')}</h2>
          <NewEpisodeDialog patientId={patientId} />
        </div>
        {episodes.data?.length === 0 && <p className="text-sm text-muted-foreground">{t('episodes.none')}</p>}
        {episodes.data?.map((e) => <EpisodeCard key={e.id} episode={e} />)}
        <SessionsCard patientId={patientId} episodeId={episodes.data?.[0]?.id} />
      </Page>
    </>
  )
}

function EpisodeCard({ episode }: { episode: Episode }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const apps = useQuery(q.applications(episode.id))
  const create = useMutation({
    mutationFn: (kind: ApplicationKind) => api.createApplication(episode.id, kind),
    onSuccess: (a) => {
      void qc.invalidateQueries({ queryKey: ['applications', episode.id] })
      void qc.invalidateQueries({ queryKey: ['reports'] })
      if (a.report_id) void navigate({ to: '/reports/$reportId', params: { reportId: a.report_id } })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0">
        <CardTitle className="text-base">
          {t(`enums.method.${episode.method}`)}
          <span className="ml-3 text-sm font-normal text-muted-foreground">
            {t(`enums.age_group.${episode.age_group}`)} · {t(`enums.setting.${episode.setting}`)} ·{' '}
            {t('episodes.since')} {formatDate(episode.started_at)}
          </span>
        </CardTitle>
        <div className="flex gap-2">
          {(['erstantrag', 'umwandlung', 'fortfuehrung'] as const).map((k) => (
            <Button key={k} size="sm" variant="outline" disabled={create.isPending} onClick={() => create.mutate(k)}>
              <FilePlus2 /> {t(`enums.kind.${k}`)}
            </Button>
          ))}
        </div>
      </CardHeader>
      <CardContent>
        {apps.data?.length === 0 ? (
          <p className="text-sm text-muted-foreground">{t('common.empty')}</p>
        ) : (
          <ul className="divide-y">
            {apps.data?.map((a) => (
              <li key={a.id} className="flex items-center justify-between py-2 text-sm">
                <span>
                  {t(`enums.kind.${a.kind}`)}
                  {a.kind === 'fortfuehrung' ? ` Nr. ${a.seq}` : ''}
                  <span className="ml-3 text-muted-foreground">{formatDate(a.created_at)}</span>
                </span>
                {a.report_id && (
                  <Button asChild size="sm" variant="ghost">
                    <Link to="/reports/$reportId" params={{ reportId: a.report_id }}>
                      {t('common.open')}
                    </Link>
                  </Button>
                )}
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  )
}

function NewEpisodeDialog({ patientId }: { patientId: string }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [open, setOpen] = useState(false)
  const [method, setMethod] = useState<Method>('VT')
  const [ageGroup, setAgeGroup] = useState<AgeGroup>('adult')
  const [setting, setSetting] = useState<Setting>('einzel')
  const create = useMutation({
    mutationFn: () => api.createEpisode({ patient_id: patientId, method, age_group: ageGroup, setting }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['episodes', patientId] })
      setOpen(false)
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button size="sm">
          <Plus /> {t('episodes.new')}
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('episodes.new')}</DialogTitle>
        </DialogHeader>
        <div className="grid gap-4">
          <EnumSelect label={t('episodes.method')} value={method} onChange={setMethod} options={['VT', 'TP', 'AP']} ns="method" />
          <EnumSelect label={t('episodes.ageGroup')} value={ageGroup} onChange={setAgeGroup} options={['adult', 'kjp']} ns="age_group" />
          <EnumSelect label={t('episodes.setting')} value={setting} onChange={setSetting} options={['einzel', 'gruppe', 'kombi']} ns="setting" />
        </div>
        <DialogFooter>
          <Button onClick={() => create.mutate()} disabled={create.isPending}>
            {t('common.create')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function EnumSelect<T extends string>({
  label,
  value,
  onChange,
  options,
  ns,
}: {
  label: string
  value: T
  onChange: (v: T) => void
  options: readonly T[]
  ns: string
}) {
  const { t } = useTranslation()
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      <Select value={value} onValueChange={(v) => onChange(v as T)}>
        <SelectTrigger className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {options.map((o) => (
            <SelectItem key={o} value={o}>
              {t(`enums.${ns}.${o}`)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}

function SessionsCard({ patientId, episodeId }: { patientId: string; episodeId?: string }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const sessions = useQuery(q.sessions(patientId))
  const [date, setDate] = useState('')
  const create = useMutation({
    mutationFn: () =>
      api.createSession({
        patient_id: patientId,
        episode_id: episodeId ?? null,
        scheduled_at: date ? new Date(date).toISOString() : null,
        modality: 'praesenz',
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['sessions'] })
      setDate('')
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0">
        <CardTitle className="text-base">{t('sessions.title')}</CardTitle>
        <div className="flex items-center gap-2">
          <Input type="datetime-local" className="w-56" value={date} onChange={(e) => setDate(e.target.value)} />
          <Button size="sm" variant="outline" onClick={() => create.mutate()} disabled={create.isPending}>
            <Plus /> {t('sessions.new')}
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        <ul className="divide-y text-sm">
          {sessions.data?.map((s) => (
            <li key={s.id} className="flex items-center justify-between py-2">
              <span>
                {formatDate(s.scheduled_at ?? s.started_at, true)}
                <span className="ml-3 text-muted-foreground">{t(`sessions.status.${s.status}`, s.status)}</span>
                {s.note_status && (
                  <span className="ml-3">
                    {t('sessions.note')}: <StatusBadge status={s.note_status} />
                    <ExportOutdatedBadge show={s.note_export_outdated} />
                  </span>
                )}
              </span>
              <Button asChild size="sm" variant="ghost">
                <Link to="/sessions/$sessionId" params={{ sessionId: s.id }}>
                  {t('sessions.open')}
                </Link>
              </Button>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  )
}
