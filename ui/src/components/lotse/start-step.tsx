import { Link } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { AlertTriangle, ArrowLeftRight, FileText, History, LineChart, RefreshCw } from 'lucide-react'
import type { ApplicationKind } from '@/bindings/ApplicationKind'
import type { SectionDraft } from '@/bindings/SectionDraft'
import { formatDate } from '@/components/bits'
import { PatientFormDialog } from '@/components/patient-form'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { api, errorMessage, q, type DraftSources, type QuestionView, type ReportView } from '@/lib/api'
import { isOpen, readiness, sameDiagnoses, sectionFacts, type Readiness } from '@/lib/lotse'
import { isBlank, parseDiagnoses } from '@/lib/report'
import { cn } from '@/lib/utils'

const KINDS: { kind: ApplicationKind; icon: typeof FileText }[] = [
  { kind: 'erstantrag', icon: FileText },
  { kind: 'umwandlung', icon: ArrowLeftRight },
  { kind: 'fortfuehrung', icon: RefreshCw },
]

/**
 * Lotse step 1: the Antragsart (changeable while the Bericht is empty) and what thera already
 * knows per section, plus the gaps to close here before writing.
 */
export function StartStep({
  view,
  bodies,
  notRelevant,
  drafts,
  sources,
  questions,
  llmActive,
  onRelevance,
  onChange,
}: {
  view: ReportView
  bodies: Record<string, string>
  notRelevant: ReadonlySet<string>
  drafts: SectionDraft[]
  sources: DraftSources | undefined
  /** The "Fragen" step's questions: open ones show per section. */
  questions: QuestionView[]
  llmActive: boolean
  onRelevance: (key: string, notRelevant: boolean) => void
  onChange: (key: string, body: string) => void
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const { report, template, patient, episode, application } = view
  const readOnly = report.status === 'signed' || report.status === 'exported'
  const candidate = useQuery({ ...q.berichtCandidates(episode?.id), enabled: !!episode }).data?.[0]
  const changeable =
    !readOnly &&
    drafts.length === 0 &&
    template.sections.every((s) => s.field !== 'prose' || isBlank(bodies[s.key] ?? ''))
  const change = useMutation({
    mutationFn: (kind: ApplicationKind) => api.changeApplicationKind(application!.id, kind),
    onSuccess: () => {
      for (const key of [['report', report.id], ['reports'], ['berichtCandidates'], ['applications'], ['checklist', report.id], ['draftSources', report.id]]) {
        void qc.invalidateQueries({ queryKey: key })
      }
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  const facts = sources?.facts ?? []
  const since = sources?.since ?? null
  const recent = since ? facts.filter((f) => !f.session_at || f.session_at >= since) : facts
  const sessions = new Set(recent.map((f) => f.session_id)).size

  const diagDef = template.sections.find((s) => s.field === 'diagnoses')
  const reportDiagnoses = diagDef ? parseDiagnoses(bodies[diagDef.key] ?? '') : null
  const diagnosesDiffer =
    !readOnly && !!diagDef && !!episode && episode.diagnoses.length > 0 && !sameDiagnoses(reportDiagnoses!.diagnoses, episode.diagnoses)
  const stageMissing = !!episode && episode.stage === 'probatorik' && application?.kind !== 'erstantrag'
  const chiffreMissing = !patient.chiffre?.trim()

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]" data-testid="lotse-start">
      <Card className="self-start">
        <CardHeader>
          <CardTitle className="text-base">{t('lotse.kindTitle')}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <div role="radiogroup" aria-label={t('lotse.kindTitle')} className="space-y-3">
            {KINDS.map(({ kind, icon: Icon }) => {
              const selected = application?.kind === kind
              const recommended = candidate?.suggested === kind
              return (
                <button
                  key={kind}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  disabled={!selected && (!changeable || change.isPending)}
                  onClick={() => !selected && change.mutate(kind)}
                  className={cn(
                    'flex w-full items-start gap-3 rounded-lg border bg-card p-3.5 text-left transition-colors',
                    selected ? 'border-primary bg-primary/5 ring-1 ring-primary/40' : 'hover:border-primary/40 disabled:opacity-55 disabled:hover:border-border',
                  )}
                  data-testid={`lotse-kind-${kind}`}
                >
                  <span
                    className={cn(
                      'mt-1 flex size-4 shrink-0 items-center justify-center rounded-full border',
                      selected ? 'border-primary' : 'border-muted-foreground/50',
                    )}
                  >
                    {selected && <span className="size-2 rounded-full bg-primary" />}
                  </span>
                  <span className="flex size-8 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
                    <Icon className="size-4" />
                  </span>
                  <span className="min-w-0 flex-1 space-y-1.5">
                    <span className="block pt-1 text-sm font-medium">
                      {t(`enums.kind.${kind}`)}
                      {selected && kind === 'fortfuehrung' && application ? ` Nr. ${application.seq}` : ''}
                    </span>
                    {recommended && candidate && (
                      <span className="block rounded-md bg-sand px-2 py-1 text-xs text-sand-foreground" data-testid="lotse-recommended">
                        <span className="font-medium">{t('lotse.recommended')}</span> · {reason(candidate.contingent, t)}
                      </span>
                    )}
                  </span>
                </button>
              )
            })}
          </div>
          {!changeable && !readOnly && <p className="text-xs text-muted-foreground">{t('lotse.kindLocked')}</p>}
          <p className="flex items-center gap-2 pt-1 text-sm text-muted-foreground">
            <LineChart className="size-4 shrink-0" />
            {facts.length === 0
              ? t('lotse.noFactsYet')
              : since
                ? t('lotse.sinceLast', { sessions, facts: recent.length })
                : t('lotse.allFacts', { sessions, facts: recent.length })}
          </p>
          {sources?.prior && (
            <p className="flex items-center gap-2 text-sm text-muted-foreground" data-testid="lotse-prior">
              <History className="size-4 shrink-0" />
              <span>
                {t('lotse.prior', {
                  kind: sources.prior.kind ? t(`enums.kind.${sources.prior.kind}`) : t('lotse.priorBericht'),
                  date: formatDate(sources.prior.signed_at),
                })}{' '}
                <Link to="/reports/$reportId" params={{ reportId: sources.prior.report_id }} search={{ doc: true }} className="underline-offset-2 hover:underline">
                  {t('lotse.priorOpen')}
                </Link>
              </span>
            </p>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('lotse.readiness')}</CardTitle>
          <CardDescription>{t('lotse.readinessHint')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <ul className="divide-y" data-testid="lotse-readiness">
            {template.sections.map((def) => {
              const off = notRelevant.has(def.key)
              const proposal = drafts.some((d) => d.section_key === def.key && d.status === 'proposed')
              const filled = !isBlank(bodies[def.key] ?? '')
              let state: Readiness
              let label: string
              if (off) {
                state = 'none'
                label = t('lotse.notRelevant')
              } else if (def.field === 'diagnoses') {
                const set = (reportDiagnoses?.diagnoses.length ?? 0) > 0
                state = set ? 'full' : 'empty'
                label = set ? t('lotse.diagnosesSet') : t('lotse.diagnosesMissing')
              } else if (filled) {
                state = 'full'
                label = t('lotse.hasText')
              } else if (proposal) {
                state = 'half'
                label = t('lotse.proposal')
              } else {
                const n = sectionFacts(template.key, def, facts, since).length
                const own = questions.filter((x) => x.section_key === def.key)
                const open = own.filter(isOpen).length
                state = readiness(n + own.filter((x) => x.answer).length, open)
                const parts = [
                  n > 0 || open === 0 ? (n === 0 ? t('lotse.noFacts') : t('lotse.facts', { count: n })) : null,
                  open > 0 ? t('lotse.openQuestions', { count: open }) : null,
                ]
                label = parts.filter(Boolean).join(' · ')
              }
              return (
                <li key={def.key} className="flex items-center gap-3 py-2.5 text-sm" data-section={def.key} data-state={state}>
                  <span className="w-5 shrink-0 text-muted-foreground tabular-nums">{def.number}</span>
                  <span className={cn('min-w-0 flex-1 truncate', off && 'text-muted-foreground line-through')} title={def.heading}>
                    {def.heading}
                  </span>
                  {def.optional && !readOnly && (
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-7 shrink-0 px-2 text-xs text-muted-foreground"
                      title={t('lotse.notRelevantHint')}
                      onClick={() => onRelevance(def.key, !off)}
                      data-testid={`lotse-relevance-${def.key}`}
                    >
                      {off ? t('lotse.markRelevant') : t('lotse.markNotRelevant')}
                    </Button>
                  )}
                  <span className="flex w-52 shrink-0 items-center gap-2 text-xs text-muted-foreground">
                    <Dot state={state} />
                    <span className="truncate">{label}</span>
                  </span>
                </li>
              )
            })}
          </ul>
          {diagnosesDiffer && (
            <Notice text={t('lotse.diagnosesChanged')}>
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  onChange(diagDef!.key, JSON.stringify({ diagnoses: episode!.diagnoses, notes: reportDiagnoses!.notes }))
                  toast.success(t('lotse.diagnosesTaken'))
                }}
                data-testid="lotse-take-diagnoses"
              >
                {t('lotse.takeOver')}
              </Button>
            </Notice>
          )}
          {chiffreMissing && !readOnly && (
            <Notice text={t('lotse.chiffreMissing')}>
              <PatientFormDialog patient={patient} trigger={<Button size="sm" variant="outline">{t('lotse.chiffreSet')}</Button>} />
            </Notice>
          )}
          {stageMissing && !readOnly && (
            <Notice text={t('lotse.stageMissing')}>
              <Button asChild size="sm" variant="outline">
                <Link to="/patients/$patientId" params={{ patientId: patient.id }}>
                  {t('lotse.stageSet')}
                </Link>
              </Button>
            </Notice>
          )}
          {!llmActive && !readOnly && (
            <Notice text={t('lotse.noModel')}>
              <Button asChild size="sm" variant="outline">
                <Link to="/settings/ki">{t('lotse.modelSet')}</Link>
              </Button>
            </Notice>
          )}
        </CardContent>
      </Card>
    </div>
  )
}

function reason(c: { stage: string; used: number; limit: number; due: unknown }, t: (k: string, o?: Record<string, unknown>) => string) {
  if (c.stage === 'probatorik') return t('lotse.reasonProbatorik')
  const o = { stage: t(`enums.stage.${c.stage}`), used: c.used, limit: c.limit }
  return c.due ? t('lotse.reasonDue', o) : t('lotse.reasonStage', o)
}

export function Dot({ state }: { state: Readiness }) {
  if (state === 'none') return <span aria-hidden className="h-px w-2.5 shrink-0 bg-muted-foreground" />
  return (
    <span
      aria-hidden
      className={cn(
        'size-2.5 shrink-0 rounded-full border',
        state === 'full' && 'border-primary bg-primary',
        state === 'half' && 'border-warning bg-[linear-gradient(90deg,var(--color-warning)_50%,transparent_50%)]',
        state === 'empty' && 'border-muted-foreground/50',
      )}
    />
  )
}

function Notice({ text, children }: { text: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-3 rounded-md bg-warning/12 px-3 py-2.5 text-sm">
      <AlertTriangle className="size-4 shrink-0 text-warning-foreground" />
      <span className="flex-1">{text}</span>
      {children}
    </div>
  )
}
