import { useState, type ReactNode } from 'react'
import { Link } from '@tanstack/react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { AlertTriangle, CheckCircle2, ChevronDown, Info, ShieldCheck, Sparkles, XCircle } from 'lucide-react'
import type { CheckItem } from '@/bindings/CheckItem'
import type { ReviewHint } from '@/bindings/ReviewHint'
import { formatDate } from '@/components/bits'
import { RedactionDialog } from '@/components/fact-draft'
import { PatientFormDialog } from '@/components/patient-form'
import { ReportPreview } from '@/components/report-preview'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { api, errorMessage, q, type ReportView, type ReviewView } from '@/lib/api'
import { checkEntries, type CheckEntry } from '@/lib/lotse'
import { parseDiagnoses } from '@/lib/report'
import { cn } from '@/lib/utils'

/**
 * Lotse step 4: one ranked list of what is still open before signing. The app's own checks
 * (empty sections, names, Chiffre, length, diagnoses) and the optional AI Gutachter-Check, whose
 * hints never change the text: "Zum Abschnitt" leads back to the section, "Ignorieren" folds a
 * hint away.
 */
export function CheckStep({
  view,
  bodies,
  content,
  flush,
  readOnly,
  llmActive,
  onSection,
  onChange,
}: {
  view: ReportView
  bodies: Record<string, string>
  /** Changes whenever the text changes (re-renders the preview). */
  content: string
  flush: () => Promise<void>
  readOnly: boolean
  llmActive: boolean
  onSection: (key: string) => void
  onChange: (key: string, body: string) => void
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const { report, template, patient, episode } = view
  const checklist = useQuery(q.checklist(report.id))
  const review = useQuery(q.review(report.id))
  const [checking, setChecking] = useState(false)
  const [showIgnored, setShowIgnored] = useState(false)

  const diagDef = template.sections.find((s) => s.field === 'diagnoses')
  const reportDiagnoses = diagDef ? parseDiagnoses(bodies[diagDef.key] ?? '') : null
  const entries = checkEntries({
    checklist: checklist.data ?? [],
    hints: review.data?.review?.hints ?? [],
    reportDiagnoses: reportDiagnoses?.diagnoses ?? null,
    episodeDiagnoses: episode?.diagnoses ?? [],
  })
  const ignored = review.data?.review?.hints.filter((h) => h.ignored) ?? []
  const passed = (checklist.data ?? []).filter((c) => c.ok && c.severity === 'block')
  const blocking = (checklist.data ?? []).some((c) => !c.ok && c.severity === 'block')
  const heading = (key: string | null) => {
    const def = key ? template.sections.find((s) => s.key === key) : null
    return def ? (def.number ?? def.heading) : null
  }

  const ignore = (h: ReviewHint, on: boolean) =>
    api
      .reviewIgnore(report.id, h.id, on)
      .then((r) => qc.setQueryData(q.review(report.id).queryKey, r))
      .catch((e) => toast.error(errorMessage(e)))

  const action = (e: CheckEntry): ReactNode => {
    if (readOnly) return null
    switch (e.kind) {
      case 'check':
        if (e.item.key === 'chiffre')
          return <PatientFormDialog patient={patient} trigger={<Button size="sm" variant="outline">{t('lotse.chiffreSet')}</Button>} />
        return e.item.section_key ? <ToSection onClick={() => onSection(e.item.section_key!)} /> : null
      case 'diagnoses':
        return (
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              onChange(diagDef!.key, JSON.stringify({ diagnoses: episode!.diagnoses, notes: reportDiagnoses!.notes }))
              toast.success(t('lotse.diagnosesTaken'))
            }}
            data-testid="check-take-diagnoses"
          >
            {t('lotse.takeOver')}
          </Button>
        )
      case 'hint':
        return (
          <>
            {e.hint.section_key && <ToSection onClick={() => onSection(e.hint.section_key!)} />}
            <Button size="sm" variant="ghost" onClick={() => void ignore(e.hint, true)} data-testid="hint-ignore">
              {t('lotse.check.ignore')}
            </Button>
          </>
        )
    }
  }

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,7fr)_minmax(0,4fr)]" data-testid="lotse-check">
      <Card className="self-start">
        <CardHeader>
          <CardTitle className="text-base">{t('lotse.check.title')}</CardTitle>
          <CardDescription>{t('lotse.check.subtitle')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {checklist.data && !blocking && (
            <p className="flex items-center gap-2 rounded-md bg-success/10 px-3 py-2.5 text-sm text-success" data-testid="check-passed">
              <ShieldCheck className="size-4 shrink-0" />
              {passedLine(passed, t)}
            </p>
          )}
          {entries.length === 0 && checklist.data && (
            <p className="text-sm text-muted-foreground">{review.data?.review ? t('lotse.check.nothingLeft') : t('lotse.check.nothingYet')}</p>
          )}
          <ul className="space-y-2.5" data-testid="check-list">
            {entries.map((e) => {
              const section = e.kind === 'hint' ? e.hint.section_key : e.kind === 'check' ? e.item.section_key : (diagDef?.key ?? null)
              const badge = section ? heading(section) : e.kind === 'hint' ? t('lotse.check.whole') : null
              return (
                <li
                  key={e.id}
                  className={cn(
                    'flex items-start gap-3 rounded-lg border px-3.5 py-3',
                    e.tone === 'block' && 'border-destructive/40 bg-destructive/5',
                    e.tone === 'warn' && 'border-warning/50 bg-warning/8',
                  )}
                  data-testid="check-entry"
                  data-kind={e.kind}
                  data-tone={e.tone}
                >
                  <EntryIcon tone={e.tone} kind={e.kind} />
                  <div className="min-w-0 flex-1 space-y-2">
                    <p className="text-sm">
                      {badge && (
                        <span className="mr-2 inline-flex min-w-6 justify-center rounded bg-muted px-1.5 py-0.5 text-xs font-medium text-muted-foreground tabular-nums">
                          {badge}
                        </span>
                      )}
                      {e.kind === 'hint' ? e.hint.text : e.kind === 'check' ? e.item.message : t('lotse.diagnosesChanged')}
                    </p>
                    <div className="flex flex-wrap gap-1.5 empty:hidden">{action(e)}</div>
                  </div>
                </li>
              )
            })}
          </ul>
          {ignored.length > 0 && (
            <div className="space-y-2">
              <button
                type="button"
                className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
                onClick={() => setShowIgnored((v) => !v)}
                data-testid="check-ignored-toggle"
              >
                <ChevronDown className={cn('size-3.5 transition-transform', !showIgnored && '-rotate-90')} />
                {t('lotse.check.ignored', { count: ignored.length })}
              </button>
              {showIgnored && (
                <ul className="space-y-1.5 pl-5 text-[13px] text-muted-foreground">
                  {ignored.map((h) => (
                    <li key={h.id} className="flex items-start gap-2">
                      <span className="flex-1">
                        {h.section_key && <span className="mr-1.5 tabular-nums">{heading(h.section_key)}</span>}
                        {h.text}
                      </span>
                      {!readOnly && (
                        <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" onClick={() => void ignore(h, false)}>
                          {t('lotse.check.restore')}
                        </Button>
                      )}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
          <AiLine
            review={review.data}
            readOnly={readOnly}
            llmActive={llmActive}
            onCheck={() => flush().then(() => setChecking(true), (e) => toast.error(errorMessage(e)))}
          />
        </CardContent>
      </Card>

      <div className="sticky top-4 h-[calc(100vh-14rem)] min-h-96 self-start">
        <ReportPreview reportId={report.id} content={content} flush={flush} />
      </div>

      {checking && (
        <RedactionDialog
          reportId={report.id}
          patientId={patient.id}
          previewKey={['reviewPreview', report.id]}
          loadPreview={() => api.reviewPreview(report.id)}
          progressEvent="review-progress"
          labels={{
            send: t('lotse.check.send'),
            runLocal: t('lotse.check.runLocal'),
            running: t('lotse.check.running'),
            part: () => t('lotse.check.running'),
            parts: () => t('lotse.check.parts', { count: review.data?.sections ?? 0 }),
          }}
          onRun={async (digest, requestId) => {
            const r = await api.reviewRun(report.id, digest, requestId)
            qc.setQueryData(q.review(report.id).queryKey, r)
            const n = r.review?.hints.length ?? 0
            toast.success(n ? t('lotse.check.done', { count: n }) : t('lotse.check.doneNone'))
          }}
          onClose={() => setChecking(false)}
        />
      )}
    </div>
  )
}

/** "Pflichtangaben vollständig · Chiffre gesetzt · keine Klarnamen im Text" from the passed checks. */
function passedLine(passed: CheckItem[], t: (k: string) => string): string {
  const parts = []
  if (passed.some((c) => c.key.startsWith('filled:'))) parts.push(t('lotse.check.passedFilled'))
  if (passed.some((c) => c.key === 'chiffre')) parts.push(t('lotse.check.passedChiffre'))
  if (passed.some((c) => c.key.startsWith('pseudonym:'))) parts.push(t('lotse.check.passedNames'))
  return parts.join(' · ')
}

function ToSection({ onClick }: { onClick: () => void }) {
  const { t } = useTranslation()
  return (
    <Button size="sm" variant="outline" onClick={onClick} data-testid="to-section">
      {t('lotse.check.toSection')}
    </Button>
  )
}

function EntryIcon({ tone, kind }: { tone: CheckEntry['tone']; kind: CheckEntry['kind'] }) {
  const cls = 'mt-0.5 size-4 shrink-0'
  if (tone === 'block') return <XCircle className={cn(cls, 'text-destructive')} />
  if (kind === 'hint') return <Sparkles className={cn(cls, tone === 'warn' ? 'text-warning-foreground' : 'text-muted-foreground')} />
  if (tone === 'warn') return <AlertTriangle className={cn(cls, 'text-warning-foreground')} />
  return <Info className={cn(cls, 'text-muted-foreground')} />
}

/** The AI part: run the check, or when it ran and whether the text changed since. */
function AiLine({
  review,
  readOnly,
  llmActive,
  onCheck,
}: {
  review: ReviewView | undefined
  readOnly: boolean
  llmActive: boolean
  onCheck: () => void
}) {
  const { t } = useTranslation()
  if (!review) return null
  const r = review.review
  const canRun = !readOnly && llmActive && review.sections > 0
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 border-t pt-3 text-[13px]" data-testid="check-ai">
      <p className={cn('flex items-center gap-2', r && review.stale ? 'text-warning-foreground' : 'text-muted-foreground')}>
        {r ? <CheckCircle2 className="size-4 shrink-0 text-primary" /> : <Sparkles className="size-4 shrink-0" />}
        {r
          ? `${t('lotse.check.checkedAt', { date: formatDate(r.checked_at, true), model: r.model })}${review.stale ? ` ${t('lotse.check.stale')}` : ''}`
          : readOnly
            ? t('lotse.check.notRun')
            : !llmActive
              ? t('lotse.check.noModel')
              : review.sections === 0
                ? t('lotse.check.noText')
                : t('lotse.check.hint')}
      </p>
      {canRun ? (
        <Button size="sm" variant="outline" onClick={onCheck} data-testid="check-run">
          <Sparkles /> {r ? t('lotse.check.again') : t('lotse.check.run')}
        </Button>
      ) : (
        !readOnly &&
        !llmActive && (
          <Button asChild size="sm" variant="outline">
            <Link to="/settings">{t('lotse.modelSet')}</Link>
          </Button>
        )
      )}
    </div>
  )
}
