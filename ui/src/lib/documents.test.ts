import { describe, expect, it, vi } from 'vitest'
import type { StyleMark } from '@/bindings/StyleMark'
import type { Template } from '@/bindings/Template'

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }))

const { appendText, placeholderCount, reviewParts, styleTypes, toggleKeep } = await import('./documents')

const mark = (start: number, end: number, text: string): StyleMark => ({ start, end, text, kind: 'person' })

describe('documents', () => {
  it('appends imported text after a blank line', () => {
    expect(appendText('', '  Neu  ')).toBe('Neu')
    expect(appendText('Alt\n\n', 'Neu')).toBe('Alt\n\nNeu')
    expect(appendText('Alt', '   ')).toBe('Alt')
  })

  it('splits the review text into parts and marks', () => {
    const text = 'Frau Muster kam. Muster bleibt.'
    const parts = reviewParts(text, [mark(17, 23, 'Muster'), mark(5, 11, 'Muster')], ['muster'])
    expect(parts.map((p) => p.text)).toEqual(['Frau ', 'Muster', ' kam. ', 'Muster', ' bleibt.'])
    expect(parts.filter((p) => p.mark).every((p) => p.kept)).toBe(true)
    expect(reviewParts('ab', [mark(0, 5, 'x')], [])).toEqual([{ text: 'ab' }])
  })

  it('toggles a kept surface text case-insensitively', () => {
    expect(toggleKeep([], ' Tübingen ')).toEqual(['Tübingen'])
    expect(toggleKeep(['Tübingen'], 'tübingen')).toEqual([])
  })

  it('offers PTV 3 first, never the session note', () => {
    const t = (key: string, title: string, category: Template['category']) => ({ key, title, category }) as Template
    const keys = styleTypes([t('b', 'Befundbericht', 'bericht'), t('n', 'Sitzungsnotiz', 'note'), t('p', 'PTV 3', 'ptv3'), t('a', 'Arztbrief', 'bericht')]).map((x) => x.key)
    expect(keys).toEqual(['p', 'a', 'b'])
  })

  it('counts placeholders', () => {
    expect(placeholderCount('Frau [Name] [Name], geb. [Datum], Az. [Zeichen] [PERSON_1]')).toBe(4)
  })
})
