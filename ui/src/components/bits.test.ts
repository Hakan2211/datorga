import { describe, expect, it } from 'vitest'
import { stepStates } from './bits'

describe('stepStates', () => {
  it('makes the first unfinished step current', () => {
    expect(
      stepStates([
        { key: 'a', done: true },
        { key: 'b', done: false },
        { key: 'c', done: true },
      ]),
    ).toEqual({ a: 'done', b: 'current', c: 'done' })
  })

  it('takes an explicit current step over the derived one', () => {
    expect(
      stepStates(
        [
          { key: 'a', done: false },
          { key: 'b', done: false },
        ],
        'b',
      ),
    ).toEqual({ a: 'todo', b: 'current' })
  })

  it('has no current step when everything is done', () => {
    expect(stepStates([{ key: 'a', done: true }])).toEqual({ a: 'done' })
  })
})
