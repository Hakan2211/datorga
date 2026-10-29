import { describe, expect, it } from 'vitest'
import { addChip, hasChip } from './lotse'

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
