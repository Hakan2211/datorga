import { useState } from 'react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { FilePlus2, History, LayoutGrid, Pencil, Plus, Stethoscope, Trash2 } from 'lucide-react'
import type { Diagnosis } from '@/bindings/Diagnosis'
import type { AgeGroup } from '@/bindings/AgeGroup'
import type { ApplicationKind } from '@/bindings/ApplicationKind'
import type { Episode } from '@/bindings/Episode'
import type { Method } from '@/bindings/Method'
import type { Setting } from '@/bindings/Setting'
import { Page, PageHeader } from '@/components/app-shell'
import { ExportOutdatedBadge, StatusBadge, formatDate } from '@/components/bits'
import { ConsentCard } from '@/components/consent-card'
import { DiagnosisRows } from '@/components/diagnoses'
import { PatientAvatar } from '@/components/patient-avatar'
import { PatientFormDialog } from '@/components/patient-form'
import { PatientTimeline } from '@/components/patient-timeline'
import { SessionDialog, type SessionDialogMode } from '@/components/session-dialog'
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
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { api, errorMessage, q } from '@/lib/api'

export const Route = createFileRoute('/patients/$patientId')({ component: PatientDetail })

function PatientDetail() {
  const { patientId } = Route.useParams()
  const { t } = useTranslation()
  const navigate = useNavigate()
  const qc = useQueryClient()
  const patient = useQuery(q.patient(patientId))
  const photo = useQuery(q.patientPhoto(patientId))
  const episodes = useQuery(q.episodes(patientId))
  const [tab, setTab] = useState('overview')

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
        leading={<PatientAvatar patientId={patientId} hasPhoto firstName={p.first_name} lastName={p.last_name} />}
        title={`${p.first_name} ${p.last_name}`}
        subtitle={
          <span className="flex items-center gap-3">
            {formatDate(p.birth_date)}
            {p.gender && <span>{t(`enums.gender.${p.gender}`)}</span>}
            <Badge variant="outline" className="font-mono">
              {p.chiffre ?? t('patients.noChiffre')}
            </Badge>
          </span>
        }
        actions={
          <>
            <PatientFormDialog
              patient={p}
              hasPhoto={!!photo.data}
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
        <Tabs value={tab} onValueChange={setTab} className="gap-6">
          <TabsList>
            <TabsTrigger value="overview" className="px-3" data-testid="tab-overview">
              <LayoutGrid /> {t('timeline.tabOverview')}
            </TabsTrigger>
            <TabsTrigger value="timeline" className="px-3" data-testid="tab-timeline">
              <History /> {t('timeline.tab')}
            </TabsTrigger>
          </TabsList>
          <TabsContent value="overview" className="space-y-6">
            <ConsentCard patientId={patientId} kind="recording" />
            <ConsentCard patientId={patientId} kind="cloud_transcription" />
            <div className="flex items-center justify-between">
              <h2 className="text-base font-semibold">{t('episodes.title')}</h2>
              <NewEpisodeDialog patientId={patientId} />
            </div>
            {episodes.data?.length === 0 && <p className="text-sm text-muted-foreground">{t('episodes.none')}</p>}
            {episodes.data?.map((e) => <EpisodeCard key={e.id} episode={e} />)}
            <SessionsCard patientId={patientId} />
          </TabsContent>
          <TabsContent value="timeline">
            <PatientTimeline patientId={patientId} />
          </TabsContent>
        </Tabs>
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
      <CardContent className="space-y-4">
        <EpisodeDiagnoses episode={episode} />
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

function EpisodeDiagnoses({ episode }: { episode: Episode }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [open, setOpen] = useState(false)
  const [rows, setRows] = useState<Diagnosis[]>(episode.diagnoses)
  const save = useMutation({
    mutationFn: () => api.setEpisodeDiagnoses(episode.id, rows),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['episodes', episode.patient_id] })
      void qc.invalidateQueries({ queryKey: ['patients'] })
      toast.success(t('common.saved'))
      setOpen(false)
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <div className="flex items-start justify-between gap-4 rounded-md bg-muted/50 px-3 py-2.5">
      <div className="flex min-w-0 items-start gap-2.5 text-sm">
        <Stethoscope className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
        {episode.diagnoses.length === 0 ? (
          <span className="text-muted-foreground">{t('episodes.diagnosesNone')}</span>
        ) : (
          <ul className="space-y-0.5" data-testid="episode-diagnoses">
            {episode.diagnoses.map((d) => (
              <li key={d.code}>
                <span className="font-mono">{d.code}</span>
                {d.certainty !== 'G' && <span className="ml-1 text-muted-foreground">{d.certainty}</span>}
                {d.label && <span className="ml-2">{d.label}</span>}
              </li>
            ))}
          </ul>
        )}
      </div>
      <Dialog
        open={open}
        onOpenChange={(o) => {
          if (o) setRows(episode.diagnoses)
          setOpen(o)
        }}
      >
        <DialogTrigger asChild>
          <Button size="sm" variant="ghost" data-testid="episode-diagnoses-edit">
            <Pencil /> {t('episodes.diagnosesEdit')}
          </Button>
        </DialogTrigger>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t('episodes.diagnoses')}</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-muted-foreground">{t('episodes.diagnosesHint')}</p>
          <DiagnosisRows value={rows} onChange={setRows} />
          <DialogFooter>
            <Button variant="ghost" onClick={() => setOpen(false)}>
              {t('common.cancel')}
            </Button>
            <Button onClick={() => save.mutate()} disabled={save.isPending}>
              {t('common.save')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
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

function SessionsCard({ patientId }: { patientId: string }) {
  const { t } = useTranslation()
  const sessions = useQuery(q.sessions(patientId))
  const [dialog, setDialog] = useState<SessionDialogMode | null>(null)

  return (
    <Card>
      <SessionDialog mode={dialog} onOpenChange={(o) => !o && setDialog(null)} />
      <CardHeader className="flex flex-row items-center justify-between space-y-0">
        <CardTitle className="text-base">{t('sessions.title')}</CardTitle>
        <Button size="sm" variant="outline" onClick={() => setDialog({ kind: 'create', patientId })} data-testid="patient-new-session">
          <Plus /> {t('sessions.new')}
        </Button>
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
