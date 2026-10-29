import type { CheckItem } from '@/bindings/CheckItem'
import type { EpisodeFact } from '@/bindings/EpisodeFact'
import type { SectionDef } from '@/bindings/SectionDef'
import type { Diagnosis } from '@/bindings/Diagnosis'
import type { ReviewHint } from '@/bindings/ReviewHint'
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

/** A section in the focus mode's list: taken over, proposal waiting, still empty, or left out. */
export type SectionState = 'done' | 'proposal' | 'open' | 'skipped'

export function sectionState(o: { notRelevant: boolean; proposal: boolean; filled: boolean }): SectionState {
  return o.notRelevant ? 'skipped' : o.proposal ? 'proposal' : o.filled ? 'done' : 'open'
}

/** Ready for step 4 (mirrors `store::focus::sync_review`): every section done or left out. */
export function allSectionsDone(states: SectionState[]): boolean {
  return states.length > 0 && states.every((s) => s === 'done' || s === 'skipped')
}

/** The next section after `from` (wrapping) that still needs her, or null when none does. */
export function nextOpenSection(keys: string[], states: Record<string, SectionState>, from: string): string | null {
  const at = keys.indexOf(from)
  for (let i = 1; i <= keys.length; i++) {
    const k = keys[(at + i) % keys.length]
    if (states[k] === 'proposal' || states[k] === 'open') return k
  }
  return null
}

export type CheckTone = 'block' | 'warn' | 'info'

/** One line of step 4: a check of the app's own, differing diagnoses, or a Gutachter-Check hint. */
export type CheckEntry =
  | { kind: 'check'; id: string; tone: CheckTone; item: CheckItem }
  | { kind: 'diagnoses'; id: string; tone: CheckTone }
  | { kind: 'hint'; id: string; tone: CheckTone; hint: ReviewHint }

const TONE_RANK: Record<CheckTone, number> = { block: 0, warn: 1, info: 2 }

/**
 * Step 4's list, most pressing first: what blocks signing, then what a Gutachter would likely
 * object to (and diagnoses that differ from the Behandlungsfall), then notes and the length.
 * Passed checks and ignored hints are left out; the length always shows.
 */
export function checkEntries(o: {
  checklist: CheckItem[]
  hints: ReviewHint[]
  reportDiagnoses: Diagnosis[] | null
  episodeDiagnoses: Diagnosis[]
}): CheckEntry[] {
  const out: CheckEntry[] = []
  for (const item of o.checklist) {
    if (item.key === 'length') out.push({ kind: 'check', id: item.key, tone: item.ok ? 'info' : 'warn', item })
    else if (!item.ok) out.push({ kind: 'check', id: item.key, tone: item.severity === 'block' ? 'block' : 'warn', item })
  }
  if (o.reportDiagnoses && o.episodeDiagnoses.length > 0 && !sameDiagnoses(o.reportDiagnoses, o.episodeDiagnoses)) {
    out.push({ kind: 'diagnoses', id: 'diagnoses', tone: 'warn' })
  }
  for (const hint of o.hints) {
    if (!hint.ignored) out.push({ kind: 'hint', id: hint.id, tone: hint.level === 'important' ? 'warn' : 'info', hint })
  }
  // Stable: within a tone, checks before hints, the length last.
  const last = (e: CheckEntry) => (e.kind === 'check' && e.item.key === 'length' ? 1 : 0)
  return out
    .map((e, i) => ({ e, i }))
    .sort((a, b) => TONE_RANK[a.e.tone] - TONE_RANK[b.e.tone] || last(a.e) - last(b.e) || a.i - b.i)
    .map(({ e }) => e)
}
