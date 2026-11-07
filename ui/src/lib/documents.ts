import { open } from '@tauri-apps/plugin-dialog'
import type { Category } from '@/bindings/Category'
import type { Imported } from '@/bindings/Imported'
import type { StyleMark } from '@/bindings/StyleMark'
import type { Template } from '@/bindings/Template'

/** What "Datei oder Scan" offers: what `document_read` reads (text layer, else Windows OCR). */
export const DOCUMENT_EXTENSIONS = ['pdf', 'docx', 'txt', 'md', 'png', 'jpg', 'jpeg', 'tif', 'tiff', 'bmp', 'gif', 'heic']

/** The native file dialog for a document; `null` when she cancels. */
export async function pickDocument(title: string, filterName: string): Promise<string | null> {
  const path = await open({ title, multiple: false, directory: false, filters: [{ name: filterName, extensions: DOCUMENT_EXTENSIONS }] })
  return typeof path === 'string' ? path : null
}

/** Imported text added below what is there, after a blank line. */
export function appendText(current: string, added: string): string {
  const head = current.trimEnd()
  const tail = added.trim()
  if (!tail) return current
  return head ? `${head}\n\n${tail}` : tail
}

/** How the text was read, for the toast ("3 Seiten, per Texterkennung"). Keys under `documents.`. */
export function importSummary(imported: Imported): { key: string; pages: number; skipped: number } {
  return { key: `documents.read.${imported.method}`, pages: imported.pages, skipped: imported.pages_skipped }
}

/** The text split into plain parts and marks, for the redaction review. */
export type ReviewPart = { text: string; mark?: StyleMark; kept?: boolean }

export function reviewParts(text: string, marks: StyleMark[], keep: string[]): ReviewPart[] {
  const kept = new Set(keep.map((k) => k.trim().toLowerCase()))
  const parts: ReviewPart[] = []
  let at = 0
  for (const m of [...marks].sort((a, b) => a.start - b.start)) {
    if (m.start < at || m.end > text.length) continue
    if (m.start > at) parts.push({ text: text.slice(at, m.start) })
    parts.push({ text: text.slice(m.start, m.end), mark: m, kept: kept.has(m.text.trim().toLowerCase()) })
    at = m.end
  }
  if (at < text.length) parts.push({ text: text.slice(at) })
  return parts
}

/** Toggles a surface text in the "keep" list (all its occurrences, case-insensitive). */
export function toggleKeep(keep: string[], text: string): string[] {
  const k = text.trim().toLowerCase()
  return keep.some((x) => x.trim().toLowerCase() === k) ? keep.filter((x) => x.trim().toLowerCase() !== k) : [...keep, text.trim()]
}

const STYLE_CATEGORIES: Category[] = ['ptv3', 'bericht', 'brief']

/** Document types an exemplar can be for: PTV 3 Berichte first, then the letter types. */
export function styleTypes(templates: Template[]): Template[] {
  const order = (t: Template) => STYLE_CATEGORIES.indexOf(t.category)
  return templates
    .filter((t) => STYLE_CATEGORIES.includes(t.category))
    .sort((a, b) => order(a) - order(b) || a.title.localeCompare(b.title, 'de'))
}

/** Placeholders a redacted text contains (to show that it was redacted). */
export function placeholderCount(text: string): number {
  return (text.match(/\[(Name|Ort|Arbeitgeber|Datum|Zeichen|Kontakt)\]/g) ?? []).length
}
