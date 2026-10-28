// Calendar maths (slice 22): ranges per view, overlap layout, snapping, series dates, and where a
// session stands. All dates are local; the backend gets RFC 3339 UTC strings.
import {
  addDays,
  addMonths,
  addWeeks,
  format,
  getISOWeek,
  isSameMonth,
  startOfDay,
  startOfMonth,
  startOfWeek,
} from 'date-fns'
import { de } from 'date-fns/locale'
import type { CalendarSession } from '@/bindings/CalendarSession'
import type { ReportStatus } from '@/bindings/ReportStatus'
import { stepStates, type PillTone, type StepState } from '@/components/bits'

export type CalendarView = 'day' | 'week' | 'month'

/** Length of a session without a planned one (Richtlinien-Einzelsitzung). */
export const DEFAULT_MINUTES = 50
export const SNAP_MINUTES = 15

/** 'YYYY-MM-DD' of a local date (tasks and the day note are keyed by it). */
export function localDate(d: Date): string {
  return format(d, 'yyyy-MM-dd')
}

/** Parses 'YYYY-MM-DD' as a local date (not UTC midnight like `new Date('2026-10-28')`). */
export function parseLocalDate(s: string): Date {
  const [y, m, d] = s.split('-').map(Number)
  return new Date(y, m - 1, d)
}

/** `[from, to)` of the view around `date`. Weeks start on Monday; the month view shows whole weeks. */
export function viewRange(view: CalendarView, date: Date): { from: Date; to: Date } {
  if (view === 'day') {
    const from = startOfDay(date)
    return { from, to: addDays(from, 1) }
  }
  if (view === 'week') {
    const from = startOfWeek(date, { weekStartsOn: 1 })
    return { from, to: addDays(from, 7) }
  }
  const from = startOfWeek(startOfMonth(date), { weekStartsOn: 1 })
  return { from, to: addDays(from, 42) }
}

export function stepView(view: CalendarView, date: Date, dir: 1 | -1): Date {
  if (view === 'day') return addDays(date, dir)
  if (view === 'week') return addWeeks(date, dir)
  return addMonths(startOfMonth(date), dir)
}

/** The 42 days of the month grid (6 weeks from Monday). */
export function monthDays(date: Date): Date[] {
  const { from } = viewRange('month', date)
  return Array.from({ length: 42 }, (_, i) => addDays(from, i))
}

export function viewTitle(view: CalendarView, date: Date, days: Date[]): string {
  if (view === 'day') return format(date, 'EEEE, d. MMMM yyyy', { locale: de })
  if (view === 'month') return format(date, 'MMMM yyyy', { locale: de })
  const first = days[0]
  const last = days[days.length - 1]
  const span = isSameMonth(first, last)
    ? `${format(first, 'd.')}–${format(last, 'd. MMMM', { locale: de })}`
    : `${format(first, 'd. MMM', { locale: de })} – ${format(last, 'd. MMM', { locale: de })}`
  return `KW ${getISOWeek(first)}, ${span}`
}

export function sessionStart(s: { scheduled_at: string | null; started_at: string | null }): Date | null {
  const at = s.scheduled_at ?? s.started_at
  return at ? new Date(at) : null
}

export function minutesOf(s: { planned_minutes: number | null }): number {
  return s.planned_minutes ?? DEFAULT_MINUTES
}

/** Minutes since local midnight. */
export function minuteOfDay(d: Date): number {
  return d.getHours() * 60 + d.getMinutes()
}

export function snap(minutes: number, step = SNAP_MINUTES): number {
  return Math.round(minutes / step) * step
}

/** "9:40", "09:40", "0940" or "9.40" → minutes past midnight; null if it isn't a time. */
export function parseTime(s: string): number | null {
  const m = /^([01]?\d|2[0-3])[:.]?([0-5]\d)$/.exec(s.trim())
  return m ? Number(m[1]) * 60 + Number(m[2]) : null
}

/** `day` at `minute` past local midnight. */
export function atMinute(day: Date, minute: number): Date {
  const d = startOfDay(day)
  d.setMinutes(minute)
  return d
}

/**
 * Side-by-side columns for overlapping events within one day: each event gets its column and the
 * number of columns of the cluster it belongs to.
 */
export function layoutDay<T extends { id: string; start: number; end: number }>(
  events: T[],
): Map<string, { col: number; cols: number }> {
  const sorted = [...events].sort((a, b) => a.start - b.start || b.end - a.end)
  const out = new Map<string, { col: number; cols: number }>()
  let cluster: { id: string; col: number }[] = []
  let colEnds: number[] = []
  let clusterEnd = -Infinity
  const flush = () => {
    for (const c of cluster) out.set(c.id, { col: c.col, cols: colEnds.length })
    cluster = []
    colEnds = []
  }
  for (const e of sorted) {
    if (e.start >= clusterEnd) flush()
    let col = colEnds.findIndex((end) => end <= e.start)
    if (col < 0) {
      col = colEnds.length
      colEnds.push(e.end)
    } else {
      colEnds[col] = e.end
    }
    cluster.push({ id: e.id, col })
    clusterEnd = Math.max(clusterEnd, e.end)
  }
  flush()
  return out
}

/** Dates of a series: the first one and then every `everyWeeks` weeks, at the same local time. */
export function seriesDates(first: Date, count: number, everyWeeks: number): Date[] {
  return Array.from({ length: count }, (_, i) => addWeeks(first, i * everyWeeks))
}

// ---------- where a session stands ----------

export const SESSION_FLOW = ['prepare', 'capture', 'review', 'note', 'finish'] as const
export type FlowStep = (typeof SESSION_FLOW)[number]

export type FlowInput = {
  hasPrep: boolean
  /** All sources, finished ones and those without a confirmed transcript. */
  sources: number
  finished: number
  unreviewed: number
  hasNote: boolean
  noteStatus: ReportStatus | null
  exportOutdated: boolean
  /** A recording runs in this session. */
  recordingHere: boolean
}

/** The session stepper's states (session page and "Heute"). */
export function flowStates(f: FlowInput): Record<FlowStep, StepState> {
  const signed = f.noteStatus === 'signed' || f.noteStatus === 'exported'
  return stepStates(
    [
      { key: 'prepare', done: f.hasPrep || f.sources > 0 || f.hasNote },
      { key: 'capture', done: f.finished > 0 || f.hasNote },
      { key: 'review', done: f.finished > 0 ? f.unreviewed === 0 : f.hasNote },
      { key: 'note', done: signed },
      { key: 'finish', done: f.noteStatus === 'exported' && !f.exportOutdated },
    ],
    f.recordingHere ? 'capture' : undefined,
  )
}

export function calendarFlow(s: CalendarSession): FlowInput {
  return {
    hasPrep: s.has_prep,
    sources: s.sources_finished + (s.recording ? 1 : 0),
    finished: s.sources_finished,
    unreviewed: s.sources_unreviewed,
    hasNote: !!s.note_id,
    noteStatus: s.note_status,
    exportOutdated: s.note_export_outdated,
    recordingHere: s.recording,
  }
}

/** The step work continues at, or null when everything is done (or the session was cancelled). */
export function nextStep(s: CalendarSession): FlowStep | null {
  if (s.status === 'cancelled') return null
  const states = flowStates(calendarFlow(s))
  return SESSION_FLOW.find((k) => states[k] === 'current') ?? null
}

/** Colour state in the calendar: geplant / läuft / durchgeführt / Notiz offen / signiert / abgesagt. */
export type CalendarState = 'planned' | 'running' | 'held' | 'note_open' | 'signed' | 'cancelled'

export function calendarState(s: CalendarSession): CalendarState {
  if (s.status === 'cancelled') return 'cancelled'
  if (s.recording) return 'running'
  if (s.note_status === 'signed' || s.note_status === 'exported') return 'signed'
  if (s.note_id) return 'note_open'
  if (s.sources_finished > 0 || s.status === 'recorded') return 'held'
  return 'planned'
}

export const stateTone: Record<CalendarState, PillTone> = {
  planned: 'sand',
  running: 'danger',
  held: 'warning',
  note_open: 'warning',
  signed: 'success',
  cancelled: 'neutral',
}

/** Recordings or a note pin a session to its time (the backend refuses to move it then). */
export function isMovable(s: CalendarSession): boolean {
  return s.status === 'planned' && !s.note_id && s.sources_finished === 0 && !s.recording
}
