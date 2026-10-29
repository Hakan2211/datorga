import { describe, expect, it } from 'vitest'
import type { QueueSession } from '@/bindings/QueueSession'
import type { ReportSummary } from '@/bindings/ReportSummary'
import { boardColumn, dayGroup, groupSessions, matchesPatient, sessionTodo, sortBoard, todoCounts } from './queue'

// Wednesday 29.10.2026, 12:00 local.
const now = new Date(2026, 9, 29, 12, 0)
const iso = (d: number, h = 9) => new Date(2026, 9, d, h, 0).toISOString()

function session(id: string, at: string | null, patch: Partial<QueueSession> = {}): QueueSession {
  return {
    id,
    patient_id: 'p',
    episode_id: null,
    scheduled_at: at,
    started_at: null,
    ended_at: null,
    duration_s: null,
    modality: null,
    status: 'planned',
    planned_minutes: null,
    cancelled_reason: null,
    note_id: null,
    note_status: null,
    note_export_outdated: false,
    patient_name: 'Anna Kurz',
    chiffre: 'K120385',
    number: 1,
    has_prep: false,
    sources_finished: 0,
    sources_unreviewed: 0,
    recording: false,
    first_name: 'Anna',
    last_name: 'Kurz',
    has_photo: false,
    summary_head: null,
    ...patch,
  }
}

describe('sessionTodo', () => {
  it('walks the steps in order', () => {
    expect(sessionTodo(session('a', iso(30)), now)).toBeNull()
    expect(sessionTodo(session('a', iso(28)), now)).toBe('note_open')
    expect(sessionTodo(session('a', iso(30), { sources_finished: 1 }), now)).toBe('note_open')
    expect(sessionTodo(session('a', iso(28), { sources_finished: 1, sources_unreviewed: 1, note_id: 'n', note_status: 'draft' }), now)).toBe(
      'transcript',
    )
    expect(sessionTodo(session('a', iso(28), { note_id: 'n', note_status: 'draft' }), now)).toBe('unsigned')
    expect(sessionTodo(session('a', iso(28), { note_id: 'n', note_status: 'signed' }), now)).toBeNull()
    expect(sessionTodo(session('a', iso(28), { note_id: 'n', note_status: 'exported', note_export_outdated: true }), now)).toBe('reexport')
  })

  it('leaves cancelled and running sessions alone', () => {
    expect(sessionTodo(session('a', iso(28), { status: 'cancelled' }), now)).toBeNull()
    expect(sessionTodo(session('a', iso(29), { recording: true }), now)).toBeNull()
  })

  it('counts per todo', () => {
    const rows = [session('a', iso(28)), session('b', iso(27)), session('c', iso(30)), session('d', iso(1), { sources_unreviewed: 1 })]
    expect(todoCounts(rows, now)).toEqual({ transcript: 1, note_open: 2, unsigned: 0, reexport: 0 })
  })
})

describe('groups', () => {
  it('splits today, this week (Mon–Sun), later and older', () => {
    expect(dayGroup(session('a', iso(29, 18)), now)).toBe('today')
    expect(dayGroup(session('a', iso(26, 8)), now)).toBe('week') // Monday
    expect(dayGroup(session('a', new Date(2026, 10, 1, 20).toISOString()), now)).toBe('week') // Sunday
    expect(dayGroup(session('a', iso(25)), now)).toBe('older')
    expect(dayGroup(session('a', new Date(2026, 10, 2, 9).toISOString()), now)).toBe('later')
    expect(dayGroup(session('a', null), now)).toBe('older')
  })

  it('orders older newest first, the rest soonest first', () => {
    const rows = [session('old1', iso(1)), session('old2', iso(20)), session('t2', iso(29, 15)), session('t1', iso(29, 8))]
    const groups = groupSessions(rows, now)
    expect(groups.map((g) => [g.group, g.sessions.map((s) => s.id)])).toEqual([
      ['today', ['t1', 't2']],
      ['older', ['old2', 'old1']],
    ])
  })

  it('searches name and Chiffre', () => {
    const s = session('a', iso(29))
    expect(matchesPatient(s, 'kurz')).toBe(true)
    expect(matchesPatient(s, 'k1203')).toBe(true)
    expect(matchesPatient(s, 'Kurz, Anna')).toBe(true)
    expect(matchesPatient(s, 'meier')).toBe(false)
    expect(matchesPatient(s, ' ')).toBe(true)
  })
})

function report(id: string, patch: Partial<ReportSummary> = {}): ReportSummary {
  return {
    id,
    patient_id: 'p',
    patient_name: 'Anna Kurz',
    chiffre: null,
    method: 'VT',
    kind: 'umwandlung',
    seq: 1n,
    status: 'draft',
    updated_at: '2026-10-01T00:00:00Z',
    sections_filled: 0,
    sections_total: 7,
    sessions_left: null,
    due_by: null,
    ...patch,
  }
}

describe('board', () => {
  it('puts a fully written draft under review', () => {
    expect(boardColumn(report('a', { sections_filled: 5 }))).toBe('draft')
    expect(boardColumn(report('a', { sections_filled: 7 }))).toBe('review')
    expect(boardColumn(report('a', { sections_total: 0 }))).toBe('draft')
    expect(boardColumn(report('a', { status: 'in_review' }))).toBe('review')
    expect(boardColumn(report('a', { status: 'exported' }))).toBe('exported')
  })

  it('sorts the closest deadline first, then the latest change', () => {
    const rows = [
      report('late', { updated_at: '2026-10-20T00:00:00Z' }),
      report('soon', { sessions_left: 1 }),
      report('later', { sessions_left: 3 }),
      report('early', { updated_at: '2026-10-02T00:00:00Z' }),
    ]
    expect(sortBoard(rows).map((r) => r.id)).toEqual(['soon', 'later', 'late', 'early'])
  })
})
