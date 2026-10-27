import { describe, expect, it } from 'vitest'
import { estimatePages, parseDiagnoses, plainText, sectionText } from './report'

describe('report helpers', () => {
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
})
