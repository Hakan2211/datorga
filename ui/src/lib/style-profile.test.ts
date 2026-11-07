import { describe, expect, it } from 'vitest'
import type { Computed } from '@/bindings/Computed'
import { mappedLabel, profileSummary } from './style-profile'

const t = (key: string, opts?: Record<string, unknown>) => (opts ? `${key}:${JSON.stringify(opts)}` : key)

describe('style profile', () => {
  it('summarizes the computed profile', () => {
    const c: Computed = { exemplars: 2, sentence_words: 14, voice: 'ich', form: 'fliesstext', person_term: 'Pat.', sections: [], phrases: [] }
    expect(profileSummary(c, t)).toEqual(['styleProfile.sentence:{"count":14}', 'styleProfile.voice.ich', '„Pat.“', 'styleProfile.form.fliesstext'])
    expect(profileSummary({ ...c, sentence_words: 0, person_term: null }, t)).toEqual(['styleProfile.voice.ich', 'styleProfile.form.fliesstext'])
    expect(profileSummary(null, t)).toEqual([])
  })

  it('labels how an exemplar was split', () => {
    const mapped: [string, number][] = [['a', 3], ['b', 0]]
    expect(mappedLabel(mapped, 'a', t)).toBe('styleProfile.mapped:{"count":3}')
    expect(mappedLabel(mapped, 'b', t)).toBe('styleProfile.whole')
    expect(mappedLabel(mapped, 'c', t)).toBeNull()
  })
})
