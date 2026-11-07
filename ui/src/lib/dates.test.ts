import { describe, expect, it } from 'vitest'
import { formatTypedDay, parseTypedDay, timeSlots } from './dates'

describe('parseTypedDay', () => {
  it('reads what a therapist types', () => {
    expect(parseTypedDay('12.3.1985')).toBe('1985-03-12')
    expect(parseTypedDay('12.03.1985')).toBe('1985-03-12')
    expect(parseTypedDay(' 01.12.85 ')).toBe('1985-12-01')
    expect(parseTypedDay('12031985')).toBe('1985-03-12')
    expect(parseTypedDay('1985-03-12')).toBe('1985-03-12')
  })
  it('waits for a full, real date', () => {
    for (const s of ['', '12.', '12.3.', '12.3.1', '12.3.198', '31.2.2020', '32.1.2020', 'abc']) expect(parseTypedDay(s)).toBeNull()
  })
})

it('formats stored days for the field', () => {
  expect(formatTypedDay('1985-03-12')).toBe('12.03.1985')
  expect(formatTypedDay(null)).toBe('')
})

it('lists quarter-hour slots', () => {
  expect(timeSlots(8, 9)).toEqual(['08:00', '08:15', '08:30', '08:45', '09:00'])
})
