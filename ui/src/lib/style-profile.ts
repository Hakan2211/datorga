// Stilprofil helpers (slice 52).
import type { Computed } from '@/bindings/Computed'

/** The `template_key` of her notes for every type (backend `style_profile::ALL_TYPES`). */
export const ALL_TYPES = '*'

type T = (key: string, opts?: Record<string, unknown>) => string

/** The computed profile as short parts: sentence length, voice, person, form. */
export function profileSummary(c: Computed | null | undefined, t: T): string[] {
  if (!c) return []
  const out: string[] = []
  if (c.sentence_words > 0) out.push(t('styleProfile.sentence', { count: c.sentence_words }))
  out.push(t(`styleProfile.voice.${c.voice}`))
  if (c.person_term) out.push(`„${c.person_term}“`)
  out.push(t(`styleProfile.form.${c.form}`))
  return out
}

/** "3 Abschnitte erkannt" / "als Ganzes" for an exemplar (0 = its headings didn't match). */
export function mappedLabel(mapped: [string, number][] | undefined, id: string, t: T): string | null {
  const n = mapped?.find(([x]) => x === id)?.[1]
  if (n === undefined) return null
  return n > 0 ? t('styleProfile.mapped', { count: n }) : t('styleProfile.whole')
}
