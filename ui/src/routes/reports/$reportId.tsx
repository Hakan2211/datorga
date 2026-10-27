import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { save } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import { AlertTriangle, CheckCircle2, ChevronDown, CircleSlash, Download, FileSignature, Plus, Trash2, XCircle } from 'lucide-react'
import type { Diagnosis } from '@/bindings/Diagnosis'
import type { SectionDef } from '@/bindings/SectionDef'
import { Page, PageHeader } from '@/components/app-shell'
import { StatusBadge, formatDate } from '@/components/bits'
import { RichField } from '@/components/rich-field'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { api, errorMessage, q, type ReportView } from '@/lib/api'
import { estimatePages, parseDiagnoses, sectionText } from '@/lib/report'
import { cn } from '@/lib/utils'

export const Route = createFileRoute('/reports/$reportId')({ component: ReportEditorPage })

function ReportEditorPage() {
  const { reportId } = Route.useParams()
  const view = useQuery({ ...q.report(reportId), staleTime: Infinity })
  if (!view.data) return null
  // Keyed so local editor state resets when the report changes (e.g. after signing).
  return <ReportEditor key={`${reportId}:${view.data.report.status}`} view={view.data} />
}

const SAVE_DELAY_MS = 700

function ReportEditor({ view }: { view: ReportView }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const { report, template, patient, episode, application } = view
  const readOnly = report.status === 'signed' || report.status === 'exported'

  const [bodies, setBodies] = useState<Record<string, string>>(() =>
    Object.fromEntries(report.sections.map((s) => [s.key, s.body])),
  )
  // Debounced autosave per section. A section stays "dirty" until its latest edit is persisted.
  const [dirty, setDirty] = useState<ReadonlySet<string>>(new Set())
  const pending = dirty.size
  const timers = useRef<Record<string, ReturnType<typeof setTimeout>>>({})
  const versions = useRef<Record<string, number>>({})
  const queued = useRef<Record<string, string>>({})

  const markDirty = useCallback((key: string, on: boolean) => {
    setDirty((prev) => {
      const next = new Set(prev)
      if (on) next.add(key)
      else next.delete(key)
      return next
    })
  }, [])

  const persist = useCallback(
    (key: string, body: string, version: number) => {
      delete queued.current[key]
      api
        .saveSection(report.id, key, body)
        .then(() => {
          void qc.invalidateQueries({ queryKey: ['checklist', report.id] })
          void qc.invalidateQueries({ queryKey: ['reports'] })
        })
        .catch((e) => toast.error(errorMessage(e)))
        .finally(() => {
          if (versions.current[key] === version) markDirty(key, false)
        })
    },
    [qc, report.id, markDirty],
  )

  const update = useCallback(
    (key: string, body: string) => {
      setBodies((b) => ({ ...b, [key]: body }))
      const version = (versions.current[key] ?? 0) + 1
      versions.current[key] = version
      markDirty(key, true)
      clearTimeout(timers.current[key])
      queued.current[key] = body
      timers.current[key] = setTimeout(() => persist(key, body, version), SAVE_DELAY_MS)
    },
    [persist, markDirty],
  )

  // Leaving the page must not drop edits still waiting for their debounce.
  useEffect(
    () => () => {
      for (const [key, body] of Object.entries(queued.current)) {
        clearTimeout(timers.current[key])
        void api.saveSection(report.id, key, body)
      }
    },
    [report.id],
  )

  const pages = useMemo(
    () => estimatePages(template.sections.map((s) => sectionText(s.field, bodies[s.key] ?? ''))),
    [bodies, template.sections],
  )

  const subtitle = application
    ? `${t(`enums.kind.${application.kind}`)}${application.kind === 'fortfuehrung' ? ` Nr. ${application.seq}` : ''}`
    : template.title

  return (
    <>
      <PageHeader
        title={
          <span className="flex items-center gap-3">
            {subtitle}
            <StatusBadge status={report.status} />
          </span>
        }
        subtitle={
          <span className="flex items-center gap-2">
            <Link to="/patients/$patientId" params={{ patientId: patient.id }} className="hover:underline">
              {patient.first_name} {patient.last_name}
            </Link>
            <Badge variant="outline" className="font-mono">
              {patient.chiffre ?? t('patients.noChiffre')}
            </Badge>
            {episode && <span>· {t(`enums.method.${episode.method}`)}, {t(`enums.age_group.${episode.age_group}`)}</span>}
            <span className="text-xs">· {template.source}</span>
          </span>
        }
        actions={
          <span className="text-xs text-muted-foreground">{pending > 0 ? t('report.saving') : t('report.allSaved')}</span>
        }
      />
      <Page>
        <div className="grid grid-cols-[minmax(0,1fr)_20rem] gap-6">
          <div className="space-y-6">
            {readOnly && (
              <p className="rounded-md bg-success/10 px-4 py-2 text-sm text-success">
                {t('report.readOnly')} {t('report.signedBy')} {report.signed_by}, {formatDate(report.signed_at, true)}
              </p>
            )}
            <p className="text-sm text-muted-foreground">{template.intro}</p>
            {template.sections.map((s) => (
              <SectionEditor
                key={s.key}
                def={s}
                body={bodies[s.key] ?? ''}
                readOnly={readOnly}
                onChange={(b) => update(s.key, b)}
              />
            ))}
          </div>
          <aside className="space-y-4">
            <div className="sticky top-6 space-y-4">
              <LengthMeter pages={pages} target={template.rules.target_pages} />
              <ChecklistCard reportId={report.id} readOnly={readOnly} busy={pending > 0} />
              <ExportCard view={view} />
            </div>
          </aside>
        </div>
      </Page>
    </>
  )
}

function SectionEditor({
  def,
  body,
  readOnly,
  onChange,
}: {
  def: SectionDef
  body: string
  readOnly: boolean
  onChange: (body: string) => void
}) {
  const { t } = useTranslation()
  const [showHints, setShowHints] = useState(true)
  return (
    <section id={`section-${def.key}`} className="scroll-mt-6 space-y-2">
      <h2 className="font-serif text-[17px] leading-snug font-semibold">
        {def.number && <span className="mr-1.5 text-primary">{def.number}.</span>}
        {def.heading}
      </h2>
      {def.hints.length > 0 && (
        <div className="rounded-md bg-muted/60 px-3 py-2 text-[13px] text-muted-foreground">
          <button
            type="button"
            className="flex items-center gap-1 text-xs font-medium tracking-wide uppercase"
            onClick={() => setShowHints((v) => !v)}
          >
            <ChevronDown className={cn('size-3.5 transition-transform', !showHints && '-rotate-90')} />
            {t('report.hints')}
          </button>
          {showHints && (
            <ul className="mt-1.5 list-disc space-y-0.5 pl-5">
              {def.hints.map((h, i) => (
                <li key={i}>{h.text}</li>
              ))}
            </ul>
          )}
        </div>
      )}
      {!def.ai_allowed && (
        <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <CircleSlash className="size-3.5" /> {t('report.noAi')}
        </p>
      )}
      {def.field === 'prose' ? (
        <RichField value={body} onChange={onChange} placeholder={t('report.placeholder')} readOnly={readOnly} />
      ) : (
        <DiagnosesEditor body={body} onChange={onChange} readOnly={readOnly} />
      )}
    </section>
  )
}

const CERTAINTIES = ['G', 'V', 'Z', 'A'] as const

function DiagnosesEditor({ body, onChange, readOnly }: { body: string; onChange: (b: string) => void; readOnly: boolean }) {
  const { t } = useTranslation()
  const [state, setState] = useState(() => parseDiagnoses(body))
  const commit = (next: typeof state) => {
    setState(next)
    const empty = next.diagnoses.length === 0 && !next.notes
    onChange(empty ? '' : JSON.stringify(next))
  }
  const setRow = (i: number, patch: Partial<Diagnosis>) =>
    commit({ ...state, diagnoses: state.diagnoses.map((d, j) => (j === i ? { ...d, ...patch } : d)) })

  return (
    <div className="space-y-3 rounded-md border bg-card p-3">
      <Label className="text-xs text-muted-foreground">{t('report.diagnoses')}</Label>
      {state.diagnoses.map((d, i) => (
        <div key={i} className="grid grid-cols-[7rem_1fr_11rem_auto] gap-2">
          <Input
            className="font-mono"
            placeholder={t('report.code')}
            value={d.code}
            disabled={readOnly}
            onChange={(e) => setRow(i, { code: e.target.value.toUpperCase() })}
          />
          <Input placeholder={t('report.label')} value={d.label} disabled={readOnly} onChange={(e) => setRow(i, { label: e.target.value })} />
          <Select value={d.certainty} disabled={readOnly} onValueChange={(v) => setRow(i, { certainty: v })}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {CERTAINTIES.map((c) => (
                <SelectItem key={c} value={c}>
                  {t(`enums.certainty.${c}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            variant="ghost"
            size="icon"
            disabled={readOnly}
            aria-label={t('common.delete')}
            onClick={() => commit({ ...state, diagnoses: state.diagnoses.filter((_, j) => j !== i) })}
          >
            <Trash2 />
          </Button>
        </div>
      ))}
      {!readOnly && (
        <Button
          variant="outline"
          size="sm"
          onClick={() => commit({ ...state, diagnoses: [...state.diagnoses, { code: '', label: '', certainty: 'G' }] })}
        >
          <Plus /> {t('report.addDiagnosis')}
        </Button>
      )}
      <Label className="block pt-2 text-xs text-muted-foreground">{t('report.notes')}</Label>
      <RichField value={state.notes} onChange={(notes) => commit({ ...state, notes })} readOnly={readOnly} />
    </div>
  )
}

function LengthMeter({ pages, target }: { pages: number; target: number | null }) {
  const { t } = useTranslation()
  if (!target) return null
  const ratio = Math.min(pages / (target * 1.25), 1)
  const over = pages > target * 1.25
  return (
    <Card>
      <CardContent className="space-y-2 pt-5">
        <div className="flex justify-between text-sm">
          <span>{t('report.length')}</span>
          <span className={cn('tabular-nums', over && 'text-destructive')}>
            ≈ {pages.toFixed(1)} / {target} {t('report.pages')}
          </span>
        </div>
        <div className="h-1.5 overflow-hidden rounded-full bg-muted">
          <div className={cn('h-full rounded-full', over ? 'bg-destructive' : 'bg-primary')} style={{ width: `${ratio * 100}%` }} />
        </div>
        <p className="text-xs text-muted-foreground">
          {t('report.target')}: {target} {t('report.pages')} (PTV 3)
        </p>
      </CardContent>
    </Card>
  )
}

function ChecklistCard({ reportId, readOnly, busy }: { reportId: string; readOnly: boolean; busy: boolean }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const checklist = useQuery(q.checklist(reportId))
  const therapist = useQuery(q.setting<string>('therapist_name'))
  const [signer, setSigner] = useState<string | null>(null)
  const signAs = signer ?? therapist.data ?? ''

  const sign = useMutation({
    mutationFn: () => api.sign(reportId, signAs),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['report', reportId] })
      void qc.invalidateQueries({ queryKey: ['reports'] })
      toast.success(t('enums.status.signed'))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  const items = checklist.data ?? []
  const blocked = items.some((c) => !c.ok && c.severity === 'block')
  const failedOnly = items.filter((c) => !c.ok || !c.key.startsWith('pseudonym:'))

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">{t('report.checklist')}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <ul className="space-y-1.5 text-[13px]">
          {failedOnly.map((c) => (
            <li key={c.key} className="flex items-start gap-2">
              {c.ok ? (
                <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-success" />
              ) : c.severity === 'block' ? (
                <XCircle className="mt-0.5 size-4 shrink-0 text-destructive" />
              ) : (
                <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" />
              )}
              {c.section_key ? (
                <a href={`#section-${c.section_key}`} className="hover:underline">
                  {c.message}
                </a>
              ) : (
                <span>{c.message}</span>
              )}
            </li>
          ))}
        </ul>
        {!readOnly && (
          <div className="space-y-2 border-t pt-3">
            <Label htmlFor="signer" className="text-xs">
              {t('report.signAs')}
            </Label>
            <Input id="signer" value={signAs} onChange={(e) => setSigner(e.target.value)} />
            <Button className="w-full" disabled={busy || blocked || !signAs.trim() || sign.isPending} onClick={() => sign.mutate()}>
              <FileSignature /> {t('report.sign')}
            </Button>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

function ExportCard({ view }: { view: ReportView }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const exportAs = useMutation({
    mutationFn: async (format: 'pdf' | 'docx') => {
      const base = `Bericht-${view.patient.chiffre ?? 'ohne-Chiffre'}-${view.application?.kind ?? 'bericht'}`
      const path = await save({
        defaultPath: `${base}.${format}`,
        filters: [{ name: format.toUpperCase(), extensions: [format] }],
      })
      if (!path) return false
      await api.exportReport(view.report.id, format, path)
      return true
    },
    onSuccess: (done) => {
      if (!done) return
      toast.success(t('report.exported'))
      void qc.invalidateQueries({ queryKey: ['report', view.report.id] })
      void qc.invalidateQueries({ queryKey: ['reports'] })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <Card>
      <CardContent className="grid gap-2 pt-5">
        <Button variant="outline" onClick={() => exportAs.mutate('pdf')} disabled={exportAs.isPending}>
          <Download /> {t('report.exportPdf')}
        </Button>
        <Button variant="outline" onClick={() => exportAs.mutate('docx')} disabled={exportAs.isPending}>
          <Download /> {t('report.exportDocx')}
        </Button>
      </CardContent>
    </Card>
  )
}
