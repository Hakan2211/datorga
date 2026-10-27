import { describe, expect, it } from 'vitest'
import { appendParagraph, estimatePages, isBlank, parseDiagnoses, plainText, sectionText } from './report'

describe('report helpers', () => {
  it('treats an empty editor as blank', () => {
    expect(isBlank('')).toBe(true)
    expect(isBlank('<p></p>')).toBe(true)
    expect(isBlank('<p> &nbsp; </p>')).toBe(true)
    expect(isBlank('<p>x</p>')).toBe(false)
  })

  it('strips TipTap html', () => {
    expect(plainText('<p>Angst &amp; <strong>Unruhe</strong></p><ul><li><p>Schlaf</p></li></ul>')).toBe('Angst & Unruhe Schlaf')
  })

  it('reads diagnoses bodies and tolerates empty ones', () => {
    expect(parseDiagnoses('')).toEqual({ diagnoses: [], notes: '' })
    const body = JSON.stringify({ diagnoses: [{ code: 'F41.0', label: 'Panikstörung', certainty: 'G' }], notes: '<p>DD</p>' })
    expect(sectionText('diagnoses', body)).toBe('F41.0 Panikstörung (G) DD')
  })

  it('estimates pages like the core checklist', () => {
    expect(estimatePages(['x'.repeat(6600)])).toBe(2)
  })

  it('appends transcript text as an escaped paragraph', () => {
    expect(appendParagraph('', ' Konflikt mit <Chef> & Team ')).toBe('<p>Konflikt mit &lt;Chef&gt; &amp; Team</p>')
    expect(appendParagraph('<p>a</p>', 'b')).toBe('<p>a</p><p>b</p>')
    expect(appendParagraph('<p>a</p>', '  ')).toBe('<p>a</p>')
  })
})
