import { describe, expect, it } from 'vitest'
import { addChip, allSectionsDone, checkEntries, hasChip, nextOpenSection, sectionState } from './lotse'

describe('answer chips', () => {
  it('fill, extend and toggle an answer', () => {
    expect(addChip('', 'keine')).toBe('keine')
    expect(addChip('Sertralin 50 mg', 'nicht bekannt')).toBe('Sertralin 50 mg; nicht bekannt')
    expect(addChip('ledig;', 'keine Kinder')).toBe('ledig; keine Kinder')
    expect(addChip('ledig; keine Kinder', 'Keine Kinder')).toBe('ledig')
    expect(addChip('keine', 'keine')).toBe('')
  })

  it('count as chosen only as a whole part', () => {
    expect(hasChip('ledig; keine Kinder', 'keine Kinder')).toBe(true)
    expect(hasChip('keine Angabe', 'keine')).toBe(false)
    expect(hasChip('', 'keine')).toBe(false)
  })
})

describe('focus mode sections', () => {
  it('state: left out wins, then a waiting proposal, then text', () => {
    expect(sectionState({ notRelevant: true, proposal: true, filled: false })).toBe('skipped')
    expect(sectionState({ notRelevant: false, proposal: true, filled: true })).toBe('proposal')
    expect(sectionState({ notRelevant: false, proposal: false, filled: true })).toBe('done')
    expect(sectionState({ notRelevant: false, proposal: false, filled: false })).toBe('open')
  })

  it('done when every section is taken over or left out', () => {
    expect(allSectionsDone(['done', 'skipped'])).toBe(true)
    expect(allSectionsDone(['done', 'proposal'])).toBe(false)
    expect(allSectionsDone([])).toBe(false)
  })

  it('next open section wraps around and skips finished ones', () => {
    const states = { a: 'done', b: 'open', c: 'skipped', d: 'proposal' } as const
    expect(nextOpenSection(['a', 'b', 'c', 'd'], states, 'b')).toBe('d')
    expect(nextOpenSection(['a', 'b', 'c', 'd'], states, 'd')).toBe('b')
    expect(nextOpenSection(['a', 'c'], { a: 'done', c: 'skipped' }, 'a')).toBe(null)
  })
})

describe('step 4 list', () => {
  const check = (key: string, ok: boolean, severity = 'block') => ({ key, ok, severity, message: key, section_key: null })
  const hint = (id: string, level: 'important' | 'note', ignored = false) => ({ id, section_key: null, level, text: id, ignored })
  const d = (code: string) => ({ code, label: code, certainty: 'G' })

  it('ranks blocks, then likely objections, then notes and the length', () => {
    const entries = checkEntries({
      checklist: [check('length', true, 'warn'), check('filled:a', true), check('pseudonym:a', false), check('chiffre', true)],
      hints: [hint('n1', 'note'), hint('i1', 'important'), hint('x', 'important', true)],
      reportDiagnoses: [d('F41.0')],
      episodeDiagnoses: [d('F41.0'), d('F32.1')],
    })
    expect(entries.map((e) => `${e.kind}:${e.id}:${e.tone}`)).toEqual([
      'check:pseudonym:a:block',
      'diagnoses:diagnoses:warn',
      'hint:i1:warn',
      'hint:n1:info',
      'check:length:info',
    ])
  })

  it('leaves out passed checks, same diagnoses and ignored hints', () => {
    const entries = checkEntries({
      checklist: [check('filled:a', true), check('length', false, 'warn')],
      hints: [hint('x', 'note', true)],
      reportDiagnoses: [d('f41.0')],
      episodeDiagnoses: [d('F41.0')],
    })
    expect(entries.map((e) => `${e.id}:${e.tone}`)).toEqual(['length:warn'])
  })
})
