// Sitzungen and Berichte as work queues (slice 32): what still needs the therapist, grouped by day
// or by status.
import { endOfWeek, isSameDay, startOfWeek } from 'date-fns'
import type { QueueSession } from '@/bindings/QueueSession'
import type { ReportSummary } from '@/bindings/ReportSummary'

// ---------- Sitzungen ----------

/**
 * What a session waits for, in the order she works through it. `note_open` (took place, no note
 * yet) and `unsigned` (note started, not signed) together are Heute's "Notiz offen".
 */
export type SessionTodo = 'transcript' | 'note_open' | 'unsigned' | 'reexport'
export const SESSION_TODOS: SessionTodo[] = ['transcript', 'note_open', 'unsigned', 'reexport']

export const sessionTime = (s: QueueSession): Date | null => {
  const at = s.scheduled_at ?? s.started_at
  return at ? new Date(at) : null
}

export function sessionTodo(s: QueueSession, now: Date): SessionTodo | null {
  if (s.status === 'cancelled' || s.recording) return null
  if (s.sources_unreviewed > 0) return 'transcript'
  if (s.note_status === 'signed' || s.note_status === 'exported') return s.note_export_outdated ? 'reexport' : null
  if (s.note_id) return 'unsigned'
  const at = sessionTime(s)
  const held = s.sources_finished > 0 || s.status === 'recorded' || (at !== null && at < now)
  return held ? 'note_open' : null
}

export function todoCounts(sessions: QueueSession[], now: Date): Record<SessionTodo, number> {
  const out: Record<SessionTodo, number> = { transcript: 0, note_open: 0, unsigned: 0, reexport: 0 }
  for (const s of sessions) {
    const k = sessionTodo(s, now)
    if (k) out[k] += 1
  }
  return out
}

/** Heute, the rest of this week (Mon–Sun, either side of today), later, and older. */
export type DayGroup = 'today' | 'week' | 'later' | 'older'
export const DAY_GROUPS: DayGroup[] = ['today', 'week', 'later', 'older']

export function dayGroup(s: QueueSession, now: Date): DayGroup {
  const at = sessionTime(s)
  if (!at) return 'older'
  if (isSameDay(at, now)) return 'today'
  if (at >= startOfWeek(now, { weekStartsOn: 1 }) && at <= endOfWeek(now, { weekStartsOn: 1 })) return 'week'
  return at > now ? 'later' : 'older'
}

/** Groups in display order, without empty ones. Older sessions newest first, the rest soonest first. */
export function groupSessions(sessions: QueueSession[], now: Date): { group: DayGroup; sessions: QueueSession[] }[] {
  const time = (s: QueueSession) => sessionTime(s)?.getTime() ?? 0
  return DAY_GROUPS.map((group) => {
    const rows = sessions.filter((s) => dayGroup(s, now) === group).sort((a, b) => time(a) - time(b))
    return { group, sessions: group === 'older' ? rows.reverse() : rows }
  }).filter((g) => g.sessions.length > 0)
}

export function matchesPatient(s: QueueSession, query: string): boolean {
  const needle = query.trim().toLocaleLowerCase('de')
  if (!needle) return true
  return [s.patient_name, `${s.last_name}, ${s.first_name}`, s.chiffre ?? ''].some((h) => h.toLocaleLowerCase('de').includes(needle))
}

// ---------- Berichte ----------

/**
 * Board columns, by status: a Bericht is `in_review` once every section is taken over or "nicht
 * relevant" and no proposal waits (set by the backend since slice 39).
 */
export type BoardColumn = 'draft' | 'review' | 'signed' | 'exported'
export const BOARD_COLUMNS: BoardColumn[] = ['draft', 'review', 'signed', 'exported']

export function boardColumn(r: ReportSummary): BoardColumn {
  switch (r.status) {
    case 'signed':
      return 'signed'
    case 'exported':
      return 'exported'
    case 'in_review':
      return 'review'
    case 'draft':
      return 'draft'
  }
}

export type ReportFilter = { method: 'VT' | 'TP' | 'AP' | null; kind: ReportSummary['kind'] }
export const noReportFilter: ReportFilter = { method: null, kind: null }

export function matchesReport(r: ReportSummary, f: ReportFilter): boolean {
  return (!f.method || r.method === f.method) && (!f.kind || r.kind === f.kind)
}

/** Open Berichte by deadline (sessions left, then planned date), the rest by last change. */
export function sortBoard(reports: ReportSummary[]): ReportSummary[] {
  const left = (r: ReportSummary) => r.sessions_left ?? Number.POSITIVE_INFINITY
  return [...reports].sort((a, b) => left(a) - left(b) || b.updated_at.localeCompare(a.updated_at))
}
