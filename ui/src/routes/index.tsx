import { useEffect, useRef, useState } from 'react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { addDays, format, isToday, startOfDay, subDays } from 'date-fns'
import { de } from 'date-fns/locale'
import {
  CalendarPlus,
  ChevronRight,
  ClipboardList,
  FileCheck2,
  FilePlus2,
  FileText,
  ListTodo,
  MoreHorizontal,
  NotebookPen,
  PenLine,
  Plus,
  Sparkles,
  Sun,
  X,
} from 'lucide-react'
import type { CalendarSession } from '@/bindings/CalendarSession'
import type { DueItem } from '@/bindings/DueItem'
import { Page, PageHeader } from '@/components/app-shell'
import { EmptyState, SectionCard, StatusPill } from '@/components/bits'
import { SessionDialog, invalidateSessions, type SessionDialogMode } from '@/components/session-dialog'
import { usePrepareApplication } from '@/components/autopilot'
import { PatientLabel } from '@/components/patient-label'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { api, errorMessage, q } from '@/lib/api'
import { calendarState, isMovable, isUntouched, localDate, nextStep, sessionStart, stateTone, type FlowStep } from '@/lib/calendar'
import { cn } from '@/lib/utils'

export const Route = createFileRoute('/')({ component: Today })

/** How far back "Fällig" looks for undocumented sessions. */
const DUE_LOOKBACK_DAYS = 60
/** Shown per "Fällig" group before "… weitere". */
const DUE_SHOWN = 4

function Today() {
  const { t } = useTranslation()
  const [now] = useState(() => new Date())
  const today = startOfDay(now)
  const date = localDate(today)
  const sessions = useQuery(q.calendar(today.toISOString(), addDays(today, 1).toISOString()))
  const [dialog, setDialog] = useState<SessionDialogMode | null>(null)

  return (
    <>
      <PageHeader title={t('today.title', { date: format(today, 'EEEE d. MMMM', { locale: de }) })} subtitle={t('privacy.local')} />
      <Page className="grid grid-cols-1 gap-6 space-y-0 xl:grid-cols-[minmax(0,1fr)_22rem]">
        <section className="space-y-3" data-testid="today-timeline">
          {sessions.data?.length === 0 ? (
            <EmptyState
              icon={Sun}
              title={t('today.noSessions')}
              description={t('today.noSessionsHint')}
              action={
                <Button variant="outline" onClick={() => setDialog({ kind: 'create' })}>
                  <CalendarPlus /> {t('calendar.newSession')}
                </Button>
              }
            />
          ) : (
            <ol className="relative space-y-3 before:absolute before:top-3 before:bottom-3 before:left-[4.78rem] before:w-px before:bg-border">
              {sessions.data?.map((s) => (
                <TimelineRow key={s.id} s={s} onEdit={() => setDialog({ kind: 'edit', session: s, documented: !isUntouched(s) })} />
              ))}
            </ol>
          )}
          {!!sessions.data?.length && (
            <Button variant="ghost" size="sm" className="ml-20" onClick={() => setDialog({ kind: 'create' })}>
              <Plus /> {t('calendar.newSession')}
            </Button>
          )}
        </section>
        <aside className="space-y-6">
          <TasksCard date={date} />
          <DueCard today={today} />
          <DayNoteCard date={date} />
        </aside>
      </Page>
      <SessionDialog mode={dialog} onOpenChange={(o) => !o && setDialog(null)} />
    </>
  )
}

const stepAction: Record<FlowStep, string> = {
  prepare: 'today.step.prepare',
  capture: 'today.step.capture',
  review: 'today.step.review',
  note: 'today.step.note',
  finish: 'today.step.finish',
}

function TimelineRow({ s, onEdit }: { s: CalendarSession; onEdit: () => void }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const state = calendarState(s)
  const step = nextStep(s)
  const start = sessionStart(s)
  const open = () => void navigate({ to: '/sessions/$sessionId', params: { sessionId: s.id } })
  const cancel = useMutation({
    mutationFn: () => api.cancelSession(s.id, null),
    onSuccess: () => invalidateSessions(qc),
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <li className="flex items-center gap-4" data-testid="today-session" data-session={s.id}>
      <span className="w-14 shrink-0 text-right text-sm text-muted-foreground tabular-nums">{start ? format(start, 'HH:mm') : '—'}</span>
      <span className={cn('relative z-10 size-2.5 shrink-0 rounded-full', state === 'cancelled' ? 'bg-border' : 'bg-primary')} />
      <div
        className={cn(
          'flex min-w-0 flex-1 items-center gap-4 rounded-lg border bg-card px-4 py-3 shadow-xs',
          state === 'cancelled' && 'opacity-60',
        )}
      >
        <button type="button" onClick={open} className="min-w-0 flex-1 text-left">
          <div className={cn('font-mono text-sm font-medium', state === 'cancelled' && 'line-through')}>{s.chiffre ?? s.patient_name}</div>
          <div className="truncate text-xs text-muted-foreground">
            {state !== 'cancelled' && `${t('calendar.sessionNo', { n: s.number })} · `}
            {s.patient_name}
          </div>
        </button>
        <StatusPill tone={stateTone[state]}>{t(`calendar.state.${state}`)}</StatusPill>
        {step ? (
          <Button size="sm" variant="outline" onClick={open} className="w-36" data-testid="today-next">
            {t(stepAction[step])}
          </Button>
        ) : (
          <span className="w-36" />
        )}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon" className="size-8 text-muted-foreground" aria-label={t('today.more')}>
              <MoreHorizontal />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onSelect={open}>{t('common.open')}</DropdownMenuItem>
            {(isMovable(s) || s.status === 'cancelled') && (
              <DropdownMenuItem onSelect={onEdit}>{s.status === 'cancelled' ? t('common.edit') : t('calendar.reschedule')}</DropdownMenuItem>
            )}
            {isMovable(s) && isUntouched(s) && (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="destructive" onSelect={() => cancel.mutate()}>
                  {t('calendar.cancelSession')}
                </DropdownMenuItem>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </li>
  )
}

function TasksCard({ date }: { date: string }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const tasks = useQuery(q.tasks(date, date, true))
  const [title, setTitle] = useState('')
  const refresh = () => void qc.invalidateQueries({ queryKey: ['tasks'] })
  const add = useMutation({
    mutationFn: () => api.createTask({ date, time: null, title, patient_id: null, session_id: null }),
    onSuccess: () => {
      setTitle('')
      refresh()
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const toggle = useMutation({
    mutationFn: ({ id, done }: { id: string; done: boolean }) => api.setTaskDone(id, done),
    onSuccess: refresh,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const remove = useMutation({
    mutationFn: api.deleteTask,
    onSuccess: refresh,
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <SectionCard icon={ListTodo} title={t('today.tasks')} contentClassName="space-y-1" className="gap-3">
      <ul className="space-y-0.5" data-testid="today-tasks">
        {tasks.data?.map((task) => (
          <li key={task.id} className="group flex items-center gap-3 rounded-md py-1 text-sm">
            <Checkbox checked={task.done} onCheckedChange={(c) => toggle.mutate({ id: task.id, done: c === true })} aria-label={task.title} />
            <span className={cn('min-w-0 flex-1 truncate', task.done && 'text-muted-foreground line-through')}>
              {task.title}
              {task.date < date && <span className="ml-2 text-xs text-warning-foreground">{t('today.taskFrom', { date: format(new Date(`${task.date}T00:00:00`), 'd.M.') })}</span>}
            </span>
            <Button
              variant="ghost"
              size="icon"
              className="size-7 text-muted-foreground opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
              onClick={() => remove.mutate(task.id)}
              aria-label={t('common.delete')}
            >
              <X />
            </Button>
          </li>
        ))}
      </ul>
      <form
        className="flex gap-2 pt-1"
        onSubmit={(e) => {
          e.preventDefault()
          if (title.trim()) add.mutate()
        }}
      >
        <Input value={title} onChange={(e) => setTitle(e.target.value)} placeholder={t('today.addTask')} className="h-8" data-testid="task-input" />
        <Button type="submit" size="icon" variant="outline" className="size-8" disabled={!title.trim() || add.isPending} aria-label={t('today.addTask')}>
          <Plus />
        </Button>
      </form>
    </SectionCard>
  )
}

function DueCard({ today }: { today: Date }) {
  const { t } = useTranslation()
  const due = useQuery(
    q.dueItems(subDays(today, DUE_LOOKBACK_DAYS).toISOString(), today.toISOString(), addDays(today, 2).toISOString()),
  )
  // Today's sessions are on the timeline with their own "Vorbereiten" already.
  const items = (due.data ?? []).filter((i) => !(i.kind === 'prep_missing' && i.at && isToday(new Date(i.at))))
  const groups: { kind: DueItem['kind']; icon: typeof PenLine; items: DueItem[] }[] = [
    { kind: 'transcript_review', icon: FileCheck2, items: items.filter((i) => i.kind === 'transcript_review') },
    { kind: 'application_due', icon: FilePlus2, items: items.filter((i) => i.kind === 'application_due') },
    { kind: 'note_open', icon: PenLine, items: items.filter((i) => i.kind === 'note_open') },
    { kind: 'report_draft', icon: FileText, items: items.filter((i) => i.kind === 'report_draft') },
    { kind: 'prep_missing', icon: ClipboardList, items: items.filter((i) => i.kind === 'prep_missing') },
  ]
  return (
    <SectionCard icon={NotebookPen} title={t('today.due')} className="gap-3" contentClassName="space-y-4">
      {items.length === 0 && <p className="text-sm text-muted-foreground">{t('today.nothingDue')}</p>}
      {groups
        .filter((g) => g.items.length > 0)
        .map((g) => (
          <div key={g.kind} data-testid={`due-${g.kind}`}>
            <div className="mb-1 flex items-center gap-2 text-sm font-medium">
              <span className="flex size-7 items-center justify-center rounded-full bg-warning/15 text-warning-foreground">
                <g.icon className="size-3.5" />
              </span>
              {t(`today.dueKind.${g.kind}`, { count: g.items.length })}
            </div>
            <ul className="space-y-0.5 pl-9">
              {g.items.slice(0, DUE_SHOWN).map((i) => (
                <li key={`${i.kind}-${i.session_id ?? i.report_id ?? i.contingent?.episode_id}`}>
                  {i.kind === 'application_due' ? <ApplicationDueRow item={i} /> : <DueLink item={i} />}
                </li>
              ))}
              {g.items.length > DUE_SHOWN && (
                <li className="text-xs text-muted-foreground">{t('calendar.more', { count: g.items.length - DUE_SHOWN })}</li>
              )}
            </ul>
          </div>
        ))}
    </SectionCard>
  )
}

/** The contingent is nearly used up: one click creates the application and its Bericht. */
function ApplicationDueRow({ item }: { item: DueItem }) {
  const { t } = useTranslation()
  const prepare = usePrepareApplication()
  const c = item.contingent
  if (!c || !item.application_kind) return null
  return (
    <div className="flex flex-wrap items-center gap-x-2 gap-y-1 px-1 py-0.5 text-sm" data-testid="application-due">
      <Link to="/patients/$patientId" params={{ patientId: item.patient_id }} className="min-w-0 hover:underline">
        <PatientLabel name={item.patient_name} chiffre={item.chiffre} />
      </Link>
      <span className="min-w-40 flex-1 text-muted-foreground">
        {t('today.applicationDue', { kind: t(`enums.kind.${item.application_kind}`), used: c.used, limit: c.limit })}
      </span>
      <Button
        size="sm"
        variant="outline"
        className="h-7"
        disabled={prepare.isPending}
        onClick={() => prepare.mutate({ episodeId: c.episode_id, kind: item.application_kind! })}
      >
        {t('today.prepareApplication')}
      </Button>
    </div>
  )
}

function DueLink({ item }: { item: DueItem }) {
  const { t } = useTranslation()
  const when = item.at ? format(new Date(item.at), item.kind === 'report_draft' ? 'd.M.' : 'EEE d.M., HH:mm', { locale: de }) : ''
  const label =
    item.kind === 'report_draft'
      ? `${t(`enums.kind.${item.application_kind}`)}${item.application_kind === 'fortfuehrung' && item.seq ? ` ${item.seq}` : ''}`
      : when
  const cls = 'group flex items-center gap-2 rounded px-1 py-1 text-sm hover:bg-foreground/5'
  const body = (
    <>
      <PatientLabel name={item.patient_name} chiffre={item.chiffre} className="flex-1" />
      <span className="shrink-0 text-xs text-muted-foreground">{label}</span>
      <ChevronRight className="size-3.5 text-muted-foreground opacity-0 group-hover:opacity-100" />
    </>
  )
  return item.session_id ? (
    <Link to="/sessions/$sessionId" params={{ sessionId: item.session_id }} className={cls}>
      {body}
    </Link>
  ) : (
    <Link to="/reports/$reportId" params={{ reportId: item.report_id! }} className={cls}>
      {body}
    </Link>
  )
}

/** Saves while typing (debounced) and when leaving the field. */
function DayNoteCard({ date }: { date: string }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const note = useQuery(q.dayNote(date))
  const [body, setBody] = useState<string | null>(null)
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined)
  const save = useMutation({
    mutationFn: (text: string) => api.setDayNote(date, text),
    onSuccess: (_, text) => qc.setQueryData(q.dayNote(date).queryKey, text.trim() ? text : null),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const value = body ?? note.data ?? ''
  const flush = () => {
    clearTimeout(timer.current)
    if (body !== null && body !== (note.data ?? '')) save.mutate(body)
  }
  useEffect(() => () => clearTimeout(timer.current), [])
  return (
    <SectionCard
      icon={Sun}
      title={t('today.dayNote')}
      className="gap-3"
      contentClassName="space-y-3"
      actions={
        <Tooltip>
          <TooltipTrigger asChild>
            {/* Opens the agent with a day brief once it exists (slice 25). */}
            <span>
              <Button variant="outline" size="sm" disabled>
                <Sparkles /> {t('today.prepareDay')}
              </Button>
            </span>
          </TooltipTrigger>
          <TooltipContent>{t('today.prepareDayHint')}</TooltipContent>
        </Tooltip>
      }
    >
      <Textarea
        value={value}
        onChange={(e) => {
          const text = e.target.value
          setBody(text)
          clearTimeout(timer.current)
          timer.current = setTimeout(() => save.mutate(text), 800)
        }}
        onBlur={flush}
        placeholder={t('today.dayNotePlaceholder')}
        className="min-h-28 resize-y"
        data-testid="day-note"
      />
      <p className="h-4 text-xs text-muted-foreground">{save.isPending ? t('common.loading') : save.isSuccess ? t('common.saved') : ''}</p>
    </SectionCard>
  )
}
