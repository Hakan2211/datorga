import { useTranslation } from 'react-i18next'
import type { LetterState } from '@/bindings/LetterState'
import { StatusPill } from '@/components/bits'

const STATE_TONE: Record<LetterState, 'neutral' | 'success' | 'primary'> = { draft: 'neutral', signed: 'success', sent: 'primary' }

/** Entwurf · Fertig · Versendet */
export function LetterStatePill({ state }: { state: LetterState }) {
  const { t } = useTranslation()
  return <StatusPill tone={STATE_TONE[state]}>{t(`enums.letter_state.${state}`)}</StatusPill>
}
