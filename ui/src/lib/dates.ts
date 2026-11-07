// Calendar days as the app stores them ("yyyy-MM-dd") and as a therapist types them ("12.3.1985").
import { format, isValid, parse } from 'date-fns'

const TYPED = ['d.M.yyyy', 'd.M.yy', 'yyyy-MM-dd', 'ddMMyyyy']

/** A stored day ("yyyy-MM-dd") as a local Date; null if missing or malformed. */
export function dayToDate(iso: string | null | undefined): Date | null {
  if (!iso) return null
  const d = parse(iso, 'yyyy-MM-dd', new Date())
  return isValid(d) ? d : null
}

export function dateToDay(d: Date): string {
  return format(d, 'yyyy-MM-dd')
}

/** "12.03.1985" for display in a date field. */
export function formatTypedDay(iso: string | null | undefined): string {
  const d = dayToDate(iso)
  return d ? format(d, 'dd.MM.yyyy') : ''
}

/**
 * What a therapist typed → "yyyy-MM-dd"; null if it isn't (yet) a full date. Accepts "12.3.1985",
 * "12.03.85" (century nearest to today), "12031985" and ISO.
 */
export function parseTypedDay(s: string): string | null {
  const text = s.trim()
  if (!text) return null
  for (const f of TYPED) {
    if (f === 'ddMMyyyy' ? !/^\d{8}$/.test(text) : f === 'd.M.yy' ? !/^\d{1,2}\.\d{1,2}\.\d{2}$/.test(text) : false) continue
    const d = parse(text, f, new Date())
    if (isValid(d) && d.getFullYear() >= 1900 && d.getFullYear() <= 2200) return dateToDay(d)
  }
  return null
}

/** Quarter-hour slots between `from` and `to` hours, as "HH:mm". */
export function timeSlots(from = 7, to = 21, step = 15): string[] {
  const out: string[] = []
  for (let m = from * 60; m <= to * 60; m += step) out.push(`${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`)
  return out
}
