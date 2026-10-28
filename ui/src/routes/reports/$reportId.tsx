import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient, type UseMutationResult } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  ChevronDown,
  CircleSlash,
  CornerDownLeft,
  Download,
  Eye,
  FileSignature,
  FileText,
  LayoutList,
  Sparkles,
  Plus,
  TextSelect,
  Trash2,
  XCircle,
} from 'lucide-react'
import type { ApplyMode } from '@/bindings/ApplyMode'
import type { Diagnosis } from '@/bindings/Diagnosis'
import type { FactEdit } from '@/bindings/FactEdit'
import type { SectionDef } from '@/bindings/SectionDef'
import type { SectionDraft } from '@/bindings/SectionDraft'
import type { SourceSegment } from '@/bindings/SourceSegment'
import { Addenda } from '@/components/addenda'
import { FactDraftCard } from '@/components/fact-draft'
import { DraftDialog, DraftProposal, DraftSources, ReportDraftCard } from '@/components/report-draft'
import { Page, PageHeader } from '@/components/app-shell'
import { StatusBadge, Stepper, StickyActionBar, formatDate, stepStates } from '@/components/bits'
import { useReportExport, type ExportFormat } from '@/components/report-export'
import { ReportDocument, type DocumentFrame } from '@/components/report-document'
import { ReportPreview } from '@/components/report-preview'
import { RichField } from '@/components/rich-field'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Toggle } from '@/components/ui/toggle'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { api, errorMessage, q, type ReportView } from '@/lib/api'
import { appendParagraph, estimatePages, isBlank, parseDiagnoses, sectionText } from '@/lib/report'
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
// View choices are per machine, like the theme (localStorage, not the encrypted settings).
const VIEW_KEY = 'thera.reportView'
const PREVIEW_KEY = 'thera.reportPreview'
type EditorView = 'sections' | 'document'

function ReportEditor({ view }: { view: ReportView }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const { report, template, patient, episode, application, session } = view
  const readOnly = report.status === 'signed' || report.status === 'exported'
  const isNote = report.kind === 'sitzungsnotiz'
  // Target for text picked from the transcript: the section last focused.
  const [activeKey, setActiveKey] = useState(() => template.sections[0]?.key ?? '')
  const [mode, setModeState] = useState<EditorView>(() => (localStorage.getItem(VIEW_KEY) === 'document' ? 'document' : 'sections'))
  const [preview, setPreviewState] = useState(() => localStorage.getItem(PREVIEW_KEY) === '1')
  const setMode = (m: EditorView) => {
    localStorage.setItem(VIEW_KEY, m)
    setModeState(m)
  }
  const setPreview = (on: boolean) => {
    localStorage.setItem(PREVIEW_KEY, on ? '1' : '0')
    setPreviewState(on)
  }
  const practice = useQuery(q.setting<string>('practice_header'))

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

  // Saves edits still waiting for their debounce (before the server reads or writes sections).
  const flush = useCallback(async () => {
    const waiting = Object.entries(queued.current)
    for (const [key] of waiting) {
      clearTimeout(timers.current[key])
      delete queued.current[key]
    }
    await Promise.all(waiting.map(([key, body]) => api.saveSection(report.id, key, body)))
    for (const [key] of waiting) markDirty(key, false)
  }, [report.id, markDirty])

  // The server writes AI text into sections, so edits still waiting for their debounce go first.
  const flushThen = useCallback(
    async (write: () => Promise<{ sections: { key: string; body: string }[] }>) => {
      await flush()
      const updated = await write()
      setBodies(Object.fromEntries(updated.sections.map((s) => [s.key, s.body])))
      void qc.invalidateQueries({ queryKey: ['checklist', report.id] })
      void qc.invalidateQueries({ queryKey: ['reports'] })
    },
    [qc, report.id, flush],
  )
  const applyFacts = useCallback((edits: FactEdit[]) => flushThen(() => api.applyFacts(report.id, edits)), [flushThen, report.id])

  // Bericht drafting from the episode's applied Sitzungsnotiz facts.
  const drafting = !isNote && !readOnly && !!application
  const draftable = useMemo(() => template.sections.filter((s) => s.field === 'prose' && s.ai_allowed), [template.sections])
  const drafts = useQuery({ ...q.reportDrafts(report.id), enabled: !isNote })
  const templates = useQuery({ ...q.templates(), enabled: !isNote })
  const noteSections = templates.data?.find((tp) => tp.key === 'sitzungsnotiz')?.sections ?? []
  const [draftKeys, setDraftKeys] = useState<string[] | null>(null)
  // The draft may send the sections' current text along: save pending edits first.
  const openDraft = (keys: string[]) => {
    flush()
      .then(() => setDraftKeys(keys))
      .catch((e) => toast.error(errorMessage(e)))
  }
  const applyDraft = async (d: SectionDraft, mode: ApplyMode) => {
    await flushThen(() => api.applyDraft(report.id, d.id, mode))
    void qc.invalidateQueries({ queryKey: ['reportDrafts', report.id] })
    toast.success(t('draft.applied'))
  }
  const rejectDraft = async (d: SectionDraft) => {
    qc.setQueryData(q.reportDrafts(report.id).queryKey, await api.rejectDraft(report.id, d.id))
  }

  const pages = useMemo(
    () => estimatePages(template.sections.map((s) => sectionText(s.field, bodies[s.key] ?? ''))),
    [bodies, template.sections],
  )

  // Guided flow for a Bericht; a note's flow is its session's.
  const flowOn = !isNote && !!application
  const sources = useQuery({ ...q.draftSources(report.id), enabled: flowOn && !readOnly })
  const llm = useQuery({ ...q.llm(), enabled: flowOn && !readOnly })
  const checklist = useQuery(q.checklist(report.id))
  const exportAs = useReportExport(view)
  const anyFilled = template.sections.some((s) => !isBlank(bodies[s.key] ?? ''))
  const proposals = drafts.data?.filter((d) => d.status === 'proposed') ?? []
  const blocked = checklist.data?.some((c) => !c.ok && c.severity === 'block') ?? true
  const exported = report.status === 'exported' && !report.export_outdated
  const states = stepStates<ReportStep>([
    { key: 'facts', done: readOnly || anyFilled || (sources.data?.facts.length ?? 0) > 0 },
    { key: 'draft', done: readOnly || (anyFilled && proposals.length === 0) },
    { key: 'edit', done: readOnly || !blocked },
    { key: 'check', done: readOnly },
    { key: 'export', done: exported },
  ])
  const current = REPORT_FLOW.find((k) => states[k] === 'current')
  const emptyDraftable = draftable.filter((s) => isBlank(bodies[s.key] ?? '')).map((s) => s.key)
  // After a view switch the target exists only once React has rendered it.
  const scrollTo = (id: string) =>
    requestAnimationFrame(() => document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' }))
  const jump = (step: ReportStep) => {
    // Checklist and export live in the side column, which the preview takes over.
    if ((step === 'check' || step === 'export') && preview) setPreview(false)
    if (step === 'draft' && proposals[0]) {
      // Proposals are reviewed next to their section.
      setMode('sections')
      scrollTo(`section-${proposals[0].section_key}`)
    } else if (step === 'edit') {
      const failing = checklist.data?.find((c) => !c.ok && c.section_key)?.section_key
      scrollTo(`section-${failing ?? template.sections[0]?.key}`)
    } else scrollTo(STEP_ANCHOR[step])
  }
  // A note's bar only leads to signing and exporting; the rest of its flow is on the session page.
  const barStep: ReportStep | null = flowOn ? (current ?? null) : isNote ? (readOnly ? (exported ? null : 'export') : 'check') : null

  const editLocked = (key: string) => {
    setMode('sections')
    scrollTo(`section-${key}`)
  }
  const docSubtitle = session
    ? `Sitzung vom ${formatDate(session.scheduled_at ?? session.started_at ?? report.created_at)}`
    : application
      ? application.kind === 'fortfuehrung'
        ? `Bericht Nr. ${application.seq} zum Fortführungsantrag`
        : `Bericht zum ${application.kind === 'erstantrag' ? 'Erstantrag' : 'Umwandlungsantrag'}`
      : template.title
  // Mirrors render::build_doc and bericht.typ, so the sheet matches the export.
  const frame: DocumentFrame = {
    header: [
      `Chiffre: ${patient.chiffre ?? '—'}`,
      episode && `Verfahren: ${episode.method}`,
      application?.kind === 'fortfuehrung' && `Bericht Nr. ${application.seq}`,
    ]
      .filter(Boolean)
      .join('\u2003'),
    practice: practice.data?.trim() || null,
    title: isNote ? template.title : 'Bericht an die Gutachterin / den Gutachter',
    subtitle: docSubtitle,
    date: formatDate(report.signed_at ?? report.updated_at),
    signedBy: report.signed_by,
  }
  const activeDef = template.sections.find((s) => s.key === activeKey)

  const subtitle = isNote
    ? `${template.title} · ${formatDate(session?.scheduled_at ?? session?.started_at ?? report.created_at)}`
    : application
      ? `${t(`enums.kind.${application.kind}`)}${application.kind === 'fortfuehrung' ? ` Nr. ${application.seq}` : ''}`
      : template.title

  return (
    <div className="flex min-h-full flex-col">
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
            {session && (
              <Link to="/sessions/$sessionId" params={{ sessionId: session.id }} className="hover:underline">
                · {t('note.session')} {formatDate(session.scheduled_at ?? session.started_at, true)}
              </Link>
            )}
            <span className="text-xs">· {template.source}</span>
          </span>
        }
      />
      <Page>
        {flowOn && (
          <Stepper
            steps={REPORT_FLOW.map((k) => ({ key: k, label: t(`flow.report.${k}`), state: states[k] }))}
            onSelect={jump}
            className="mx-auto max-w-4xl"
          />
        )}
        <div className="flex items-center justify-between gap-3">
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            value={mode}
            onValueChange={(v) => v && setMode(v as EditorView)}
            aria-label={t('report.view.label')}
          >
            <ToggleGroupItem value="sections" className="px-3" data-testid="view-sections">
              <LayoutList /> {t('report.view.sections')}
            </ToggleGroupItem>
            <ToggleGroupItem value="document" className="px-3" data-testid="view-document">
              <FileText /> {t('report.view.document')}
            </ToggleGroupItem>
          </ToggleGroup>
          <Toggle variant="outline" size="sm" className="px-3" pressed={preview} onPressedChange={setPreview} data-testid="preview-toggle">
            <Eye /> {t('report.preview')}
          </Toggle>
        </div>
        <div className={cn('grid gap-6', preview ? 'grid-cols-[minmax(0,1fr)_minmax(0,1fr)]' : 'grid-cols-[minmax(0,1fr)_20rem]')}>
          <div className="min-w-0 space-y-6">
            {readOnly && (
              <p className="rounded-md bg-success/10 px-4 py-2 text-sm text-success">
                {t(isNote ? 'note.readOnly' : 'report.readOnly')} {t('report.signedBy')} {report.signed_by}, {formatDate(report.signed_at, true)}
              </p>
            )}
            <p className="text-sm text-muted-foreground">{template.intro}</p>
            {isNote && !readOnly && view.segments.length > 0 && (
              <FactDraftCard
                reportId={report.id}
                patientId={patient.id}
                sections={template.sections.filter((s) => s.field === 'prose' && s.ai_allowed)}
                onApply={applyFacts}
              />
            )}
            {drafting && (
              <div id="flow-draft" className="scroll-mt-6">
                <ReportDraftCard
                  reportId={report.id}
                  sections={draftable}
                  isEmpty={(key) => isBlank(bodies[key] ?? '')}
                  onDraft={openDraft}
                />
              </div>
            )}
            {mode === 'document' && proposals.length > 0 && !readOnly && (
              <div className="flex items-center justify-between gap-3 rounded-md border border-primary/30 bg-primary/5 px-4 py-2 text-sm">
                <span className="flex items-center gap-2">
                  <Sparkles className="size-4 text-primary" /> {t('report.proposalsWaiting', { count: proposals.length })}
                </span>
                <Button size="sm" variant="outline" onClick={() => jump('draft')}>
                  {t('report.toSections')}
                </Button>
              </div>
            )}
            {mode === 'document' ? (
              <ReportDocument
                sections={template.sections}
                bodies={bodies}
                readOnly={readOnly}
                frame={frame}
                onChange={update}
                onActive={setActiveKey}
                onEditLocked={editLocked}
              />
            ) : template.sections.map((s) => {
              const canDraft = drafting && draftable.some((d) => d.key === s.key)
              const proposal = drafts.data?.find((d) => d.section_key === s.key && d.status === 'proposed')
              const applied = drafts.data?.filter((d) => d.section_key === s.key && d.status === 'applied') ?? []
              return (
                <SectionEditor
                  key={s.key}
                  def={s}
                  body={bodies[s.key] ?? ''}
                  readOnly={readOnly}
                  active={isNote && !readOnly && s.key === activeKey}
                  onFocus={() => setActiveKey(s.key)}
                  onChange={(b) => update(s.key, b)}
                  action={
                    canDraft && (
                      <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={() => openDraft([s.key])}>
                        <Sparkles className="size-3.5" /> {t('draft.section')}
                      </Button>
                    )
                  }
                  above={
                    proposal &&
                    !readOnly && (
                      <DraftProposal
                        draft={proposal}
                        sectionEmpty={isBlank(bodies[s.key] ?? '')}
                        noteSections={noteSections}
                        onApply={(mode) => applyDraft(proposal, mode)}
                        onReject={() => rejectDraft(proposal)}
                      />
                    )
                  }
                  below={applied.map((d) => (
                    <DraftSources key={d.id} draft={d} noteSections={noteSections} />
                  ))}
                />
              )
            })}
            {draftKeys && (
              <DraftDialog
                reportId={report.id}
                patientId={patient.id}
                keys={draftKeys}
                sections={draftable}
                filled={draftKeys.filter((k) => !isBlank(bodies[k] ?? ''))}
                onClose={() => setDraftKeys(null)}
              />
            )}
            {isNote && readOnly && <Addenda reportId={report.id} addenda={report.addenda} noteSigner={report.signed_by} />}
          </div>
          {preview ? (
            <div className="sticky top-4 h-[calc(100vh-12rem)] min-h-96 self-start">
              <ReportPreview
                reportId={report.id}
                content={`${report.status}:${JSON.stringify(bodies)}`}
                flush={flush}
                onClose={() => setPreview(false)}
              />
            </div>
          ) : (
          <aside className="space-y-4">
            <div className="sticky top-6 space-y-4">
              {mode === 'document' && !readOnly && activeDef && activeDef.hints.length > 0 && (
                <Card>
                  <CardHeader className="pb-2">
                    <CardTitle className="text-sm">{t('report.activeHints', { section: activeDef.heading })}</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <ul className="list-disc space-y-0.5 pl-5 text-[13px] text-muted-foreground">
                      {activeDef.hints.map((h, i) => (
                        <li key={i}>{h.text}</li>
                      ))}
                    </ul>
                  </CardContent>
                </Card>
              )}
              <LengthMeter pages={pages} target={template.rules.target_pages} />
              <ChecklistCard reportId={report.id} readOnly={readOnly} busy={pending > 0} primary={barStep === 'check'} />
              {isNote && !readOnly && (
                <TranscriptPicker
                  segments={view.segments}
                  sections={template.sections.filter((s) => s.field === 'prose')}
                  target={activeKey}
                  onTarget={setActiveKey}
                  onInsert={(key, text) => update(key, appendParagraph(bodies[key] ?? '', text))}
                />
              )}
              <ExportCard view={view} exportAs={exportAs} />
            </div>
          </aside>
          )}
        </div>
      </Page>
      <ReportBar
        info={
          <>
            <span>{pending > 0 ? t('report.saving') : t('report.allSaved')}</span>
            {template.rules.target_pages && (
              <span>
                ≈ {pages.toFixed(1)} / {template.rules.target_pages} {t('report.pages')}
              </span>
            )}
          </>
        }
        step={barStep}
        canDraftEmpty={
          drafting && !!llm.data?.config.active && emptyDraftable.length > 0 && (sources.data?.facts.length ?? 0) > 0
        }
        proposals={proposals.length}
        onDraftEmpty={() => openDraft(emptyDraftable)}
        onJump={jump}
        onExport={() => exportAs.mutate('pdf')}
        exporting={exportAs.isPending}
      />
    </div>
  )
}

const REPORT_FLOW = ['facts', 'draft', 'edit', 'check', 'export'] as const
type ReportStep = (typeof REPORT_FLOW)[number]
const STEP_ANCHOR: Record<ReportStep, string> = {
  facts: 'flow-draft',
  draft: 'flow-draft',
  edit: 'flow-draft',
  check: 'flow-check',
  export: 'flow-export',
}

/** The bottom bar: save state on the left, the one next action of the current step on the right. */
function ReportBar({
  info,
  step,
  canDraftEmpty,
  proposals,
  onDraftEmpty,
  onJump,
  onExport,
  exporting,
}: {
  info: ReactNode
  step: ReportStep | null
  canDraftEmpty: boolean
  proposals: number
  onDraftEmpty: () => void
  onJump: (step: ReportStep) => void
  onExport: () => void
  exporting: boolean
}) {
  const { t } = useTranslation()
  let action: ReactNode = null
  if (step === 'facts') {
    info = (
      <>
        {info}
        <span>{t('flow.report.noFacts')}</span>
      </>
    )
  } else if (step === 'draft' && proposals > 0) {
    action = (
      <Button onClick={() => onJump('draft')}>
        {t('flow.report.toProposal', { count: proposals })} <ArrowRight />
      </Button>
    )
  } else if (step === 'draft' && canDraftEmpty) {
    action = (
      <Button onClick={onDraftEmpty}>
        <Sparkles /> {t('draft.draftEmpty')}
      </Button>
    )
  } else if (step === 'draft' || step === 'edit' || step === 'check') {
    action = (
      <Button onClick={() => onJump('check')} variant={step === 'draft' ? 'outline' : 'default'} data-testid="flow-next">
        {t('flow.report.toCheck')} <ArrowRight />
      </Button>
    )
  } else if (step === 'export') {
    action = (
      <Button onClick={onExport} disabled={exporting} data-testid="flow-export">
        <Download /> {t('report.exportPdf')}
      </Button>
    )
  }
  return (
    <StickyActionBar info={info} className="mt-auto">
      {action}
    </StickyActionBar>
  )
}

function SectionEditor({
  def,
  body,
  readOnly,
  active = false,
  onFocus,
  onChange,
  action,
  above,
  below,
}: {
  def: SectionDef
  body: string
  readOnly: boolean
  active?: boolean
  onFocus?: () => void
  onChange: (body: string) => void
  /** Next to the heading (e.g. "Entwurf…"). */
  action?: ReactNode
  /** Between the hints and the editor (e.g. an AI proposal). */
  above?: ReactNode
  /** Under the editor (e.g. sources of applied AI text). */
  below?: ReactNode
}) {
  const { t } = useTranslation()
  const [showHints, setShowHints] = useState(true)
  return (
    <section
      id={`section-${def.key}`}
      onFocusCapture={onFocus}
      className={cn('scroll-mt-6 space-y-2 border-l-2 border-transparent pl-3 -ml-3.5', active && 'border-primary/60')}
    >
      <div className="flex items-start justify-between gap-3">
        <h2 className="font-serif text-[17px] leading-snug font-semibold">
          {def.number && <span className="mr-1.5 text-primary">{def.number}.</span>}
          {def.heading}
        </h2>
        {action}
      </div>
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
      {above}
      {def.field === 'prose' ? (
        <RichField value={body} onChange={onChange} placeholder={t('report.placeholder')} readOnly={readOnly} />
      ) : (
        <DiagnosesEditor body={body} onChange={onChange} readOnly={readOnly} />
      )}
      {below}
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

function ChecklistCard({ reportId, readOnly, busy, primary }: { reportId: string; readOnly: boolean; busy: boolean; primary: boolean }) {
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
    <Card id="flow-check" className="scroll-mt-6">
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
            <Button className="w-full" variant={primary ? 'default' : 'outline'} disabled={busy || blocked || !signAs.trim() || sign.isPending} onClick={() => sign.mutate()}>
              <FileSignature /> {t('report.sign')}
            </Button>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

function ExportCard({
  view,
  exportAs,
}: {
  view: ReportView
  exportAs: UseMutationResult<boolean, Error, ExportFormat>
}) {
  const { t } = useTranslation()
  return (
    <Card id="flow-export" className="scroll-mt-6">
      <CardContent className="grid gap-2 pt-5">
        {view.report.export_outdated && (
          <p className="flex items-start gap-2 text-sm text-warning-foreground">
            <AlertTriangle className="mt-0.5 size-4 shrink-0" />
            {t('note.exportOutdatedSince', { date: formatDate(view.report.exported_at, true) })}
          </p>
        )}
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

/** Confirmed transcript of the note's session; a segment, or the part selected in it, can be appended to a section. */
function TranscriptPicker({
  segments,
  sections,
  target,
  onTarget,
  onInsert,
}: {
  segments: SourceSegment[]
  sections: SectionDef[]
  target: string
  onTarget: (key: string) => void
  onInsert: (key: string, text: string) => void
}) {
  const { t } = useTranslation()
  const [used, setUsed] = useState<ReadonlySet<number>>(new Set())
  const key = sections.some((s) => s.key === target) ? target : (sections[0]?.key ?? '')
  // Text selected within a single segment (Whisper may return a long dictation as one segment).
  const [picked, setPicked] = useState<{ index: number; text: string } | null>(null)
  useEffect(() => {
    const onChange = () => {
      const sel = document.getSelection()
      const text = sel?.toString().trim() ?? ''
      const node = sel && sel.rangeCount > 0 ? sel.getRangeAt(0).commonAncestorContainer : null
      const el = (node instanceof Element ? node : node?.parentElement)?.closest<HTMLElement>('[data-segment]')
      setPicked(text && el ? { index: Number(el.dataset.segment), text } : null)
    }
    document.addEventListener('selectionchange', onChange)
    return () => document.removeEventListener('selectionchange', onChange)
  }, [])

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">{t('note.transcript')}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        {segments.length === 0 ? (
          <p className="text-[13px] text-muted-foreground">{t('note.transcriptEmpty')}</p>
        ) : (
          <>
            <div className="space-y-1.5">
              <Label className="text-xs">{t('note.insertInto')}</Label>
              <Select value={key} onValueChange={onTarget}>
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {sections.map((s) => (
                    <SelectItem key={s.key} value={s.key}>
                      {s.heading}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <p className="text-xs text-muted-foreground">{t('note.selectHint')}</p>
            <ul className="max-h-[50vh] space-y-1 overflow-y-auto text-[13px]">
              {segments.map((seg, i) => {
                const part = picked?.index === i ? picked.text : null
                return (
                <li
                  key={seg.segment_id}
                  data-segment-id={seg.segment_id}
                  className="group flex items-start gap-2 rounded px-1.5 py-1 transition-shadow hover:bg-muted/60"
                >
                  <span className="flex-1">
                    <span className="mr-1.5 text-[11px] text-muted-foreground">{t(`note.role.${seg.role}`)}</span>
                    <span data-segment={i} className={cn(used.has(i) && 'text-muted-foreground')}>
                      {seg.text}
                    </span>
                  </span>
                  <Button
                    variant={part ? 'secondary' : 'ghost'}
                    size="icon"
                    className="size-6 shrink-0"
                    title={t(part ? 'note.insertSelection' : 'note.insert')}
                    aria-label={t(part ? 'note.insertSelection' : 'note.insert')}
                    // Keep the text selection alive until the click is handled.
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => {
                      onInsert(key, part ?? seg.text)
                      setUsed((u) => new Set(u).add(i))
                      if (part) document.getSelection()?.removeAllRanges()
                    }}
                  >
                    {part ? <TextSelect className="size-3.5" /> : <CornerDownLeft className="size-3.5" />}
                  </Button>
                </li>
                )
              })}
            </ul>
          </>
        )}
      </CardContent>
    </Card>
  )
}
