import type { LastBackup } from '@/lib/api'

/** Default reminder interval; the setting `backup_reminder_days` overrides it (0 = off). */
export const DEFAULT_REMINDER_DAYS = 7

const DAY_MS = 24 * 60 * 60 * 1000

/**
 * Whether to remind: there is data to lose, the reminder is on, and the last backup is older
 * than `days` (or there is none). Returns the age in whole days, `null` for "never backed up",
 * or `false` when no reminder is due.
 */
export function backupDue(last: LastBackup | null | undefined, days: number, hasData: boolean, now = Date.now()): number | null | false {
  if (!hasData || days <= 0) return false
  if (!last) return null
  const at = Date.parse(last.at)
  if (Number.isNaN(at)) return null
  const age = now - at
  return age >= days * DAY_MS ? Math.floor(age / DAY_MS) : false
}
