// Briefe (slice 45): pure helpers for the list, the letter page and the settings.
import type { Letter } from '@/bindings/Letter'
import type { LetterMeta } from '@/bindings/LetterMeta'
import type { LetterState } from '@/bindings/LetterState'
import type { LetterSummary } from '@/bindings/LetterSummary'
import type { Practice } from '@/bindings/Practice'
import type { RecipientKind } from '@/bindings/RecipientKind'
import type { Category } from '@/bindings/Category'
import type { Template } from '@/bindings/Template'

/** As `letter::DEFAULT_SALUTATION`. */
export const DEFAULT_SALUTATION = 'Sehr geehrte Damen und Herren,'

export const RECIPIENT_KINDS: RecipientKind[] = ['rentenversicherung', 'pflegekasse', 'versorgungsamt', 'arzt', 'krankenkasse', 'sonstige']

export const EMPTY_PRACTICE: Practice = {
  name: '',
  subtitle: '',
  street: '',
  postal_code: '',
  city: '',
  phone: '',
  fax: '',
  email: '',
  web: '',
  bank: '',
  iban: '',
  bic: '',
  tax_no: '',
  title_lines: '',
  place: '',
  signature_image: false,
}

const joined = (parts: (string | null | undefined)[], sep: string) =>
  parts
    .map((p) => p?.trim() ?? '')
    .filter(Boolean)
    .join(sep)

/** "Praxis X · Straße 1 · 12345 Ort", as `Practice::header_line` (Bericht header, sender line). */
export function practiceLine(p: Practice | null | undefined): string | null {
  if (!p) return null
  return joined([p.name, p.street, joined([p.postal_code, p.city], ' ')], ' · ') || null
}

/**
 * Templates a "Neuer Brief" offers: the letter layouts (optionally of some categories only),
 * recommended ones for the recipient first.
 */
export function letterTemplates(
  templates: Template[],
  kind: RecipientKind | null,
  categories?: Category[],
): { template: Template; recommended: boolean }[] {
  return templates
    .filter((t) => t.layout === 'letter' && (!categories || categories.includes(t.category)))
    .map((template) => ({ template, recommended: !!kind && template.recipients.includes(kind) }))
    .sort((a, b) => Number(b.recommended) - Number(a.recommended) || a.template.title.localeCompare(b.template.title, 'de'))
}

export type LetterFilter = 'all' | LetterState

export function matchesLetter(l: LetterSummary, filter: LetterFilter, query: string): boolean {
  if (filter !== 'all' && l.state !== filter) return false
  const needle = query.trim().toLowerCase()
  if (!needle) return true
  return [l.patient_name, l.recipient_name, l.subject, l.template_title].some((s) => s.toLowerCase().includes(needle))
}

/** Address lines from the textarea: trimmed, empty lines dropped. */
export function linesFromText(text: string): string[] {
  return text
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
}

/** The day as YYYY-MM-DD in local time (date inputs, "Versendet am"). */
export function isoDay(d = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}

/** "07.11.2026" from YYYY-MM-DD (or an RFC 3339 time). */
export function germanDay(iso: string): string {
  const m = iso.match(/^(\d{4})-(\d{2})-(\d{2})/)
  return m ? `${m[3]}.${m[2]}.${m[1]}` : iso
}

/** "Musterstadt, den 07.11.2026" as the letter prints it. */
export function placeDate(place: string | null | undefined, date: string): string {
  const p = place?.trim()
  return p ? `${p}, den ${germanDay(date)}` : germanDay(date)
}

/** The editable part of a letter, for `letter_meta_save`. */
export const metaOf = (l: Letter): LetterMeta => ({
  episode_id: l.episode_id,
  recipient_id: l.recipient_id,
  recipient: l.recipient,
  salutation: l.salutation,
  subject: l.subject,
  refs: l.refs,
  place: l.place,
  letter_date: l.letter_date,
  follow_up_on: l.follow_up_on,
  follow_up_note: l.follow_up_note,
})

// ---------- Textbausteine (slice 48) ----------

/** As `letter::PLACEHOLDERS`: filled from the patient record when a block is inserted. */
export const PLACEHOLDERS = [
  '{Name}',
  '{Vorname}',
  '{Nachname}',
  '{Anrede}',
  '{Frau/Herr}',
  '{sie/er}',
  '{ihre/seine}',
  '{die Patientin/den Patienten}',
  '{Geburtsdatum}',
  '{Behandlung seit}',
  '{Diagnosen}',
  '{Krankenkasse}',
  '{Versichertennummer}',
  '{Rentenversicherungsnummer}',
] as const

/** The insert menu: blocks meant for the section first, then the rest; most used first in each. */
export function orderBlocks<B extends { template_keys: string[]; section_key: string | null; use_count: number }>(
  blocks: B[],
  templateKey: string,
  sectionKey: string | null,
): { section: B[]; rest: B[] } {
  const fit = blocks
    .filter((b) => b.template_keys.length === 0 || b.template_keys.includes(templateKey))
    .sort((a, b) => b.use_count - a.use_count)
  const section = sectionKey ? fit.filter((b) => b.section_key === sectionKey) : []
  return { section, rest: fit.filter((b) => !section.includes(b)) }
}

/** The first words of a block's text, for lists. */
export function blockSnippet(html: string, max = 140): string {
  const text = new DOMParser().parseFromString(html.replace(/<\/(p|li|h3)>/g, '$& '), 'text/html').body.textContent?.replace(/\s+/g, ' ').trim() ?? ''
  return text.length > max ? `${text.slice(0, max - 1).trimEnd()}…` : text
}

/**
 * Text saved from a letter as a Textbaustein, with the letter patient's name and birth date
 * turned back into placeholders, so the block carries no one's identity into other letters.
 */
export function toPlaceholders(
  html: string,
  p: { first_name: string; last_name: string; birth_date: string | null },
): { html: string; replaced: boolean } {
  const esc = (s: string) => s.trim().replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  // Word edges that also hold for umlauts ("Özdemir"); \b is ASCII-only.
  const word = (src: string) => new RegExp(`(?<![\\p{L}\\d])${src}(?![\\p{L}\\d])`, 'gu')
  const first = esc(p.first_name)
  const last = esc(p.last_name)
  let out = html
  const swap = (re: RegExp, to: string) => (out = out.replace(re, to))
  if (last) {
    swap(word(`(?:Frau|Herrn?)\\s+(?:${first ? `${first}\\s+` : ''})?${last}`), '{Frau/Herr} {Nachname}')
    if (first) swap(word(`${first}\\s+${last}`), '{Name}')
    swap(word(last), '{Nachname}')
  }
  if (first) swap(word(first), '{Vorname}')
  const m = p.birth_date?.match(/^(\d{4})-(\d{2})-(\d{2})/)
  if (m) swap(word(`${m[3]}\\.${m[2]}\\.${m[1]}`), '{Geburtsdatum}')
  return { html: out, replaced: out !== html }
}
