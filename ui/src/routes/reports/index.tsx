import { useState } from 'react'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { addDays, format } from 'date-fns'
import { CalendarClock, Columns3, FilePlus2, FileText, Plus, Rows3 } from 'lucide-react'
import type { ApplicationKind } from '@/bindings/ApplicationKind'
import type { DueItem } from '@/bindings/DueItem'
import type { ReportSummary } from '@/bindings/ReportSummary'
import { Page, PageHeader } from '@/components/app-shell'
import { usePrepareApplication } from '@/components/autopilot'
import { EmptyState, FilterChip, formatDate } from '@/components/bits'
import { ReportsTable } from '@/components/reports-table'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Progress } from '@/components/ui/progress'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { PatientLabel } from '@/components/patient-label'
import { NewReportDialog } from '@/components/lotse/new-report'
import { q } from '@/lib/api'
import { BOARD_COLUMNS, boardColumn, matchesReport, noReportFilter, sortBoard, type ReportFilter } from '@/lib/queue'
import { cn, storedChoice } from '@/lib/utils'

export const Route = createFileRoute('/reports/')({ component: Reports })

type View = 'board' | 'table'
const VIEW_KEY = 'thera.reportsView'
const KINDS: ApplicationKind[] = ['erstantrag', 'umwandlung', 'fortfuehrung']

function Reports() {
  const { t } = useTranslation()
  const reports = useQuery(q.reports())
  const [view, setViewState] = useState<View>(() => storedChoice(VIEW_KEY, ['board', 'table'], 'board'))
  const [filter, setFilter] = useState<ReportFilter>(noReportFilter)
  const setView = (v: View) => {
    localStorage.setItem(VIEW_KEY, v)
    setViewState(v)
  }
  const toggle = <K extends keyof ReportFilter>(key: K, value: ReportFilter[K]) =>
    setFilter((f) => ({ ...f, [key]: f[key] === value ? null : value }))

  const all = reports.data ?? []
  const shown = sortBoard(all.filter((r) => matchesReport(r, filter)))
  const filtered = filter.method !== null || filter.kind !== null
  const methods = (['VT', 'TP', 'AP'] as const).filter((m) => all.some((r) => r.method === m))
  const kinds = KINDS.filter((k) => all.some((r) => r.kind === k))

  return (
    <>
      <PageHeader
        title={t('nav.reports')}
        actions={
          <>
            <ToggleGroup type="single" variant="outline" value={view} onValueChange={(v) => v && setView(v as View)} aria-label={t('reports.view')}>
              <ToggleGroupItem value="board" className="px-3" data-testid="reports-view-board">
                <Columns3 /> {t('reports.board')}
              </ToggleGroupItem>
              <ToggleGroupItem value="table" className="px-3" data-testid="reports-view-table">
                <Rows3 /> {t('patients.table')}
              </ToggleGroupItem>
            </ToggleGroup>
            <NewReportDialog
              trigger={
                <Button data-testid="new-report">
                  <Plus /> {t('reports.new')}
                </Button>
              }
            />
          </>
        }
      />
      <Page>
        <DueBanner />
        {reports.isSuccess && all.length === 0 ? (
          <EmptyState
            icon={FileText}
            title={t('reports.emptyTitle')}
            description={t('reports.emptyBody')}
            action={
              <Button asChild>
                <Link to="/patients">{t('reports.toPatients')}</Link>
              </Button>
            }
          />
        ) : (
          <>
            <div className="flex flex-wrap items-center gap-2" role="toolbar" aria-label={t('reports.filterLabel')}>
              <FilterChip active={!filtered} onClick={() => setFilter(noReportFilter)}>
                {t('patients.filters.all')}
              </FilterChip>
              {methods.map((m) => (
                <FilterChip key={m} active={filter.method === m} onClick={() => toggle('method', m)}>
                  {m}
                </FilterChip>
              ))}
              {kinds.map((k) => (
                <FilterChip key={k} active={filter.kind === k} onClick={() => toggle('kind', k)} data-testid={`reports-filter-${k}`}>
                  {t(`enums.kind.${k}`)}
                </FilterChip>
              ))}
            </div>
            {view === 'board' ? (
              <Board reports={shown} />
            ) : (
              <Card>
                <CardContent className="pt-2">
                  <ReportsTable reports={shown} />
                </CardContent>
              </Card>
            )}
          </>
        )}
      </Page>
    </>
  )
}

function Board({ reports }: { reports: ReportSummary[] }) {
  const { t } = useTranslation()
  return (
    // Columns keep a readable width and the board scrolls sideways when the side panel takes room.
    <div className="grid auto-cols-[minmax(14rem,1fr)] grid-flow-col gap-4 overflow-x-auto pb-2" data-testid="reports-board">
      {BOARD_COLUMNS.map((col) => {
        const cards = reports.filter((r) => boardColumn(r) === col)
        return (
          <section key={col} className="flex min-w-0 flex-col gap-3 rounded-xl bg-muted/50 p-3" data-testid={`board-${col}`}>
            <h2 className="flex items-center justify-between gap-2 px-1 text-sm font-semibold">
              <span className="min-w-0 truncate">{t(`reports.column.${col}`)}</span>
              <span className="shrink-0 text-xs font-normal text-muted-foreground tabular-nums">{cards.length}</span>
            </h2>
            {cards.length === 0 ? (
              <p className="rounded-lg border border-dashed px-3 py-6 text-center text-xs text-muted-foreground">{t('reports.columnEmpty')}</p>
            ) : (
              cards.map((r) => <ReportCard key={r.id} report={r} />)
            )}
          </section>
        )
      })}
    </div>
  )
}

function ReportCard({ report: r }: { report: ReportSummary }) {
  const { t } = useTranslation()
  const open = r.status === 'draft' || r.status === 'in_review'
  const pct = r.sections_total > 0 ? (r.sections_filled / r.sections_total) * 100 : 0
  return (
    <Link
      to="/reports/$reportId"
      params={{ reportId: r.id }}
      className="block min-w-0 overflow-hidden rounded-lg border bg-card p-3.5 shadow-xs transition-colors hover:border-primary/40 focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
      data-testid="report-card"
    >
      <div className="flex items-start justify-between gap-2">
        <PatientLabel name={r.patient_name} chiffre={r.chiffre} />
        {r.method && (
          <Badge variant="secondary" className="shrink-0">
            {r.method}
          </Badge>
        )}
      </div>
      <div className="mt-1 truncate text-sm text-muted-foreground">
        {r.kind ? t(`enums.kind.${r.kind}`) : '—'}
        {r.kind === 'fortfuehrung' && r.seq ? ` Nr. ${r.seq}` : ''}
      </div>
      {open ? (
        <>
          <Progress value={pct} className="mt-3 h-1.5" />
          <div className="mt-1.5 flex flex-wrap items-center justify-between gap-x-2 gap-y-0.5 text-xs text-muted-foreground">
            <span data-testid="report-card-progress">{t('reports.sections', { filled: r.sections_filled, total: r.sections_total })}</span>
            <Deadline report={r} />
          </div>
        </>
      ) : (
        <div className="mt-2 text-xs text-muted-foreground">
          {t(`enums.status.${r.status}`)} {formatDate(r.updated_at)}
        </div>
      )}
    </Link>
  )
}

/** "fällig ca. 12.11." from the planned session that uses up the contingent, else what's left. */
function Deadline({ report: r }: { report: ReportSummary }) {
  const { t } = useTranslation()
  if (r.sessions_left === null) return null
  const text =
    r.sessions_left <= 0
      ? t('reports.contingentUsed')
      : r.due_by
        ? t('reports.dueBy', { date: format(new Date(r.due_by), 'd.M.') })
        : t('reports.sessionsLeft', { count: r.sessions_left })
  return (
    <span
      className={cn('flex items-center gap-1', r.sessions_left <= 1 && 'font-medium text-warning-foreground')}
      data-testid="report-card-deadline"
    >
      <CalendarClock className="size-3 shrink-0" />
      {text}
    </span>
  )
}

/** Cases whose contingent is nearly used up and that have no application yet: one click starts it. */
function DueBanner() {
  // `applications_due` looks at "now" only; the range matters for the other kinds, which this ignores.
  const [now] = useState(() => new Date())
  const due = useQuery(q.dueItems(now.toISOString(), now.toISOString(), addDays(now, 1).toISOString()))
  const items = (due.data ?? []).filter((i) => i.kind === 'application_due')
  if (items.length === 0) return null
  return (
    <div className="space-y-2">
      {items.map((i) => (
        <DueRow key={i.contingent?.episode_id} item={i} />
      ))}
    </div>
  )
}

function DueRow({ item }: { item: DueItem }) {
  const { t } = useTranslation()
  const prepare = usePrepareApplication()
  const c = item.contingent
  if (!c || !item.application_kind) return null
  return (
    <div className="flex items-center gap-3 rounded-lg border border-warning/40 bg-warning/10 px-4 py-3 text-sm" data-testid="reports-due">
      <FilePlus2 className="size-4 shrink-0 text-warning-foreground" />
      <span className="flex-1">
        {t('reports.dueBanner', { kind: t(`enums.kind.${item.application_kind}`), name: item.patient_name, used: c.used, limit: c.limit })}
      </span>
      <Button
        size="sm"
        disabled={prepare.isPending}
        onClick={() => prepare.mutate({ episodeId: c.episode_id, kind: item.application_kind! })}
      >
        {t('reports.prepare')}
      </Button>
    </div>
  )
}
