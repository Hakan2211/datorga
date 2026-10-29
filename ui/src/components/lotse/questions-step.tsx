import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react'
import { Link } from '@tanstack/react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Check, ChevronDown, Loader2, Mic, MicOff, Search } from 'lucide-react'
import type { AnswerSource } from '@/bindings/AnswerSource'
import { formatDate } from '@/components/bits'
import { RedactionDialog } from '@/components/fact-draft'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage, q, type QuestionView, type ReportQuestions } from '@/lib/api'
import { useCaptureDevice } from '@/lib/capture'
import { registerTarget, startDictation, stopDictation, targetPhase, useDictation } from '@/lib/dictation'
import { addChip, hasChip, isOpen, questionId } from '@/lib/lotse'
import { cn } from '@/lib/utils'

const SAVE_DELAY_MS = 700

/**
 * Lotse step 2: what the sessions don't say yet, asked one question at a time. Her answers go
 * into the draft as her own statements ("Ihre Angabe").
 */
export function QuestionsStep({
  reportId,
  patientId,
  filled,
  readOnly,
  llmActive,
}: {
  reportId: string
  patientId: string
  /** Sections with text already: their questions are optional and fold away. */
  filled: ReadonlySet<string>
  readOnly: boolean
  llmActive: boolean
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const data = useQuery(q.reportQuestions(reportId)).data
  const [checking, setChecking] = useState(false)
  if (!data) return null
  const covered = data.questions.filter((x) => !x.answer && x.covered_by.length > 0)
  const own = data.questions.filter((x) => x.answer || x.covered_by.length === 0)
  const asked = own.filter((x) => !filled.has(x.section_key))
  const written = own.filter((x) => filled.has(x.section_key))
  const answered = asked.filter((x) => x.answer).length
  const groups = groupBySection(asked)
  const canCheck = llmActive && !readOnly && data.checkable > 0 && data.facts > 0

  return (
    <div className="mx-auto max-w-3xl space-y-5" data-testid="lotse-questions">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-1">
          <p className="text-sm text-muted-foreground">{t('lotse.questions.intro')}</p>
          <p className="text-sm font-medium" data-testid="questions-progress">
            {t('lotse.questions.progress', { done: answered, total: asked.length })}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          {canCheck && (
            <Button variant="outline" size="sm" onClick={() => setChecking(true)} data-testid="questions-check">
              <Search /> {data.check ? t('lotse.questions.recheck') : t('lotse.questions.check')}
            </Button>
          )}
          {!readOnly && (
            <Button asChild variant="outline" size="sm">
              <Link to="/reports/$reportId/werkstatt" params={{ reportId }} data-testid="questions-dictate">
                <Mic /> {t('lotse.questions.freeDictation')}
              </Link>
            </Button>
          )}
        </div>
      </div>
      <CheckLine data={data} canCheck={canCheck} covered={covered.length} />

      {groups.map(([key, items]) => (
        <section key={key} className="space-y-3" data-section={key}>
          <h2 className="text-sm font-medium text-muted-foreground">
            {items[0].number} {shortHeading(items[0].heading)}
          </h2>
          {items.map((x) => (
            <QuestionCard key={questionId(x)} reportId={reportId} question={x} readOnly={readOnly} />
          ))}
        </section>
      ))}
      {asked.length === 0 && <p className="text-sm text-muted-foreground">{t('lotse.questions.none')}</p>}

      {written.length > 0 && (
        <details className="group rounded-lg border bg-card" data-testid="questions-written">
          <summary className="flex cursor-pointer items-center gap-2 px-4 py-3 text-sm">
            <ChevronDown className="size-4 text-muted-foreground transition-transform group-open:rotate-180" />
            {t('lotse.questions.written', { count: written.length })}
          </summary>
          <div className="space-y-4 border-t px-4 py-3">
            {groupBySection(written).map(([key, items]) => (
              <section key={key} className="space-y-3" data-section={key}>
                <h2 className="text-sm font-medium text-muted-foreground">
                  {items[0].number} {shortHeading(items[0].heading)}
                </h2>
                {items.map((x) => (
                  <QuestionCard key={questionId(x)} reportId={reportId} question={x} readOnly={readOnly} />
                ))}
              </section>
            ))}
          </div>
        </details>
      )}

      {covered.length > 0 && <CoveredList reportId={reportId} items={covered} readOnly={readOnly} />}

      {checking && (
        <RedactionDialog
          reportId={reportId}
          patientId={patientId}
          previewKey={['gapsPreview', reportId]}
          loadPreview={() => api.gapsPreview(reportId)}
          progressEvent="gaps-progress"
          labels={{
            send: t('lotse.questions.send'),
            runLocal: t('lotse.questions.runLocal'),
            running: t('lotse.questions.running'),
            part: () => t('lotse.questions.running'),
            parts: (count) => t('lotse.questions.parts', { count: data.checkable || count }),
          }}
          onRun={async (digest, requestId) => {
            const r = await api.gapsRun(reportId, digest, requestId)
            qc.setQueryData(q.reportQuestions(reportId).queryKey, r)
            const n = r.questions.filter((x) => !x.answer && x.covered_by.length > 0).length
            toast.success(n ? t('lotse.questions.checked', { count: n }) : t('lotse.questions.checkedNone'))
          }}
          onClose={() => setChecking(false)}
        />
      )}
    </div>
  )
}

function CheckLine({ data, canCheck, covered }: { data: ReportQuestions; canCheck: boolean; covered: number }) {
  const { t } = useTranslation()
  if (!data.check) {
    return canCheck ? <p className="text-[13px] text-muted-foreground">{t('lotse.questions.checkHint')}</p> : null
  }
  return (
    <p className={cn('text-[13px]', data.check.stale ? 'text-warning-foreground' : 'text-muted-foreground')} data-testid="questions-checked">
      {t('lotse.questions.checkedAt', { date: formatDate(data.check.checked_at, true), model: data.check.model, count: covered })}
      {data.check.stale && ` ${t('lotse.questions.stale')}`}
    </p>
  )
}

function groupBySection(items: QuestionView[]): [string, QuestionView[]][] {
  const groups = new Map<string, QuestionView[]>()
  for (const x of items) groups.set(x.section_key, [...(groups.get(x.section_key) ?? []), x])
  return [...groups.entries()]
}

/** The PTV 3 heading up to its first comma or parenthesis. */
export function shortHeading(h: string) {
  const i = h.search(/[,(]/)
  return (i > 0 ? h.slice(0, i) : h).trim()
}

export function QuestionCard({ reportId, question, readOnly }: { reportId: string; question: QuestionView; readOnly: boolean }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [text, setTextState] = useState(question.answer?.text ?? '')
  // Dictated pieces arrive outside React's render: they append to the latest text.
  const latest = useRef(text)
  const setText = (value: string) => {
    latest.current = value
    setTextState(value)
  }
  const [saved, setSaved] = useState(!!question.answer)
  const source = useRef<AnswerSource>(question.answer?.source ?? 'typed')
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined)
  const pending = useRef<string | null>(null)

  const save = useCallback(
    (value: string) => {
      clearTimeout(timer.current)
      pending.current = null
      api
        .setAnswer(reportId, question.section_key, question.ask_key, value, source.current)
        .then((answer) => {
          setSaved(!!answer)
          qc.setQueryData(q.reportQuestions(reportId).queryKey, (old) =>
            old && {
              ...old,
              questions: old.questions.map((x) => (questionId(x) === questionId(question) ? { ...x, answer } : x)),
            },
          )
          void qc.invalidateQueries({ queryKey: ['draftSources', reportId] })
        })
        .catch((e) => toast.error(errorMessage(e)))
    },
    [qc, reportId, question],
  )
  const change = (value: string, from: AnswerSource) => {
    source.current = from
    setText(value)
    setSaved(false)
    pending.current = value
    clearTimeout(timer.current)
    timer.current = setTimeout(() => save(value), SAVE_DELAY_MS)
  }
  const flush = () => {
    if (pending.current !== null) save(pending.current)
  }
  // Leaving the step must not drop an answer still waiting for its debounce.
  useEffect(() => () => flush(), []) // eslint-disable-line react-hooks/exhaustive-deps

  const chips = useMemo(() => [...(question.record ? [question.record] : []), ...question.chips], [question])
  const pick = (chip: string) => {
    source.current = text.trim() && source.current !== 'chip' ? source.current : 'chip'
    const value = addChip(text, chip)
    setText(value)
    save(value)
  }

  return (
    <Card className="gap-0 py-0" data-testid="question" data-ask={question.ask_key} data-answered={saved}>
      <CardContent className="space-y-2.5 p-4">
        <div className="flex items-start justify-between gap-3">
          <label htmlFor={`q-${questionId(question)}`} className="text-sm font-medium">
            {question.question}
          </label>
          {saved && (
            <span className="flex shrink-0 items-center gap-1 text-xs text-primary" data-testid="question-own">
              <Check className="size-3.5" /> {t('lotse.questions.own')}
            </span>
          )}
        </div>
        <div className="relative">
          <Textarea
            id={`q-${questionId(question)}`}
            value={text}
            disabled={readOnly}
            rows={text.length > 80 ? 3 : 1}
            placeholder={t('lotse.questions.placeholder')}
            className="min-h-9 resize-y pr-11"
            onChange={(e) => change(e.target.value, source.current === 'dictated' ? 'dictated' : 'typed')}
            onBlur={flush}
            data-testid="question-input"
          />
          {!readOnly && (
            <AnswerMic
              reportId={reportId}
              onText={(piece) => {
                const prev = latest.current
                change(prev.trim() ? `${prev.trimEnd()} ${piece}` : piece, 'dictated')
              }}
            />
          )}
        </div>
        {chips.length > 0 && !readOnly && (
          <div className="flex flex-wrap gap-1.5">
            {chips.map((c) => {
              const on = hasChip(text, c)
              return (
                <button
                  key={c}
                  type="button"
                  onClick={() => pick(c)}
                  aria-pressed={on}
                  className={cn(
                    'rounded-full border px-2.5 py-0.5 text-xs transition-colors',
                    on ? 'border-primary bg-primary text-primary-foreground' : 'text-muted-foreground hover:border-primary/50 hover:text-foreground',
                  )}
                  data-testid="question-chip"
                >
                  {c}
                </button>
              )
            })}
          </div>
        )}
      </CardContent>
    </Card>
  )
}

/** The mic inside an answer field: the same local dictation as the editors, only her voice. */
function AnswerMic({ reportId, onText }: { reportId: string; onText: (text: string) => void }) {
  const { t } = useTranslation()
  const id = useId()
  const d = useDictation()
  const { startId } = useCaptureDevice()
  const phase = targetPhase(d, id)
  const on = d.targetId === id
  const insert = useRef(onText)
  insert.current = onText
  useEffect(() => registerTarget(id, (text) => insert.current(text)), [id])
  const label = on ? t('dictation.stop') : t('dictation.start')
  return (
    <button
      type="button"
      title={`${label} · ${t(`dictation.phase.${phase}`)}`}
      aria-label={label}
      aria-pressed={on}
      data-phase={phase}
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => void (on ? stopDictation() : startDictation(id, reportId, startId))}
      className={cn(
        'absolute right-1.5 bottom-1.5 flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground',
        on && 'bg-destructive/10 text-destructive hover:bg-destructive/15 hover:text-destructive',
      )}
      data-testid="question-mic"
    >
      {phase === 'starting' || phase === 'loading' ? (
        <Loader2 className="size-4 animate-spin" />
      ) : on ? (
        <MicOff className="size-4" />
      ) : (
        <Mic className="size-4" />
      )}
    </button>
  )
}

/** Questions the last check found answered by session facts: shown with the facts, answerable anyway. */
function CoveredList({ reportId, items, readOnly }: { reportId: string; items: QuestionView[]; readOnly: boolean }) {
  const { t } = useTranslation()
  const [own, setOwn] = useState<ReadonlySet<string>>(new Set())
  return (
    <details className="group rounded-lg border bg-card" data-testid="questions-covered">
      <summary className="flex cursor-pointer items-center gap-2 px-4 py-3 text-sm">
        <ChevronDown className="size-4 text-muted-foreground transition-transform group-open:rotate-180" />
        {t('lotse.questions.covered', { count: items.length })}
      </summary>
      <ul className="space-y-3 border-t px-4 py-3">
        {items.map((x) => {
          const id = questionId(x)
          return (
            <li key={id} className="space-y-1.5">
              <p className="text-sm">
                <span className="text-muted-foreground tabular-nums">{x.number}</span> {x.question}
              </p>
              <ul className="space-y-0.5 pl-4 text-[13px] text-muted-foreground">
                {x.covered_by.map((f) => (
                  <li key={f.fact_id}>
                    <span className="tabular-nums">{t('draft.session', { n: f.session_no })}:</span> {f.text}
                  </li>
                ))}
              </ul>
              {own.has(id) ? (
                <QuestionCard reportId={reportId} question={x} readOnly={readOnly} />
              ) : (
                !readOnly && (
                  <Button variant="ghost" size="sm" className="h-7 px-2 text-xs" onClick={() => setOwn(new Set([...own, id]))}>
                    {t('lotse.questions.answerAnyway')}
                  </Button>
                )
              )}
            </li>
          )
        })}
      </ul>
    </details>
  )
}

/** Questions she still has to answer: not answered, not covered by the sessions, in a section without text. */
export function openQuestions(data: ReportQuestions | undefined, filled: ReadonlySet<string>): QuestionView[] {
  return data?.questions.filter((x) => isOpen(x) && !filled.has(x.section_key)) ?? []
}
