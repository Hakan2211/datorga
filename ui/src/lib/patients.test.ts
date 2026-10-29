import { describe, expect, it } from 'vitest'
import type { PatientCard } from '@/bindings/PatientCard'
import { ageOn, chaptersIn, icdChapter, initials, matches, noFilter, sortCards } from './patients'

function card(last: string, patch: Partial<PatientCard> = {}, birth: string | null = null, gender: 'w' | 'm' | 'd' | null = null): PatientCard {
  return {
    patient: {
      id: last,
      external_ref: null,
      first_name: 'A',
      last_name: last,
      birth_date: birth,
      chiffre: null,
      pronouns: null,
      gender,
      consent_recording: false,
      status: 'active',
      created_at: '2026-01-01T00:00:00Z',
      archived_at: null,
    },
    has_photo: false,
    method: null,
    age_group: null,
    diagnoses: [],
    next_at: null,
    last_at: null,
    open_items: 0,
    ...patch,
  }
}

const now = new Date(2026, 9, 29)

describe('patients', () => {
  it('computes age in full years', () => {
    expect(ageOn('1988-10-29', now)).toBe(38)
    expect(ageOn('1988-10-30', now)).toBe(37)
    expect(ageOn(null, now)).toBeNull()
    expect(ageOn('kaputt', now)).toBeNull()
  })

  it('maps ICD codes to chapters', () => {
    expect(icdChapter('F32.1')).toBe('F3')
    expect(icdChapter(' f41.0')).toBe('F4')
    expect(icdChapter('Z73')).toBe('other')
    expect(chaptersIn([card('a', { diagnoses: [{ code: 'Z73', label: '', certainty: 'G' }, { code: 'F41.1', label: '', certainty: 'G' }] }), card('b', { diagnoses: [{ code: 'F32.1', label: '', certainty: 'V' }] })])).toEqual(['F3', 'F4', 'other'])
  })

  it('filters by age group (episode first, else age), gender, method and chapter', () => {
    const teen = card('Teen', {}, '2010-01-01')
    const kjpEpisode = card('Kjp', { age_group: 'kjp', method: 'VT' }, '1990-01-01')
    const adult = card('Adult', { method: 'TP', diagnoses: [{ code: 'F33.0', label: '', certainty: 'G' }] }, '1980-01-01', 'w')
    expect(matches(teen, { ...noFilter, ageGroup: 'kjp' }, now)).toBe(true)
    expect(matches(kjpEpisode, { ...noFilter, ageGroup: 'kjp' }, now)).toBe(true)
    expect(matches(adult, { ...noFilter, ageGroup: 'kjp' }, now)).toBe(false)
    expect(matches(adult, { ...noFilter, gender: 'w', method: 'TP', chapter: 'F3' }, now)).toBe(true)
    expect(matches(adult, { ...noFilter, chapter: 'F4' }, now)).toBe(false)
    expect(matches(card('x'), { ...noFilter, gender: 'w' }, now)).toBe(false)
  })

  it('sorts by name, next appointment and last seen', () => {
    const a = card('Adler', { next_at: '2026-11-05T09:00:00Z', last_at: '2026-10-01T09:00:00Z' })
    const b = card('Berg', { next_at: '2026-11-01T09:00:00Z' })
    const c = card('Cem', { last_at: '2026-10-20T09:00:00Z' })
    expect(sortCards([c, b, a], 'name').map((x) => x.patient.last_name)).toEqual(['Adler', 'Berg', 'Cem'])
    expect(sortCards([a, b, c], 'next').map((x) => x.patient.last_name)).toEqual(['Berg', 'Adler', 'Cem'])
    expect(sortCards([a, b, c], 'last').map((x) => x.patient.last_name)).toEqual(['Cem', 'Adler', 'Berg'])
  })

  it('builds initials', () => {
    expect(initials('anna', 'Kraus')).toBe('AK')
    expect(initials('', '')).toBe('')
  })
})
