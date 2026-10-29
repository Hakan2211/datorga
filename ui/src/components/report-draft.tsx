// Bericht drafting per section (Phase 2): the applied facts of the episode's Sitzungsnotizen become
// a section draft. Each sentence shows the facts it cites; nothing reaches the Bericht until the
// therapist applies a draft (replace or append).
import { useEffect, useMemo, useState } from 'react'
import { Link } from '@tanstack/react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Sparkles, X } from 'lucide-react'
import type { ApplyMode } from '@/bindings/ApplyMode'
import type { EpisodeFact } from '@/bindings/EpisodeFact'
import type { PriorSource } from '@/bindings/PriorSource'
import type { ReportAnswer } from '@/bindings/ReportAnswer'
import type { SectionDef } from '@/bindings/SectionDef'
import type { SectionDraft } from '@/bindings/SectionDraft'
import { formatDate } from '@/components/bits'
import { RedactionDialog } from '@/components/fact-draft'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { api, errorMessage, q, type DraftChoice } from '@/lib/api'

export type SessionRow = { id: string; no: number; at: string | null; facts: number }

/** The sessions with applied facts, in order, from the episode's facts. */
function sessionRows(facts: EpisodeFact[]): SessionRow[] {
  const rows: SessionRow[] = []
  for (const f of facts) {
    const last = rows[rows.length - 1]
    if (last?.id === f.session_id) last.facts++
    else rows.push({ id: f.session_id, no: f.session_no, at: f.session_at, facts: 1 })
  }
  return rows
}

/** The sessions a draft could draw on, and the default choice: those since the previous application. */
export function useDraftSessions(reportId: string) {
  const sources = useQuery(q.draftSources(reportId))
  return useMemo(() => {
    const rows = sessionRows(sources.data?.facts ?? [])
    const since = sources.data?.since ?? null
    const recent = rows.filter((r) => !since || !r.at || r.at > since).map((r) => r.id)
    return { ready: !!sources.data, rows, since, defaults: recent.length ? recent : rows.map((r) => r.id) }
  }, [sources.data])
}

/**
 * What the focus mode's "Optionen" and "Neu entwerfen" chose; the dialog then only previews.
 * `sessionIds` undefined: the default sessions, `null`: all.
 */
export type DraftPreset = { sessionIds?: string[] | null; withExisting?: boolean; instruction?: string; withoutPrior?: boolean }

/**
 * Redaction preview + the choices for drafting `keys` (sessions, existing text, instruction),
 * then the run. `filled`: the keys whose section already holds text. With a `preset` the choices
 * were made before and the dialog shows them in one line.
 */
export function DraftDialog({
  reportId,
  patientId,
  keys,
  sections,
  filled,
  preset,
  onClose,
}: {
  reportId: string
  patientId: string
  keys: string[]
  sections: SectionDef[]
  filled: string[]
  preset?: DraftPreset
  onClose: () => void
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const sources = useQuery(q.draftSources(reportId))
  const sessions = useDraftSessions(reportId)
  const rows = sessions.rows
  // Default: the sessions since the previous application (all for the first one).
  const [chosen, setChosen] = useState<string[] | null>(null)
  useEffect(() => {
    if (chosen !== null || !sessions.ready) return
    const given = preset?.sessionIds
    setChosen(given === undefined ? sessions.defaults : given === null ? rows.map((r) => r.id) : given)
  }, [chosen, rows, sessions, preset])
  const [withExisting, setWithExisting] = useState(preset?.withExisting ?? filled.length > 0)
  const [allFacts, setAllFacts] = useState(false)
  const [instruction, setInstruction] = useState(preset?.instruction ?? '')
  // The preview follows the instruction once typing pauses.
  const [sent, setSent] = useState('')
  useEffect(() => {
    const id = setTimeout(() => setSent(instruction.trim()), 400)
    return () => clearTimeout(id)
  }, [instruction])
  const settled = sent === instruction.trim()
  const all = chosen !== null && chosen.length === rows.length
  const choice: DraftChoice = {
    sections: keys,
    instruction: sent || null,
    sessionIds: all ? null : chosen,
    withExisting: withExisting && filled.length > 0,
    allFacts,
    withoutPrior: !!preset?.withoutPrior,
  }
  const previewKey = ['draftPreview', reportId, choice]
  // Shares the dialog's preview query (same key): sections skipped for lack of facts.
  const preview = useQuery({
    queryKey: previewKey,
    queryFn: () => api.draftPreview(reportId, choice),
    enabled: chosen !== null && chosen.length > 0,
    gcTime: 0,
  })
  const heading = (k: string) => sections.find((s) => s.key === k)?.heading ?? k

  return (
    <RedactionDialog
      reportId={reportId}
      patientId={patientId}
      previewKey={previewKey}
      loadPreview={() => api.draftPreview(reportId, choice)}
      progressEvent="draft-progress"
      canRun={settled && chosen !== null && chosen.length > 0}
      labels={{
        send: t('draft.send'),
        runLocal: t('draft.runLocal'),
        running: t('draft.progress.running'),
        part: (done, total) => t('draft.progress.part', { done, total }),
        parts: (count) => t('draft.parts', { count }),
      }}
      onRun={async (digest, requestId) => {
        const r = await api.draftRun(reportId, choice, digest, requestId)
        qc.setQueryData(q.reportDrafts(reportId).queryKey, r.drafts)
        if (r.cancelled) {
          if (r.done === 0) toast.info(t('facts.cancelledNothing'))
          else toast.success(t('draft.cancelledPartial', { done: r.done, total: r.total }))
          return
        }
        toast.success(
          t('draft.done', { count: keys.length }) + (r.dropped ? ` ${t('draft.dropped', { count: r.dropped })}` : ''),
        )
      }}
      onClose={onClose}
    >
      <section className="space-y-2">
        <p>
          <span className="text-muted-foreground">{t('draft.sectionsLabel')}:</span> {keys.map(heading).join(' · ')}
        </p>
        {preview.data && preview.data.skipped.length > 0 && (
          <p className="text-[13px] text-muted-foreground" data-testid="draft-skipped">
            {t('draft.skipped', { sections: preview.data.skipped.map(heading).join(' · ') })}
          </p>
        )}
        {preview.data && (preview.data.skipped.length > 0 || allFacts) && (
          <label className="flex items-center gap-2 text-[13px]">
            <Checkbox checked={allFacts} onCheckedChange={(v) => setAllFacts(v === true)} />
            {t('draft.allFacts')}
          </label>
        )}
        {preview.data && preview.data.untagged > 0 && (
          <p className="text-[13px] text-muted-foreground">{t('draft.untagged', { count: preview.data.untagged })}</p>
        )}
        {preset && chosen !== null && (
          <p className="text-[13px] text-muted-foreground" data-testid="draft-preset">
            {all ? t('draft.presetAll', { count: rows.length }) : t('draft.presetSome', { count: chosen.length, total: rows.length })}
            {withExisting && filled.length > 0 && ` · ${t('draft.presetExisting')}`}
            {sources.data?.prior && preset.withoutPrior && ` · ${t('draft.presetWithoutPrior')}`}
            {sent && ` · ${t('draft.presetInstruction', { text: sent })}`}
          </p>
        )}
        {!preset && rows.length > 1 && chosen !== null && (
          <div className="space-y-1.5" data-testid="draft-sessions">
            <p className="text-xs font-medium">{t('draft.sessions')}</p>
            <ul className="flex flex-wrap gap-x-4 gap-y-1">
              {rows.map((r) => (
                <li key={r.id}>
                  <label className="flex items-center gap-1.5 text-[13px]">
                    <Checkbox
                      checked={chosen.includes(r.id)}
                      onCheckedChange={(v) =>
                        setChosen((c) => (v === true ? [...(c ?? []), r.id] : (c ?? []).filter((id) => id !== r.id)))
                      }
                    />
                    {t('draft.session', { n: r.no })}
                    <span className="text-muted-foreground">
                      {formatDate(r.at)} · {t('draft.factCount', { count: r.facts })}
                    </span>
                  </label>
                </li>
              ))}
            </ul>
            <p className="text-[13px] text-muted-foreground">
              {chosen.length === 0
                ? t('draft.noSession')
                : sources.data?.since
                  ? t('draft.sessionsSince', { date: formatDate(sources.data.since) })
                  : t('draft.sessionsHint')}
            </p>
          </div>
        )}
        {!preset && filled.length > 0 && (
          <div className="space-y-1">
            <label className="flex items-center gap-2 text-[13px]">
              <Checkbox checked={withExisting} onCheckedChange={(v) => setWithExisting(v === true)} />
              {t('draft.withExisting')}
            </label>
            <p className="pl-6 text-[13px] text-muted-foreground">{t('draft.withExistingHint')}</p>
          </div>
        )}
        {!preset && (
          <>
            <Label htmlFor="draft-instruction" className="text-xs">
              {t('draft.instruction')}
            </Label>
            <Input
              id="draft-instruction"
              value={instruction}
              maxLength={500}
              onChange={(e) => setInstruction(e.target.value)}
              placeholder={t('draft.instructionPlaceholder')}
            />
            <p className="text-[13px] text-muted-foreground">{t('draft.instructionHint')}</p>
          </>
        )}
      </section>
    </RedactionDialog>
  )
}

/** A proposed draft under its section heading: sentences with their facts, apply or reject. */
export function DraftProposal({
  draft,
  sectionEmpty,
  noteSections,
  onApply,
  onReject,
}: {
  draft: SectionDraft
  sectionEmpty: boolean
  noteSections: SectionDef[]
  onApply: (mode: ApplyMode) => Promise<void>
  onReject: () => Promise<void>
}) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)
  const act = (f: () => Promise<void>) => async () => {
    setBusy(true)
    try {
      await f()
    } catch (e) {
      toast.error(errorMessage(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="space-y-2 rounded-md border border-primary/40 bg-primary/5 p-3 text-sm" data-testid="draft-proposal">
      <div className="flex items-center gap-2">
        <Sparkles className="size-4 text-primary" />
        <span className="font-medium">{draft.with_existing ? t('draft.proposalAddition') : t('draft.proposal')}</span>
        {draft.source === 'dictation' && <span className="text-xs text-muted-foreground">{t('draft.fromDictation')}</span>}
        <span className="font-mono text-xs text-muted-foreground">{draft.model}</span>
      </div>
      {draft.sentences.length === 0 ? (
        <p className="text-muted-foreground">{t('draft.empty')}</p>
      ) : (
        <>
          <p className="text-[13px] text-muted-foreground">
            {t(draft.source === 'dictation' ? 'draft.proposalHintDictation' : 'draft.proposalHint')}
          </p>
          <Sentences draft={draft} noteSections={noteSections} />
        </>
      )}
      <div className="flex flex-wrap items-center gap-2 pt-1">
        {draft.sentences.length > 0 &&
          (sectionEmpty ? (
            <Button size="sm" disabled={busy} onClick={act(() => onApply('replace'))}>
              {t('draft.use')}
            </Button>
          ) : draft.with_existing ? (
            // Written as an addition to the current text: appending is the natural choice.
            <>
              <Button size="sm" disabled={busy} onClick={act(() => onApply('append'))}>
                {t('draft.append')}
              </Button>
              <Button size="sm" variant="outline" disabled={busy} onClick={act(() => onApply('replace'))}>
                {t('draft.replace')}
              </Button>
            </>
          ) : (
            <>
              <Button size="sm" disabled={busy} onClick={act(() => onApply('replace'))}>
                {t('draft.replace')}
              </Button>
              <Button size="sm" variant="outline" disabled={busy} onClick={act(() => onApply('append'))}>
                {t('draft.append')}
              </Button>
            </>
          ))}
        <Button size="sm" variant="ghost" disabled={busy} onClick={act(onReject)}>
          <X /> {draft.sentences.length ? t('draft.reject') : t('draft.dismiss')}
        </Button>
      </div>
    </div>
  )
}

/** Where the applied AI text of a section came from. */
export function DraftSources({ draft, noteSections }: { draft: SectionDraft; noteSections: SectionDef[] }) {
  const { t } = useTranslation()
  return (
    <details className="text-[13px] text-muted-foreground">
      <summary className="cursor-pointer text-xs">
        {t('draft.appliedSources', { model: draft.model, date: formatDate(draft.applied_at, true) })}
      </summary>
      <div className="mt-1.5">
        <Sentences draft={draft} noteSections={noteSections} />
      </div>
    </details>
  )
}

export function Sentences({ draft, noteSections }: { draft: SectionDraft; noteSections: SectionDef[] }) {
  return (
    <ol className="space-y-1">
      {draft.sentences.map((s) => (
        <li key={s.id} className="flex flex-wrap items-baseline gap-x-1.5">
          <span className="text-foreground">{s.text}</span>
          {s.sources.map((f) => (
            <FactChip key={f.fact_id} fact={f} noteSections={noteSections} />
          ))}
          {s.answers.map((a) => (
            <AnswerChip key={a.id} answer={a} />
          ))}
          {s.prior.map((p) => (
            <PriorChip key={`${p.report_id}/${p.section_key}`} prior={p} />
          ))}
        </li>
      ))}
    </ol>
  )
}

/** A sentence resting on her answer to a Lotse question. */
export function AnswerChip({ answer }: { answer: ReportAnswer }) {
  const { t } = useTranslation()
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="rounded bg-primary/10 px-1.5 py-0.5 text-[11px] whitespace-nowrap text-primary" data-testid="answer-chip">
          {t('lotse.questions.own')}
        </span>
      </TooltipTrigger>
      <TooltipContent className="max-w-sm">
        <p className="text-xs opacity-80">{answer.question}</p>
        <p>{answer.text}</p>
      </TooltipContent>
    </Tooltip>
  )
}

/** A sentence resting on a section of the Vorbericht. */
export function PriorChip({ prior }: { prior: PriorSource }) {
  const { t } = useTranslation()
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Link
          to="/reports/$reportId"
          params={{ reportId: prior.report_id }}
          search={{ doc: true }}
          className="rounded bg-sand px-1.5 py-0.5 text-[11px] whitespace-nowrap text-sand-foreground hover:bg-primary/15 hover:text-foreground"
          data-testid="prior-chip"
        >
          {t('draft.prior', { date: formatDate(prior.signed_at) })}
        </Link>
      </TooltipTrigger>
      <TooltipContent className="max-w-sm">
        <p>{prior.excerpt}</p>
      </TooltipContent>
    </Tooltip>
  )
}

export function FactChip({ fact, noteSections }: { fact: EpisodeFact; noteSections: SectionDef[] }) {
  const { t } = useTranslation()
  const heading = noteSections.find((s) => s.key === fact.section_key)?.heading ?? fact.section_key
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Link
          to="/reports/$reportId"
          params={{ reportId: fact.note_id }}
          className="rounded bg-muted px-1.5 py-0.5 text-[11px] whitespace-nowrap text-muted-foreground tabular-nums hover:bg-primary/15 hover:text-foreground"
          aria-label={t('draft.openNote')}
        >
          {t('draft.session', { n: fact.session_no })}
        </Link>
      </TooltipTrigger>
      <TooltipContent className="max-w-sm">
        <p className="text-xs opacity-80">
          {formatDate(fact.session_at)} · {heading}
        </p>
        <p>{fact.text}</p>
      </TooltipContent>
    </Tooltip>
  )
}
