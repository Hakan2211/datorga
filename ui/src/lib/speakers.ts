import type { TranscriptSegment } from '@/bindings/TranscriptSegment'

/** Speaker labels as the backend stores them (`core::diarize`). */
export const THERAPIST = 'therapist'
export const PATIENT = 'patient'

/** Distinct labels of a recording: therapist, patient, then "Sprecher n" in number order. */
export function speakerLabels(segments: Pick<TranscriptSegment, 'speaker'>[]): string[] {
  const set = new Set(segments.map((s) => s.speaker).filter((s): s is string => !!s))
  const rank = (l: string) => (l === THERAPIST ? 0 : l === PATIENT ? 1 : 2)
  return [...set].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b, 'de', { numeric: true }))
}
