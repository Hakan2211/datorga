import { describe, expect, it } from 'vitest'
import { speakerLabels } from './speakers'

describe('speakerLabels', () => {
  it('lists each label once, therapist and patient first', () => {
    const segs = ['speaker_2', null, 'patient', 'speaker_1', 'therapist', 'patient', 'speaker_10'].map((speaker) => ({ speaker }))
    expect(speakerLabels(segs)).toEqual(['therapist', 'patient', 'speaker_1', 'speaker_2', 'speaker_10'])
  })

  it('is empty without speakers', () => {
    expect(speakerLabels([{ speaker: null }])).toEqual([])
  })
})
