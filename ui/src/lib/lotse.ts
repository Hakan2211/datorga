import type { EpisodeFact } from '@/bindings/EpisodeFact'
import type { SectionDef } from '@/bindings/SectionDef'
import type { Diagnosis } from '@/bindings/Diagnosis'

/** Template whose sections facts are tagged with (mirrors `extract::TAG_TEMPLATE_KEY`). */
export const TAG_TEMPLATE_KEY = 'ptv3-erst-umwandlung'

/** The facts a draft of `def` would see by default: since the previous application, tag-filtered. */
export function sectionFacts(templateKey: string, def: SectionDef, facts: EpisodeFact[], since: string | null): EpisodeFact[] {
  const recent = since ? facts.filter((f) => !f.session_at || f.session_at >= since) : facts
  // Tags exist for the Erstantrag sections; the Umwandlung section and the Fortführung take all facts.
  const tagged = templateKey === TAG_TEMPLATE_KEY && !def.when.kind?.length
  return tagged ? recent.filter((f) => !f.bericht || f.bericht.includes(def.key)) : recent
}

export type Readiness = 'full' | 'half' | 'empty' | 'none'

/** Three facts make a section ready to draft, one or two a start. */
export function factReadiness(count: number): Readiness {
  return count >= 3 ? 'full' : count > 0 ? 'half' : 'empty'
}

/** Same diagnoses (code + certainty), in any order. */
export function sameDiagnoses(a: Diagnosis[], b: Diagnosis[]): boolean {
  const key = (l: Diagnosis[]) => l.map((d) => `${d.code.trim().toUpperCase()}|${d.certainty}`).sort().join(',')
  return key(a) === key(b)
}
