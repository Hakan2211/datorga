// Her own templates (slice 51): helpers for the template builder and editor.
import type { Ask } from '@/bindings/Ask'
import type { Hint } from '@/bindings/Hint'
import type { SectionDef } from '@/bindings/SectionDef'
import type { Template } from '@/bindings/Template'

/** Keys of her own templates (the backend's `builder::OWN_PREFIX`). */
export const OWN_PREFIX = 'eigen-'

export function isOwn(t: Pick<Template, 'key'> | string): boolean {
  return (typeof t === 'string' ? t : t.key).startsWith(OWN_PREFIX)
}

/** Placeholders a Betreff or Anfangstext may use (filled from the patient record). */
export const PLACEHOLDERS = ['{Name}', '{Geburtsdatum}', '{Frau/Herrn}', '{Frau/Herr}', '{sie/er}', '{Behandlung seit}', '{Krankenkasse}'] as const

/** Lower-case ASCII words joined by `_` (umlauts spelled out), like the backend's section keys. */
export function slugKey(s: string): string {
  const base = s
    .toLowerCase()
    .replace(/ä/g, 'ae')
    .replace(/ö/g, 'oe')
    .replace(/ü/g, 'ue')
    .replace(/ß/g, 'ss')
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .slice(0, 32)
    .replace(/_+$/, '')
  return !base || /^\d/.test(base) ? `abschnitt${base ? `_${base}` : ''}` : base
}

/** A key from `label` that isn't in `taken` (`befund`, `befund_2`, …). */
export function uniqueKey(label: string, taken: string[]): string {
  const base = slugKey(label)
  if (!taken.includes(base)) return base
  for (let n = 2; ; n++) if (!taken.includes(`${base}_${n}`)) return `${base}_${n}`
}

export function emptySection(taken: string[], heading = ''): SectionDef {
  return {
    key: uniqueKey(heading || 'abschnitt', taken),
    number: null,
    heading,
    field: 'prose',
    ai_allowed: true,
    required: false,
    optional: false,
    dictation: false,
    when: {},
    hints: [],
    asks: [],
    prior: [],
  }
}

/** A blank template to start from (one text section). */
export function blankTemplate(title: string): Template {
  return {
    key: `${OWN_PREFIX}${slugKey(title).replace(/_/g, '-')}-${Math.random().toString(16).slice(2, 6)}`,
    version: '1',
    title,
    source: 'Eigene Vorlage',
    report_kinds: [],
    category: 'brief',
    layout: 'letter',
    refs: [],
    recipients: [],
    release: false,
    rules: { target_pages: null, keyword_style_allowed: false, signature_required: true, pseudonymized: false, numbered: false },
    intro: '',
    sections: [emptySection([], '')],
  }
}

export function move<T>(list: T[], i: number, by: -1 | 1): T[] {
  const j = i + by
  if (j < 0 || j >= list.length) return list
  const out = [...list]
  ;[out[i], out[j]] = [out[j], out[i]]
  return out
}

/** Hints as one line each (conditions of an unchanged line are kept). */
export function hintsToText(hints: Hint[]): string {
  return hints.map((h) => h.text).join('\n')
}

export function hintsFromText(text: string, old: Hint[]): Hint[] {
  return text
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
    .map((line) => old.find((h) => h.text === line) ?? { text: line, when: {} })
}

/** Chips as typed: comma-separated. */
export function chipsFromText(text: string): string[] {
  return text
    .split(',')
    .map((c) => c.trim())
    .filter(Boolean)
}

export function newAsk(question: string, taken: string[]): Ask {
  return { key: uniqueKey(question || 'frage', taken), question, chips: [], when: {} }
}

/** What the editor warns about before saving (the backend validates again). */
export function templateProblems(t: Template): string[] {
  const out: string[] = []
  if (!t.title.trim()) out.push('title')
  if (t.sections.length === 0) out.push('noSections')
  if (t.category === 'bericht' && !t.audience) out.push('audience')
  if (t.sections.some((s) => s.field === 'diagnoses' && (s.ai_allowed || s.asks.length > 0))) out.push('diagnoses')
  return out
}

/** Drafts handed from the builder to the editor route (not saved until she saves). */
const drafts = new Map<string, Template>()

export function putDraft(t: Template): string {
  const id = Math.random().toString(36).slice(2, 10)
  drafts.set(id, t)
  return id
}

export function getDraft(id: string): Template | undefined {
  return drafts.get(id)
}
