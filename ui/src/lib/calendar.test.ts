import { describe, expect, it } from 'vitest'
import type { CalendarSession } from '@/bindings/CalendarSession'
import { calendarState, isMovable, isUntouched, layoutDay, monthDays, nextStep, parseTime, seriesDates, snap, viewRange, viewTitle } from './calendar'

const session = (over: Partial<CalendarSession> = {}): CalendarSession => ({
  id: 's',
  patient_id: 'p',
  episode_id: null,
  scheduled_at: '2026-10-28T08:00:00Z',
  started_at: null,
  ended_at: null,
  duration_s: null,
  modality: 'praesenz',
  status: 'planned',
  planned_minutes: null,
  cancelled_reason: null,
  note_id: null,
  note_status: null,
  note_export_outdated: false,
  patient_name: 'Anna Beispiel',
  chiffre: 'B1',
  number: 1,
  has_prep: false,
  sources_finished: 0,
  sources_unreviewed: 0,
  recording: false,
  ...over,
})

describe('viewRange', () => {
  it('starts weeks on Monday and covers six weeks in the month view', () => {
    const wed = new Date(2026, 9, 28, 15, 30)
    const week = viewRange('week', wed)
    expect(week.from).toEqual(new Date(2026, 9, 26))
    expect(week.to).toEqual(new Date(2026, 10, 2))
    expect(viewRange('day', wed)).toEqual({ from: new Date(2026, 9, 28), to: new Date(2026, 9, 29) })
    const month = monthDays(wed)
    expect(month).toHaveLength(42)
    expect(month[0]).toEqual(new Date(2026, 8, 28)) // Monday before 1 October
  })

  it('titles the week with its ISO number', () => {
    const days = [26, 27, 28, 29, 30].map((d) => new Date(2026, 9, d))
    expect(viewTitle('week', days[2], days)).toBe('KW 44, 26.–30. Oktober')
  })
})

describe('layoutDay', () => {
  it('puts overlapping events side by side and keeps others full width', () => {
    const out = layoutDay([
      { id: 'a', start: 480, end: 530 },
      { id: 'b', start: 500, end: 550 },
      { id: 'c', start: 540, end: 590 },
      { id: 'd', start: 600, end: 650 },
    ])
    expect(out.get('a')).toEqual({ col: 0, cols: 2 })
    expect(out.get('b')).toEqual({ col: 1, cols: 2 })
    expect(out.get('c')).toEqual({ col: 0, cols: 2 }) // reuses a's column once a ended
    expect(out.get('d')).toEqual({ col: 0, cols: 1 })
  })
})

describe('series and snapping', () => {
  it('keeps the local wall-clock time across the DST change', () => {
    const dates = seriesDates(new Date(2026, 9, 21, 9, 0), 3, 1) // 25 Oct 2026 is the switch
    expect(dates.map((d) => [d.getDate(), d.getHours()])).toEqual([
      [21, 9],
      [28, 9],
      [4, 9],
    ])
    expect(seriesDates(new Date(2026, 9, 21, 9), 2, 2)[1].getDate()).toBe(4)
  })

  it('reads typed times', () => {
    expect(['9:40', '09:40', '0940', '9.40', ' 23:59 '].map(parseTime)).toEqual([580, 580, 580, 580, 1439])
    expect(['24:00', '9:60', '9', 'neun', ''].map(parseTime)).toEqual([null, null, null, null, null])
  })

  it('snaps to quarter hours', () => {
    expect([snap(7), snap(8), snap(52), snap(-8)]).toEqual([0, 15, 45, -15])
  })
})

describe('session state', () => {
  it('derives colour state and next step', () => {
    expect([calendarState(session()), nextStep(session())]).toEqual(['planned', 'prepare'])
    expect(nextStep(session({ has_prep: true }))).toBe('capture')
    const held = session({ status: 'recorded', sources_finished: 1, sources_unreviewed: 1 })
    expect([calendarState(held), nextStep(held)]).toEqual(['held', 'review'])
    const noted = session({ status: 'recorded', sources_finished: 1, note_id: 'n', note_status: 'draft' })
    expect([calendarState(noted), nextStep(noted)]).toEqual(['note_open', 'note'])
    const signed = session({ note_id: 'n', note_status: 'signed' })
    expect([calendarState(signed), nextStep(signed)]).toEqual(['signed', 'finish'])
    expect(nextStep(session({ note_id: 'n', note_status: 'exported' }))).toBeNull()
    expect(calendarState(session({ recording: true }))).toBe('running')
    expect(nextStep(session({ recording: true }))).toBe('capture')
    const cancelled = session({ status: 'cancelled' })
    expect([calendarState(cancelled), nextStep(cancelled)]).toEqual(['cancelled', null])
  })

  it('moves sessions until the note is signed, cancels only untouched ones', () => {
    expect(isMovable(session())).toBe(true)
    expect(isMovable(session({ note_id: 'n', note_status: 'draft' }))).toBe(true)
    expect(isMovable(session({ status: 'recorded', sources_finished: 1 }))).toBe(true)
    expect(isMovable(session({ note_id: 'n', note_status: 'signed' }))).toBe(false)
    expect(isMovable(session({ recording: true }))).toBe(false)
    expect(isMovable(session({ status: 'cancelled' }))).toBe(false)
    expect(isUntouched(session())).toBe(true)
    expect(isUntouched(session({ note_id: 'n', note_status: 'draft' }))).toBe(false)
    expect(isUntouched(session({ sources_finished: 1 }))).toBe(false)
  })
})
