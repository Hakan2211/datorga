import type { EpisodeFact } from '@/bindings/EpisodeFact'
import type { SectionDef } from '@/bindings/SectionDef'
import type { Diagnosis } from '@/bindings/Diagnosis'
import type { QuestionView } from '@/lib/api'

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

/**
 * Three facts or answers make a section ready to draft if no question is open, any material a
 * start.
 */
export function readiness(material: number, open = 0): Readiness {
  return material >= 3 && open === 0 ? 'full' : material > 0 ? 'half' : 'empty'
}

/** Same diagnoses (code + certainty), in any order. */
export function sameDiagnoses(a: Diagnosis[], b: Diagnosis[]): boolean {
  const key = (l: Diagnosis[]) => l.map((d) => `${d.code.trim().toUpperCase()}|${d.certainty}`).sort().join(',')
  return key(a) === key(b)
}

/** Stable key of a question within a Bericht. */
export function questionId(q: Pick<QuestionView, 'section_key' | 'ask_key'>): string {
  return `${q.section_key}/${q.ask_key}`
}

/** Still for her to answer: no answer of hers, and the sessions don't answer it either. */
export function isOpen(q: QuestionView): boolean {
  return !q.answer && q.covered_by.length === 0
}

const CHIP_SEP = '; '

/** The answer split into its chip-sized parts ("keine; nicht bekannt"). */
function parts(text: string): string[] {
  return text.split(/\s*;\s*/).map((p) => p.trim()).filter(Boolean)
}

export function hasChip(text: string, chip: string): boolean {
  return parts(text).some((p) => p.toLowerCase() === chip.toLowerCase())
}

/** A chip fills an empty answer, extends one, or comes out again if it is in already. */
export function addChip(text: string, chip: string): string {
  const list = parts(text)
  const at = list.findIndex((p) => p.toLowerCase() === chip.toLowerCase())
  if (at >= 0) {
    list.splice(at, 1)
    return list.join(CHIP_SEP)
  }
  return text.trim() ? `${text.trim().replace(/[;,]$/, '')}${CHIP_SEP}${chip}` : chip
}
