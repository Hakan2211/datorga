// Voice mode (slice 24): a mic in the editor toolbar. Dictated text appears at the cursor piece by
// piece; a grey "…" marks the piece still being recognized. Only the therapist's voice.
import { useEffect, useId } from 'react'
import type { Editor } from '@tiptap/react'
import { useTranslation } from 'react-i18next'
import { Loader2, Mic, MicOff } from 'lucide-react'
import { useCaptureDevice } from '@/lib/capture'
import { registerTarget, startDictation, stopDictation, targetPhase, useDictation } from '@/lib/dictation'
import { insertDictated, setDictationPending } from '@/lib/editor'
import { cn } from '@/lib/utils'

export function DictationButton({ editor, reportId }: { editor: Editor; reportId: string }) {
  const { t } = useTranslation()
  const id = useId()
  const d = useDictation()
  const { startId } = useCaptureDevice()
  const phase = targetPhase(d, id)
  const on = d.targetId === id

  useEffect(() => registerTarget(id, (text) => insertDictated(editor, text)), [id, editor])
  useEffect(() => setDictationPending(editor, phase === 'recognizing'), [editor, phase])

  const label = on ? t('dictation.stop') : t('dictation.start')
  const level = on ? Math.min(1, (d.status?.level ?? 0) * 3) : 0
  return (
    <button
      type="button"
      title={`${label} · ${t(`dictation.phase.${phase}`)}`}
      aria-label={label}
      aria-pressed={on}
      data-testid="dictation-button"
      data-phase={phase}
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => void (on ? stopDictation() : startDictation(id, reportId, startId))}
      className={cn(
        'ml-auto flex items-center gap-1.5 rounded px-2 py-1.5 text-xs text-muted-foreground hover:bg-muted hover:text-foreground',
        on && 'bg-destructive/10 text-destructive hover:bg-destructive/15 hover:text-destructive',
      )}
      style={on ? { boxShadow: `0 0 0 ${1 + level * 3}px color-mix(in oklch, var(--destructive) 35%, transparent)` } : undefined}
    >
      {phase === 'starting' || phase === 'loading' ? (
        <Loader2 className="size-3.5 animate-spin" />
      ) : on ? (
        <MicOff className="size-3.5" />
      ) : (
        <Mic className="size-3.5" />
      )}
      {on ? t(`dictation.phase.${phase}`) : t('dictation.short')}
    </button>
  )
}
