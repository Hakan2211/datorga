// One section of a report: heading, PTV 3 hints and its editor (prose or diagnoses). Used by the
// Bericht page's full view and the Lotse focus mode.
import { useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, CircleSlash } from 'lucide-react'
import type { SectionDef } from '@/bindings/SectionDef'
import { DiagnosisRows } from '@/components/diagnoses'
import { RichField } from '@/components/rich-field'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { parseDiagnoses } from '@/lib/report'
import { cn } from '@/lib/utils'

export function SectionEditor({
  def,
  body,
  readOnly,
  active = false,
  onFocus,
  onChange,
  action,
  above,
  below,
  dictateFor,
  notRelevant = false,
  onRelevance,
}: {
  def: SectionDef
  body: string
  readOnly: boolean
  /** Marked "nicht relevant": the editor collapses to a note. */
  notRelevant?: boolean
  /** Set for an optional section that may be marked "nicht relevant". */
  onRelevance?: (notRelevant: boolean) => void
  /** Report id: voice mode in this section's toolbar. */
  dictateFor?: string
  active?: boolean
  onFocus?: () => void
  onChange: (body: string) => void
  /** Next to the heading (e.g. "Entwurf…"). */
  action?: ReactNode
  /** Between the hints and the editor (e.g. an AI proposal). */
  above?: ReactNode
  /** Under the editor (e.g. sources of applied AI text). */
  below?: ReactNode
}) {
  const { t } = useTranslation()
  const [showHints, setShowHints] = useState(true)
  return (
    <section
      id={`section-${def.key}`}
      onFocusCapture={onFocus}
      className={cn('scroll-mt-6 space-y-2 border-l-2 border-transparent pl-3 -ml-3.5', active && 'border-primary/60')}
    >
      <div className="flex items-start justify-between gap-3">
        <h2 className={cn('font-serif text-[17px] leading-snug font-semibold', notRelevant && 'text-muted-foreground')}>
          {def.number && <span className="mr-1.5 text-primary">{def.number}.</span>}
          {def.heading}
        </h2>
        <div className="flex shrink-0 items-center gap-1">
          {onRelevance && (
            <Button
              variant="ghost"
              size="sm"
              className="h-7 text-xs text-muted-foreground"
              title={t('lotse.notRelevantHint')}
              onClick={() => onRelevance(!notRelevant)}
              data-testid={`relevance-${def.key}`}
            >
              {notRelevant ? t('lotse.markRelevant') : t('lotse.markNotRelevant')}
            </Button>
          )}
          {!notRelevant && action}
        </div>
      </div>
      {notRelevant ? (
        <p className="rounded-md border border-dashed px-3 py-2 text-[13px] text-muted-foreground">{t('lotse.notRelevantHint')}</p>
      ) : (
      <>
      {def.hints.length > 0 && (
        <div className="rounded-md bg-muted/60 px-3 py-2 text-[13px] text-muted-foreground">
          <button
            type="button"
            className="flex items-center gap-1 text-xs font-medium tracking-wide uppercase"
            onClick={() => setShowHints((v) => !v)}
          >
            <ChevronDown className={cn('size-3.5 transition-transform', !showHints && '-rotate-90')} />
            {t('report.hints')}
          </button>
          {showHints && (
            <ul className="mt-1.5 list-disc space-y-0.5 pl-5">
              {def.hints.map((h, i) => (
                <li key={i}>{h.text}</li>
              ))}
            </ul>
          )}
        </div>
      )}
      {!def.ai_allowed && (
        <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <CircleSlash className="size-3.5" /> {t('report.noAi')}
        </p>
      )}
      {above}
      {def.field === 'prose' ? (
        <RichField value={body} onChange={onChange} placeholder={t('report.placeholder')} readOnly={readOnly} dictateFor={dictateFor} />
      ) : (
        <DiagnosesEditor body={body} onChange={onChange} readOnly={readOnly} />
      )}
      {below}
      </>
      )}
    </section>
  )
}

export function DiagnosesEditor({ body, onChange, readOnly }: { body: string; onChange: (b: string) => void; readOnly: boolean }) {
  const { t } = useTranslation()
  const [state, setState] = useState(() => parseDiagnoses(body))
  const commit = (next: typeof state) => {
    setState(next)
    const empty = next.diagnoses.length === 0 && !next.notes
    onChange(empty ? '' : JSON.stringify(next))
  }

  return (
    <div className="space-y-3 rounded-md border bg-card p-3">
      <Label className="text-xs text-muted-foreground">{t('report.diagnoses')}</Label>
      <DiagnosisRows value={state.diagnoses} readOnly={readOnly} onChange={(diagnoses) => commit({ ...state, diagnoses })} />
      <Label className="block pt-2 text-xs text-muted-foreground">{t('report.notes')}</Label>
      <RichField value={state.notes} onChange={(notes) => commit({ ...state, notes })} readOnly={readOnly} />
    </div>
  )
}
