import { describe, expect, it } from 'vitest'
import type { LetterSummary } from '@/bindings/LetterSummary'
import type { Template } from '@/bindings/Template'
import {
  EMPTY_PRACTICE,
  blockSnippet,
  germanDay,
  letterTemplates,
  linesFromText,
  matchesLetter,
  orderBlocks,
  placeDate,
  practiceLine,
  toPlaceholders,
} from './letters'

const tpl = (key: string, title: string, layout: 'letter' | 'bericht', recipients: Template['recipients'] = []): Template => ({
  key,
  version: '1',
  title,
  source: null,
  report_kinds: [],
  category: layout === 'letter' ? 'brief' : 'ptv3',
  layout,
  refs: [],
  recipients,
  release: false,
  rules: { target_pages: null, keyword_style_allowed: false, signature_required: true, pseudonymized: false, numbered: false },
  intro: '',
  sections: [],
})

const letter = (over: Partial<LetterSummary>): LetterSummary => ({
  report_id: 'r',
  patient_id: 'p',
  patient_name: 'Anna Beispiel',
  episode_id: null,
  template_key: 'freier-brief',
  template_title: 'Freier Brief',
  recipient_name: 'Landratsamt',
  recipient_kind: null,
  subject: 'Befundbericht',
  status: 'draft',
  state: 'draft',
  created_at: '2026-11-07T10:00:00Z',
  updated_at: '2026-11-07T10:00:00Z',
  signed_at: null,
  exported_at: null,
  sent_at: null,
  sent_via: null,
  follow_up_on: null,
  ...over,
})

describe('letters', () => {
  it('builds the practice line like the backend', () => {
    expect(practiceLine({ ...EMPTY_PRACTICE, name: 'Praxis', street: 'Weg 1', postal_code: '12345', city: 'Ort' })).toBe('Praxis · Weg 1 · 12345 Ort')
    expect(practiceLine({ ...EMPTY_PRACTICE, city: 'Ort' })).toBe('Ort')
    expect(practiceLine(EMPTY_PRACTICE)).toBeNull()
  })

  it('offers letter templates, recommended first', () => {
    const list = letterTemplates(
      [tpl('ptv3', 'PTV 3', 'bericht'), tpl('frei', 'Freier Brief', 'letter', ['arzt']), tpl('best', 'Behandlungsbestätigung', 'letter', ['sonstige'])],
      'arzt',
    )
    expect(list.map((x) => [x.template.key, x.recommended])).toEqual([
      ['frei', true],
      ['best', false],
    ])
    expect(letterTemplates([tpl('frei', 'B', 'letter'), tpl('best', 'A', 'letter')], null).map((x) => x.template.key)).toEqual(['best', 'frei'])
    // "Neuer Bericht" → "Weitere Berichte": the bericht category only.
    const drv = { ...tpl('drv', 'Befundbericht Rentenversicherung', 'letter', ['rentenversicherung']), category: 'bericht' as const }
    expect(letterTemplates([drv, tpl('frei', 'B', 'letter')], null, ['bericht']).map((x) => x.template.key)).toEqual(['drv'])
  })

  it('filters by state and search', () => {
    expect(matchesLetter(letter({}), 'all', 'landrats')).toBe(true)
    expect(matchesLetter(letter({}), 'sent', '')).toBe(false)
    expect(matchesLetter(letter({ state: 'sent' }), 'sent', 'anna')).toBe(true)
    expect(matchesLetter(letter({}), 'all', 'xyz')).toBe(false)
  })

  it('formats dates and address lines', () => {
    expect(linesFromText(' Weg 1 \n\n12345 Ort\n')).toEqual(['Weg 1', '12345 Ort'])
    expect(germanDay('2026-11-07')).toBe('07.11.2026')
    expect(placeDate('Freiburg', '2026-11-07T08:00:00Z')).toBe('Freiburg, den 07.11.2026')
    expect(placeDate(' ', '2026-11-07')).toBe('07.11.2026')
  })
})

describe('Textbausteine (slice 48)', () => {
  const b = (id: string, keys: string[], section: string | null, uses: number) => ({ id, template_keys: keys, section_key: section, use_count: uses })
  it('orders blocks for the section first, most used first, and keeps other types out', () => {
    const all = [b('all', [], null, 3), b('drv', ['drv-befund'], null, 9), b('sec', ['drv-befund'], 'befund', 1), b('pflege', ['pflege-stellungnahme'], null, 99)]
    const o = orderBlocks(all, 'drv-befund', 'befund')
    expect(o.section.map((x) => x.id)).toEqual(['sec'])
    expect(o.rest.map((x) => x.id)).toEqual(['drv', 'all'])
    expect(orderBlocks(all, 'drv-befund', null).rest.map((x) => x.id)).toEqual(['drv', 'all', 'sec'])
  })
  it('shortens the text of a block', () => {
    expect(blockSnippet('<p>Eins</p><ul><li><p>zwei &amp; drei</p></li></ul>')).toBe('Eins zwei & drei')
    expect(blockSnippet(`<p>${'a'.repeat(200)}</p>`, 10)).toBe('aaaaaaaaa…')
  })
})

describe('saving letter text as a Textbaustein', () => {
  const lena = { first_name: 'Lena', last_name: 'Sommer', birth_date: '1996-03-14' }
  it('turns the name and birth date back into placeholders', () => {
    const r = toPlaceholders('<p>Frau Lena Sommer, geb. 14.03.1996, schläft schlecht. Lena Sommer kommt. Frau Sommer auch, Sommerzeit nicht.</p>', lena)
    expect(r.html).toBe('<p>{Frau/Herr} {Nachname}, geb. {Geburtsdatum}, schläft schlecht. {Name} kommt. {Frau/Herr} {Nachname} auch, Sommerzeit nicht.</p>')
    expect(r.replaced).toBe(true)
    expect(toPlaceholders('<p>Ohne Namen.</p>', lena)).toEqual({ html: '<p>Ohne Namen.</p>', replaced: false })
  })
})
