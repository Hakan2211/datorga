import { useState } from 'react'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { format, isToday } from 'date-fns'
import { de } from 'date-fns/locale'
import { CalendarDays, ListChecks, Search } from 'lucide-react'
import type { QueueSession } from '@/bindings/QueueSession'
import { Page, PageHeader } from '@/components/app-shell'
import { EmptyState, FilterChip, type StepState } from '@/components/bits'
import { PatientAvatar } from '@/components/patient-avatar'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { q } from '@/lib/api'
import { calendarFlow, flowStates, SESSION_FLOW } from '@/lib/calendar'
import { groupSessions, matchesPatient, SESSION_TODOS, sessionTime, sessionTodo, todoCounts, type DayGroup, type SessionTodo } from '@/lib/queue'
import { cn } from '@/lib/utils'

export const Route = createFileRoute('/sessions/')({ component: Sessions })

type Filter = SessionTodo | 'all'
/** Older sessions shown before "Mehr anzeigen"; the list grows with the practice. */
const OLDER_SHOWN = 30

function Sessions() {
  const { t } = useTranslation()
  const queue = useQuery(q.sessionQueue())
  const [filter, setFilter] = useState<Filter>('all')
  const [query, setQuery] = useState('')
  const [olderShown, setOlderShown] = useState(OLDER_SHOWN)
  // Re-read on every refetch, so a session slides from "Diese Woche" to "Heute" on its day.
  const now = new Date(queue.dataUpdatedAt || Date.now())

  const all = queue.data ?? []
  const searched = all.filter((s) => matchesPatient(s, query))
  const counts = todoCounts(searched, now)
  const shown = filter === 'all' ? searched : searched.filter((s) => sessionTodo(s, now) === filter)
  const groups = groupSessions(shown, now)

  return (
    <>
      <PageHeader
        title={t('sessions.title')}
        subtitle={t('sessions.subtitle')}
        actions={
          <div className="relative w-64">
            <Search className="absolute top-2.5 left-3 size-4 text-muted-foreground" />
            <Input
              className="pl-9"
              placeholder={t('patients.searchPlaceholder')}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              data-testid="sessions-search"
            />
          </div>
        }
      />
      <Page>
        {queue.isSuccess && all.length === 0 ? (
          <EmptyState
            icon={CalendarDays}
            title={t('sessions.emptyTitle')}
            description={t('sessions.emptyBody')}
            action={
              <Button asChild>
                <Link to="/calendar" search={{ view: 'week' }}>{t('sessions.toCalendar')}</Link>
              </Button>
            }
          />
        ) : (
          <>
            <div className="flex flex-wrap items-center gap-2" role="toolbar" aria-label={t('sessions.filterLabel')}>
              {SESSION_TODOS.map((k) => (
                <FilterChip
                  key={k}
                  active={filter === k}
                  count={counts[k]}
                  onClick={() => setFilter(filter === k ? 'all' : k)}
                  data-testid={`sessions-filter-${k}`}
                >
                  {t(`sessions.todo.${k}`)}
                </FilterChip>
              ))}
              <FilterChip active={filter === 'all'} onClick={() => setFilter('all')} data-testid="sessions-filter-all">
                {t('patients.filters.all')}
              </FilterChip>
            </div>

            {queue.isSuccess && shown.length === 0 ? (
              <EmptyState
                icon={ListChecks}
                title={filter === 'all' ? t('common.noMatch') : t('sessions.nothingTodo')}
                description={filter === 'all' ? undefined : t('sessions.nothingTodoBody')}
                action={
                  filter !== 'all' || query ? (
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => {
                        setFilter('all')
                        setQuery('')
                      }}
                    >
                      {t('sessions.showAll')}
                    </Button>
                  ) : undefined
                }
              />
            ) : (
              <Card className="gap-5 px-5 py-5" data-testid="session-queue">
                {groups.map((g) => {
                  const rows = g.group === 'older' ? g.sessions.slice(0, olderShown) : g.sessions
                  return (
                    <section key={g.group} data-testid={`sessions-group-${g.group}`}>
                      <h2 className="mb-2 text-sm font-semibold">{t(`sessions.group.${g.group}`)}</h2>
                      <ul className="divide-y rounded-lg border">
                        {rows.map((s) => (
                          <QueueRow key={s.id} session={s} group={g.group} now={now} />
                        ))}
                      </ul>
                      {rows.length < g.sessions.length && (
                        <Button variant="ghost" size="sm" className="mt-2" onClick={() => setOlderShown((n) => n + OLDER_SHOWN)}>
                          {t('sessions.showMore', { count: g.sessions.length - rows.length })}
                        </Button>
                      )}
                    </section>
                  )
                })}
                <p className="text-center text-sm text-muted-foreground">{t('sessions.count', { count: shown.length })}</p>
              </Card>
            )}
          </>
        )}
      </Page>
    </>
  )
}

/** "09:00" today, "Di 10:30" this week, a date otherwise. */
function when(s: QueueSession, group: DayGroup): string {
  const at = sessionTime(s)
  if (!at) return '—'
  if (group === 'today' || isToday(at)) return format(at, 'HH:mm')
  if (group === 'week') return format(at, 'EEEEEE HH:mm', { locale: de })
  return format(at, 'dd.MM.yyyy')
}

function QueueRow({ session: s, group, now }: { session: QueueSession; group: DayGroup; now: Date }) {
  const { t } = useTranslation()
  const cancelled = s.status === 'cancelled'
  const todo = sessionTodo(s, now)
  const states = flowStates(calendarFlow(s))
  const label = cancelled ? t('sessions.status.cancelled') : todo ? t(`sessions.todo.${todo}`) : null
  return (
    <li className={cn('flex items-center gap-4 px-4 py-2.5', cancelled && 'opacity-60')} data-testid="session-row" data-todo={todo ?? ''}>
      <PatientAvatar patientId={s.patient_id} hasPhoto={s.has_photo} firstName={s.first_name} lastName={s.last_name} className="size-8 text-xs" />
      <Link to="/patients/$patientId" params={{ patientId: s.patient_id }} className="w-44 shrink-0 truncate text-sm font-medium hover:underline">
        {s.patient_name}
      </Link>
      <span className="w-24 shrink-0 text-sm text-muted-foreground tabular-nums">{when(s, group)}</span>
      <FlowDots states={SESSION_FLOW.map((k) => ({ key: k, state: states[k] }))} muted={cancelled} />
      <span className="min-w-0 flex-1 truncate text-sm text-muted-foreground" title={s.summary_head ?? undefined} data-testid="session-row-summary">
        {[label, s.summary_head].filter(Boolean).join(' • ')}
      </span>
      <Button asChild size="sm" variant="outline" className="shrink-0">
        <Link to="/sessions/$sessionId" params={{ sessionId: s.id }}>
          {t('sessions.continue')}
        </Link>
      </Button>
    </li>
  )
}

/** The session stepper in small: one dot per step, filled when done, ringed where work continues. */
function FlowDots({ states, muted }: { states: { key: string; state: StepState }[]; muted: boolean }) {
  const { t } = useTranslation()
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="flex shrink-0 items-center gap-1.5" data-testid="flow-dots">
          {states.map((s) => (
            <span
              key={s.key}
              data-state={s.state}
              className={cn(
                'size-2.5 rounded-full border',
                muted
                  ? 'border-muted-foreground/40'
                  : s.state === 'done'
                    ? 'border-primary bg-primary'
                    : s.state === 'current'
                      ? 'border-primary ring-2 ring-primary/20'
                      : 'border-muted-foreground/40',
              )}
            />
          ))}
        </span>
      </TooltipTrigger>
      <TooltipContent className="whitespace-pre-line">
        {states.map((s) => `${s.state === 'done' ? '✓' : s.state === 'current' ? '→' : '·'} ${t(`today.step.${s.key}`)}`).join('\n')}
      </TooltipContent>
    </Tooltip>
  )
}
