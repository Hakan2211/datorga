import { describe, expect, it } from 'vitest'
import { addChip, allSectionsDone, hasChip, nextOpenSection, sectionState } from './lotse'

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
