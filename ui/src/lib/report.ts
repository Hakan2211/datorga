import type { DiagnosesBody } from '@/bindings/DiagnosesBody'
import type { FieldKind } from '@/bindings/FieldKind'

/** Must match thera_core::pii::CHARS_PER_PAGE so the meter agrees with the checklist. */
export const CHARS_PER_PAGE = 3300

export function plainText(html: string): string {
  return html
    .replace(/<[^>]*>/g, ' ')
    .replace(/&nbsp;/g, ' ')
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/\s+/g, ' ')
    .trim()
}

export function parseDiagnoses(body: string): DiagnosesBody {
  try {
    const d = JSON.parse(body) as Partial<DiagnosesBody>
    return { diagnoses: d.diagnoses ?? [], notes: d.notes ?? '' }
  } catch {
    return { diagnoses: [], notes: '' }
  }
}

export function sectionText(field: FieldKind, body: string): string {
  if (field === 'prose') return plainText(body)
  const d = parseDiagnoses(body)
  const list = d.diagnoses.map((x) => `${x.code} ${x.label} (${x.certainty})`).join('; ')
  return `${list} ${plainText(d.notes)}`.trim()
}

export function estimatePages(texts: string[]): number {
  return texts.reduce((n, t) => n + [...t].length, 0) / CHARS_PER_PAGE
}
