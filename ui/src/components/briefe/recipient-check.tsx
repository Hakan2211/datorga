import { useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { ShieldQuestion } from 'lucide-react'
import type { ReviewHint } from '@/bindings/ReviewHint'
import { SectionCard } from '@/components/bits'
import { AiLine, ReviewRun, useCheckText } from '@/components/lotse/check-step'
import { shortHeading } from '@/components/lotse/questions-step'
import { Button } from '@/components/ui/button'
import { api, errorMessage, q, type ReportView } from '@/lib/api'
import { cn } from '@/lib/utils'

/**
 * The Empfänger-Check on the letter page (slice 50): the AI reads the letter as its recipient
 * and lists what would be asked back. Hints only; the text is never changed.
 */
export function RecipientCheckCard({
  view,
  flush,
  llmActive,
  onSection,
}: {
  view: ReportView
  flush: () => Promise<void>
  llmActive: boolean
  onSection: (key: string) => void
}) {
  const { t } = useTranslation()
  const ck = useCheckText(true)
  const qc = useQueryClient()
  const { report, template, patient } = view
  const review = useQuery(q.review(report.id))
  const [checking, setChecking] = useState(false)
  const hints = review.data?.review?.hints ?? []
  const open = hints.filter((h) => !h.ignored)
  const ignored = hints.length - open.length

  const ignore = (h: ReviewHint, on: boolean) =>
    api
      .reviewIgnore(report.id, h.id, on)
      .then((r) => qc.setQueryData(q.review(report.id).queryKey, r))
      .catch((e) => toast.error(errorMessage(e)))
  const heading = (key: string | null) => {
    const def = key ? template.sections.find((s) => s.key === key) : null
    return def ? shortHeading(def.heading) || t('letterAi.opening') : ck('whole')
  }

  return (
    <SectionCard icon={ShieldQuestion} title={ck('title')} contentClassName="space-y-3" tour="recipient-check">
      {open.length > 0 && (
        <ul className="space-y-2" data-testid="recipient-check-hints">
          {open.map((h) => (
            <li
              key={h.id}
              className={cn('rounded-md border px-3 py-2 text-[13px]', h.level === 'important' && 'border-warning/50 bg-warning/8')}
              data-testid="recipient-hint"
            >
              <p>
                <span className="mr-1.5 rounded bg-muted px-1.5 py-0.5 text-xs font-medium text-muted-foreground">{heading(h.section_key)}</span>
                {h.text}
              </p>
              <div className="mt-1.5 flex gap-1.5">
                {h.section_key && (
                  <Button size="sm" variant="outline" className="h-7" onClick={() => onSection(h.section_key!)}>
                    {t('lotse.check.toSection')}
                  </Button>
                )}
                <Button size="sm" variant="ghost" className="h-7" onClick={() => void ignore(h, true)} data-testid="recipient-hint-ignore">
                  {t('lotse.check.ignore')}
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {review.data?.review && open.length === 0 && <p className="text-[13px] text-muted-foreground">{t('lotse.check.nothingLeft')}</p>}
      {ignored > 0 && (
        <p className="text-xs text-muted-foreground">
          {t('lotse.check.ignored', { count: ignored })}{' '}
          <button
            type="button"
            className="underline hover:text-foreground"
            onClick={() => hints.filter((h) => h.ignored).forEach((h) => void ignore(h, false))}
          >
            {t('lotse.check.restore')}
          </button>
        </p>
      )}
      <AiLine
        review={review.data}
        isLetter
        readOnly={false}
        llmActive={llmActive}
        onCheck={() => flush().then(() => setChecking(true), (e) => toast.error(errorMessage(e)))}
      />
      {checking && <ReviewRun reportId={report.id} patientId={patient.id} isLetter onClose={() => setChecking(false)} />}
    </SectionCard>
  )
}
