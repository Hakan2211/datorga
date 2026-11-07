import { describe, expect, it } from 'vitest'
import { blankTemplate, chipsFromText, emptySection, hintsFromText, isOwn, move, putDraft, getDraft, slugKey, templateProblems, uniqueKey } from './templates'

describe('own templates', () => {
  it('makes keys like the backend', () => {
    expect(slugKey('Psychischer Befund')).toBe('psychischer_befund')
    expect(slugKey('Größe & Übel')).toBe('groesse_uebel')
    expect(slugKey('1990')).toBe('abschnitt_1990')
    expect(slugKey('')).toBe('abschnitt')
    expect(uniqueKey('Befund', ['befund', 'befund_2'])).toBe('befund_3')
  })

  it('starts a blank own template that passes the checks', () => {
    const t = blankTemplate('Mein Brief')
    expect(isOwn(t)).toBe(true)
    expect(isOwn('drv-befund')).toBe(false)
    expect(t.key).toMatch(/^eigen-mein-brief-/)
    expect(templateProblems(t)).toEqual([])
    expect(templateProblems({ ...t, title: ' ', category: 'bericht' })).toEqual(['title', 'audience'])
    const diag = { ...emptySection([]), field: 'diagnoses' as const, ai_allowed: true }
    expect(templateProblems({ ...t, sections: [diag] })).toContain('diagnoses')
  })

  it('edits lists', () => {
    expect(move([1, 2, 3], 0, 1)).toEqual([2, 1, 3])
    expect(move([1, 2, 3], 0, -1)).toEqual([1, 2, 3])
    const old = [{ text: 'A', when: { method: ['VT' as const] } }]
    expect(hintsFromText('A\n\n B ', old)).toEqual([old[0], { text: 'B', when: {} }])
    expect(chipsFromText('keine, nicht bekannt,,')).toEqual(['keine', 'nicht bekannt'])
  })

  it('hands drafts to the editor', () => {
    const t = blankTemplate('x')
    expect(getDraft(putDraft(t))).toBe(t)
    expect(getDraft('nope')).toBeUndefined()
  })
})
