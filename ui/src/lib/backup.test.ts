import { describe, expect, it } from 'vitest'
import { backupDue } from './backup'

const now = Date.parse('2026-10-27T12:00:00Z')
const last = (at: string) => ({ at, file: 'x.therabak', bytes: 1 })

describe('backupDue', () => {
  it('stays quiet without data or when switched off', () => {
    expect(backupDue(null, 7, false, now)).toBe(false)
    expect(backupDue(null, 0, true, now)).toBe(false)
  })

  it('reminds when there is no backup yet', () => {
    expect(backupDue(null, 7, true, now)).toBeNull()
    expect(backupDue(last('kaputt'), 7, true, now)).toBeNull()
  })

  it('reminds once the interval has passed and reports the age in days', () => {
    expect(backupDue(last('2026-10-21T12:00:01Z'), 7, true, now)).toBe(false)
    expect(backupDue(last('2026-10-20T12:00:00Z'), 7, true, now)).toBe(7)
    expect(backupDue(last('2026-09-27T12:00:00Z'), 7, true, now)).toBe(30)
    expect(backupDue(last('2026-10-26T11:00:00Z'), 1, true, now)).toBe(1)
  })
})
