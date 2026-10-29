import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { save } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import { addMonths, format, isSameDay, isSameMonth, isWeekend } from 'date-fns'
import { de } from 'date-fns/locale'
import { Check, ChevronLeft, ChevronRight, Download, FileText, Plus, Video } from 'lucide-react'
import type { CalendarSession } from '@/bindings/CalendarSession'
import { Page } from '@/components/app-shell'
import { StatusPill } from '@/components/bits'
import { SessionDialog, invalidateSessions, type SessionDialogMode } from '@/components/session-dialog'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { api, errorMessage, q } from '@/lib/api'
import {
  atMinute,
  calendarState,
  isMovable,
  isUntouched,
  layoutDay,
  localDate,
  minuteOfDay,
  minutesOf,
  monthDays,
  parseLocalDate,
  sessionStart,
  snap,
  SNAP_MINUTES,
  stateTone,
  stepView,
  viewRange,
  viewTitle,
  type CalendarState,
  type CalendarView,
} from '@/lib/calendar'
import { cn } from '@/lib/utils'

type Search = { view: CalendarView; date?: string }

export const Route = createFileRoute('/calendar')({
  validateSearch: (s: Record<string, unknown>): Search => ({
    view: s.view === 'day' || s.view === 'month' ? s.view : 'week',
    date: typeof s.date === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(s.date) ? s.date : undefined,
  }),
  component: CalendarPage,
})

const HOUR_PX = 56
const DAY_START = 7
const DAY_END = 20
/** How far the .ics export reaches from the start of the view. */
const EXPORT_MONTHS = 3

function CalendarPage() {
  const { t } = useTranslation()
  const { view, date } = Route.useSearch()
  const navigate = useNavigate({ from: '/calendar' })
  const anchor = date ? parseLocalDate(date) : new Date()
  const { from, to } = viewRange(view, anchor)
  const data = useQuery(q.calendar(from.toISOString(), to.toISOString()))
  const sessions = data.data ?? []
  const [dialog, setDialog] = useState<SessionDialogMode | null>(null)

  const go = (search: Partial<Search>) => void navigate({ search: (s) => ({ ...s, ...search }) })

  // Mo–Fr, plus the weekend days that have sessions.
  const days =
    view === 'day'
      ? [anchor]
      : Array.from({ length: 7 }, (_, i) => new Date(from.getFullYear(), from.getMonth(), from.getDate() + i)).filter(
          (d) => !isWeekend(d) || sessions.some((s) => isSameDay(sessionStart(s) ?? 0, d)),
        )

  const exportIcs = useMutation({
    mutationFn: async () => {
      const path = await save({
        defaultPath: `thera-Termine-${localDate(from)}.ics`,
        filters: [{ name: 'iCalendar', extensions: ['ics'] }],
      })
      if (!path) return null
      return api.exportIcs(from.toISOString(), addMonths(from, EXPORT_MONTHS).toISOString(), path)
    },
    onSuccess: (n) => n !== null && toast.success(t('calendar.exported', { count: n })),
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <div className="flex min-h-full flex-col">
      <div className="flex items-center gap-3 border-b px-8 py-4">
        <div className="flex items-center gap-1">
          <Button variant="outline" size="icon" onClick={() => go({ date: localDate(stepView(view, anchor, -1)) })} aria-label={t('calendar.prev')}>
            <ChevronLeft />
          </Button>
          <Button variant="outline" size="icon" onClick={() => go({ date: localDate(stepView(view, anchor, 1)) })} aria-label={t('calendar.next')}>
            <ChevronRight />
          </Button>
          <Button variant="outline" className="ml-1" onClick={() => go({ date: undefined })}>
            {t('calendar.today')}
          </Button>
        </div>
        <h1 className="flex-1 text-center font-serif text-xl" data-testid="calendar-title">
          {viewTitle(view, anchor, days)}
        </h1>
        <ToggleGroup
          type="single"
          variant="outline"
          value={view}
          onValueChange={(v) => v && go({ view: v as CalendarView })}
          data-testid="calendar-view"
        >
          <ToggleGroupItem value="day" className="px-3">
            {t('calendar.day')}
          </ToggleGroupItem>
          <ToggleGroupItem value="week" className="px-3">
            {t('calendar.week')}
          </ToggleGroupItem>
          <ToggleGroupItem value="month" className="px-3">
            {t('calendar.month')}
          </ToggleGroupItem>
        </ToggleGroup>
        <Button
          variant="ghost"
          size="icon"
          onClick={() => exportIcs.mutate()}
          disabled={exportIcs.isPending}
          title={t('calendar.exportHint', { months: EXPORT_MONTHS })}
          aria-label={t('calendar.export')}
        >
          <Download />
        </Button>
        <Button onClick={() => setDialog({ kind: 'create', at: undefined })} data-testid="calendar-new">
          <Plus /> {t('calendar.newSession')}
        </Button>
      </div>
      <Page className="flex-1 space-y-3 py-4">
        {view === 'month' ? (
          <MonthGrid anchor={anchor} sessions={sessions} onDay={(d) => go({ view: 'day', date: localDate(d) })} />
        ) : (
          <TimeGrid
            days={days}
            sessions={sessions}
            onCreate={(at) => setDialog({ kind: 'create', at })}
            onEdit={(s) => setDialog({ kind: 'edit', session: s, documented: !isUntouched(s) })}
          />
        )}
        <Legend />
      </Page>
      <SessionDialog mode={dialog} onOpenChange={(o) => !o && setDialog(null)} />
    </div>
  )
}

function Legend() {
  const { t } = useTranslation()
  const states: CalendarState[] = ['planned', 'held', 'note_open', 'signed', 'cancelled']
  return (
    <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
      {states.map((s) => (
        <StatusPill key={s} tone={stateTone[s]}>
          {t(`calendar.state.${s}`)}
        </StatusPill>
      ))}
      <span className="ml-2">{t('calendar.dragHint')}</span>
    </div>
  )
}

/** A session being dragged: the block stays in its own column (pointer capture) and is shifted by `dx`. */
type Drag = { id: string; dayIdx: number; dx: number; minute: number; moved: boolean }

function TimeGrid({
  days,
  sessions,
  onCreate,
  onEdit,
}: {
  days: Date[]
  sessions: CalendarSession[]
  onCreate: (at: Date) => void
  onEdit: (s: CalendarSession) => void
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const columns = useRef<(HTMLDivElement | null)[]>([])
  const scroller = useRef<HTMLDivElement>(null)
  const [drag, setDrag] = useState<Drag | null>(null)
  const [openId, setOpenId] = useState<string | null>(null)
  const draggedAt = useRef(0)
  const now = useNow()

  const placed = sessions
    .map((s) => ({ s, start: sessionStart(s) }))
    .filter((x): x is { s: CalendarSession; start: Date } => !!x.start)
  const startHour = Math.min(DAY_START, ...placed.map((p) => p.start.getHours()))
  const endHour = Math.max(DAY_END, ...placed.map((p) => Math.ceil((minuteOfDay(p.start) + minutesOf(p.s)) / 60)))
  const top = (minute: number) => ((minute - startHour * 60) / 60) * HOUR_PX

  // Start the view at 07:00 (or the first session), not at midnight.
  useEffect(() => {
    scroller.current?.scrollTo({ top: 0 })
  }, [days.length])

  const move = useMutation({
    mutationFn: ({ s, at }: { s: CalendarSession; at: Date }) =>
      api.updateSession(s.id, { scheduled_at: at.toISOString(), planned_minutes: s.planned_minutes, modality: s.modality }),
    // Keep the block where it was dropped until the new data is in.
    onSuccess: async () => {
      invalidateSessions(qc)
      await qc.refetchQueries({ queryKey: ['calendar'] })
    },
    onError: (e) => toast.error(errorMessage(e)),
    onSettled: () => setDrag(null),
  })

  // Window listeners bound to the one session under the pointer: a block never sees another's drag.
  const pointerDown = (e: ReactPointerEvent, s: CalendarSession, dayIdx: number, origMinute: number) => {
    if (!isMovable(s) || e.button !== 0 || move.isPending) return
    const [x0, y0] = [e.clientX, e.clientY]
    const left = (i: number) => columns.current[i]?.getBoundingClientRect().left ?? 0
    let cur: Drag = { id: s.id, dayIdx, dx: 0, minute: origMinute, moved: false }
    const onMove = (ev: PointerEvent) => {
      const dy = ev.clientY - y0
      if (!cur.moved && Math.abs(dy) <= 4 && Math.abs(ev.clientX - x0) <= 4) return
      const col = columns.current.findIndex((c) => {
        const r = c?.getBoundingClientRect()
        return !!r && ev.clientX >= r.left && ev.clientX < r.right
      })
      const to = col < 0 ? cur.dayIdx : col
      const minute = Math.max(0, Math.min(24 * 60 - SNAP_MINUTES, snap(origMinute + (dy / HOUR_PX) * 60)))
      cur = { ...cur, moved: true, dayIdx: to, dx: left(to) - left(dayIdx), minute }
      setDrag(cur)
    }
    const onUp = () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      if (cur.moved) draggedAt.current = performance.now()
      if (cur.moved && (cur.dayIdx !== dayIdx || cur.minute !== origMinute)) {
        move.mutate({ s, at: atMinute(days[cur.dayIdx], cur.minute) })
      } else {
        setDrag(null)
      }
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }
  // The click that ends a drag (on the block or, past its edge, on the column) does nothing.
  const afterDrag = () => performance.now() - draggedAt.current < 300

  return (
    <div className="overflow-hidden rounded-lg border bg-card" data-testid="time-grid">
      <div className="flex border-b">
        <div className="w-14 shrink-0" />
        {days.map((d) => (
          <div key={d.toISOString()} className={cn('flex-1 border-l py-2.5 text-center text-sm', isSameDay(d, now) && 'bg-accent/40')}>
            <span className="text-muted-foreground">{format(d, 'EEEEEE', { locale: de })}. </span>
            <span
              className={cn(
                'inline-flex min-w-7 items-center justify-center rounded-full px-1 font-medium',
                isSameDay(d, now) && 'bg-primary text-primary-foreground',
              )}
            >
              {format(d, 'dd.MM.')}
            </span>
          </div>
        ))}
      </div>
      <div ref={scroller} className="max-h-[calc(100vh-15rem)] overflow-y-auto">
        {/* pt-2 keeps the first hour label visible above the first line. */}
        <div className="relative flex pt-2" style={{ height: (endHour - startHour) * HOUR_PX + 8 }}>
          <div className="w-14 shrink-0">
            {Array.from({ length: endHour - startHour }, (_, i) => (
              <div key={i} className="relative text-right text-xs text-muted-foreground tabular-nums" style={{ height: HOUR_PX }}>
                <span className="absolute -top-2 right-2">{String(startHour + i).padStart(2, '0')}:00</span>
              </div>
            ))}
          </div>
          {days.map((day, dayIdx) => {
            const here = placed.filter(({ start }) => isSameDay(start, day))
            const layout = layoutDay(
              here.map(({ s, start }) => {
                const m = drag?.id === s.id ? drag.minute : minuteOfDay(start)
                return { id: s.id, start: m, end: m + minutesOf(s) }
              }),
            )
            return (
              <div
                key={day.toISOString()}
                ref={(el) => {
                  columns.current[dayIdx] = el
                }}
                className={cn('relative flex-1 cursor-copy border-l', isSameDay(day, now) && 'bg-accent/40')}
                data-day={localDate(day)}
                onClick={(e) => {
                  if (afterDrag()) return
                  const y = e.clientY - e.currentTarget.getBoundingClientRect().top
                  onCreate(atMinute(day, startHour * 60 + Math.floor(((y / HOUR_PX) * 60) / 15) * 15))
                }}
              >
                {Array.from({ length: endHour - startHour }, (_, i) => (
                  <div key={i} className="pointer-events-none border-b border-border/60" style={{ height: HOUR_PX }} />
                ))}
                {isSameDay(day, now) && minuteOfDay(now) >= startHour * 60 && (
                  <div className="pointer-events-none absolute inset-x-0 z-20 h-0.5 bg-destructive/70" style={{ top: top(minuteOfDay(now)) }} />
                )}
                {here.map(({ s, start }) => {
                  const origDay = dayIdx
                  const dragging = drag?.id === s.id && drag.moved
                  const origMinute = minuteOfDay(start)
                  const minute = drag?.id === s.id ? drag.minute : origMinute
                  const pos = layout.get(s.id) ?? { col: 0, cols: 1 }
                  return (
                    <Popover key={s.id} open={openId === s.id} onOpenChange={(o) => setOpenId(o ? s.id : null)}>
                      <PopoverTrigger asChild>
                        <div
                          role="button"
                          tabIndex={0}
                          data-testid="cal-event"
                          data-session={s.id}
                          onPointerDown={(e) => pointerDown(e, s, origDay, origMinute)}
                          onClick={(e) => {
                            e.stopPropagation()
                            // Radix skips its toggle for a prevented click: no popover after a drag.
                            if (afterDrag()) e.preventDefault()
                          }}
                          className="absolute z-10 px-0.5"
                          style={{
                            top: top(minute),
                            height: Math.max(22, (minutesOf(s) / 60) * HOUR_PX - 2),
                            // While dragging the block takes the full width of the target column.
                            left: dragging ? 0 : `${(pos.col / pos.cols) * 100}%`,
                            width: dragging ? '100%' : `${100 / pos.cols}%`,
                            transform: dragging ? `translateX(${drag?.dx ?? 0}px)` : undefined,
                            zIndex: dragging ? 30 : undefined,
                          }}
                        >
                          <EventBlock s={s} minute={minute} dragging={dragging} />
                        </div>
                      </PopoverTrigger>
                      <PopoverContent className="w-72" align="start" onClick={(e) => e.stopPropagation()}>
                        <EventDetails
                          s={s}
                          onEdit={() => {
                            setOpenId(null)
                            onEdit(s)
                          }}
                        />
                      </PopoverContent>
                    </Popover>
                  )
                })}
              </div>
            )
          })}
        </div>
      </div>
      {move.isPending && <div className="border-t px-4 py-1.5 text-xs text-muted-foreground">{t('calendar.moving')}</div>}
    </div>
  )
}

const blockStyle: Record<CalendarState, string> = {
  planned: 'border-primary/50 bg-card text-primary',
  running: 'border-destructive bg-destructive/10 text-destructive',
  held: 'border-primary/50 bg-card text-primary',
  note_open: 'border-primary/50 bg-card text-primary',
  signed: 'border-primary bg-primary text-primary-foreground',
  cancelled: 'border-dashed border-border bg-muted/60 text-muted-foreground line-through',
}

function EventBlock({ s, minute, dragging }: { s: CalendarSession; minute: number; dragging: boolean }) {
  const state = calendarState(s)
  const time = `${String(Math.floor(minute / 60)).padStart(2, '0')}:${String(minute % 60).padStart(2, '0')}`
  return (
    <div
      data-state={state}
      className={cn(
        'relative flex h-full flex-col overflow-hidden rounded-md border px-2 py-1 text-xs leading-tight shadow-xs select-none',
        blockStyle[state],
        isMovable(s) ? 'cursor-grab' : 'cursor-pointer',
        dragging && 'cursor-grabbing opacity-80 ring-2 ring-primary/40',
      )}
    >
      <span className="font-medium tabular-nums">{time}</span>
      <span className="truncate font-mono">{s.chiffre ?? s.patient_name}</span>
      <span className="absolute top-1.5 right-1.5 flex items-center gap-1">
        {s.modality === 'video' && <Video className="size-3" />}
        {state === 'signed' && <Check className="size-3.5" />}
        {(state === 'held' || state === 'note_open') && <span className="size-2 rounded-full bg-warning" />}
        {state === 'running' && <span className="size-2 animate-pulse rounded-full bg-destructive" />}
      </span>
    </div>
  )
}

function EventDetails({ s, onEdit }: { s: CalendarSession; onEdit: () => void }) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const state = calendarState(s)
  const start = sessionStart(s)
  return (
    <div className="space-y-3 text-sm" data-testid="cal-details">
      <div>
        <div className="font-medium">{s.patient_name}</div>
        <div className="text-muted-foreground">
          {s.chiffre && <span className="font-mono">{s.chiffre} · </span>}
          {state !== 'cancelled' && `${t('calendar.sessionNo', { n: s.number })} · `}
          {t('calendar.minutes', { count: minutesOf(s) })}
        </div>
        {start && <div className="text-muted-foreground">{format(start, "EEEE, d. MMMM, HH:mm 'Uhr'", { locale: de })}</div>}
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <StatusPill tone={stateTone[state]}>{t(`calendar.state.${state}`)}</StatusPill>
        {s.has_prep && (
          <span className="flex items-center gap-1 text-xs text-muted-foreground">
            <FileText className="size-3.5" /> {t('calendar.hasPrep')}
          </span>
        )}
      </div>
      {s.cancelled_reason && <p className="text-muted-foreground">„{s.cancelled_reason}“</p>}
      <div className="flex gap-2">
        <Button size="sm" onClick={() => void navigate({ to: '/sessions/$sessionId', params: { sessionId: s.id } })}>
          {t('common.open')}
        </Button>
        {(isMovable(s) || s.status === 'cancelled') && (
          <Button size="sm" variant="outline" onClick={onEdit} data-testid="cal-edit">
            {s.status === 'cancelled' ? t('common.edit') : t('calendar.reschedule')}
          </Button>
        )}
      </div>
    </div>
  )
}

const MONTH_CHIPS = 3

function MonthGrid({ anchor, sessions, onDay }: { anchor: Date; sessions: CalendarSession[]; onDay: (d: Date) => void }) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const now = useNow()
  const days = monthDays(anchor)
  return (
    <div className="overflow-hidden rounded-lg border bg-card" data-testid="month-grid">
      <div className="grid grid-cols-7 border-b text-center text-xs text-muted-foreground">
        {days.slice(0, 7).map((d) => (
          <div key={d.toISOString()} className="py-2">
            {format(d, 'EEEEEE', { locale: de })}.
          </div>
        ))}
      </div>
      <div className="grid grid-cols-7">
        {days.map((d) => {
          const here = sessions.filter((s) => isSameDay(sessionStart(s) ?? 0, d))
          return (
            <div
              key={d.toISOString()}
              role="button"
              tabIndex={0}
              onClick={() => onDay(d)}
              className={cn(
                'min-h-28 cursor-pointer space-y-1 border-r border-b p-1.5 text-left transition-colors hover:bg-foreground/[0.03] [&:nth-child(7n)]:border-r-0',
                !isSameMonth(d, anchor) && 'bg-muted/40 text-muted-foreground',
              )}
            >
              <div
                className={cn(
                  'inline-flex size-6 items-center justify-center rounded-full text-xs',
                  isSameDay(d, now) && 'bg-primary font-medium text-primary-foreground',
                )}
              >
                {d.getDate()}
              </div>
              {here.slice(0, MONTH_CHIPS).map((s) => {
                const state = calendarState(s)
                return (
                  <button
                    key={s.id}
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation()
                      void navigate({ to: '/sessions/$sessionId', params: { sessionId: s.id } })
                    }}
                    className={cn('flex w-full items-center gap-1 truncate rounded border px-1.5 py-0.5 text-[11px]', blockStyle[state])}
                  >
                    <span className="tabular-nums">{format(sessionStart(s)!, 'HH:mm')}</span>
                    <span className="truncate font-mono">{s.chiffre ?? s.patient_name}</span>
                  </button>
                )
              })}
              {here.length > MONTH_CHIPS && (
                <div className="px-1 text-[11px] text-muted-foreground">{t('calendar.more', { count: here.length - MONTH_CHIPS })}</div>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}

/** The current time, updated every minute (today marker, "now" line). */
function useNow() {
  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 60_000)
    return () => clearInterval(id)
  }, [])
  return now
}
