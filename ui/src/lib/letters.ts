// Briefe (slice 45): pure helpers for the list, the letter page and the settings.
import type { LetterState } from '@/bindings/LetterState'
import type { LetterSummary } from '@/bindings/LetterSummary'
import type { Practice } from '@/bindings/Practice'
import type { RecipientKind } from '@/bindings/RecipientKind'
import type { Template } from '@/bindings/Template'

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

/** Templates the Briefe page offers: the letter layouts, recommended ones for the recipient first. */
export function letterTemplates(templates: Template[], kind: RecipientKind | null): { template: Template; recommended: boolean }[] {
  return templates
    .filter((t) => t.layout === 'letter')
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
