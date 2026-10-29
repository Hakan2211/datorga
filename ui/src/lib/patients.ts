// Patient overview: age, ICD chapter, filter chips and sort order for the card view.
import type { PatientCard } from '@/bindings/PatientCard'

/** Full years on `now`, or null without a (valid) birth date. */
export function ageOn(birthDate: string | null, now: Date): number | null {
  const m = birthDate?.match(/^(\d{4})-(\d{2})-(\d{2})$/)
  if (!m) return null
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])]
  let age = now.getFullYear() - y
  if (now.getMonth() + 1 < mo || (now.getMonth() + 1 === mo && now.getDate() < d)) age -= 1
  return age >= 0 ? age : null
}

/** "F32.1" → "F3"; anything outside chapter V (F00–F99) → "other". */
export function icdChapter(code: string): string {
  const m = code.trim().toUpperCase().match(/^F(\d)/)
  return m ? `F${m[1]}` : 'other'
}

export type CardFilter = {
  ageGroup: 'adult' | 'kjp' | null
  gender: 'w' | 'm' | 'd' | null
  chapter: string | null
  method: 'VT' | 'TP' | 'AP' | null
}

export const noFilter: CardFilter = { ageGroup: null, gender: null, chapter: null, method: null }

export type CardSort = 'name' | 'next' | 'last'

/** The Behandlungsfall's age group wins; without one the age decides (under 21 = KJP, as in the Psychotherapie-Richtlinie). */
export function ageGroupOf(card: PatientCard, now: Date): 'adult' | 'kjp' | null {
  if (card.age_group) return card.age_group
  const age = ageOn(card.patient.birth_date, now)
  return age === null ? null : age < 21 ? 'kjp' : 'adult'
}

export function matches(card: PatientCard, f: CardFilter, now: Date): boolean {
  if (f.ageGroup && ageGroupOf(card, now) !== f.ageGroup) return false
  if (f.gender && card.patient.gender !== f.gender) return false
  if (f.method && card.method !== f.method) return false
  if (f.chapter && !card.diagnoses.some((d) => icdChapter(d.code) === f.chapter)) return false
  return true
}

/** Chapters that occur in the list, in code order, so the chips only offer what can match. */
export function chaptersIn(cards: PatientCard[]): string[] {
  const set = new Set(cards.flatMap((c) => c.diagnoses.map((d) => icdChapter(d.code))))
  return [...set].sort((a, b) => (a === 'other' ? 1 : b === 'other' ? -1 : a.localeCompare(b)))
}

const byName = (a: PatientCard, b: PatientCard) =>
  a.patient.last_name.localeCompare(b.patient.last_name, 'de') || a.patient.first_name.localeCompare(b.patient.first_name, 'de')

/** `next`: soonest first, no appointment last. `last`: most recently seen first, never seen last. */
export function sortCards(cards: PatientCard[], sort: CardSort): PatientCard[] {
  const out = [...cards]
  if (sort === 'name') return out.sort(byName)
  const key = (c: PatientCard) => (sort === 'next' ? c.next_at : c.last_at)
  return out.sort((a, b) => {
    const [x, y] = [key(a), key(b)]
    if (x && y && x !== y) return sort === 'next' ? x.localeCompare(y) : y.localeCompare(x)
    if (x && !y) return -1
    if (!x && y) return 1
    return byName(a, b)
  })
}

export function initials(first: string, last: string): string {
  return `${first.trim()[0] ?? ''}${last.trim()[0] ?? ''}`.toUpperCase()
}
