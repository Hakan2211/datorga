// Bericht drafting per section (Phase 2): the applied facts of the episode's Sitzungsnotizen become
// a section draft. Each sentence shows the facts it cites; nothing reaches the Bericht until the
// therapist applies a draft (replace or append).
import { useEffect, useState } from 'react'
import { Link } from '@tanstack/react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Sparkles, X } from 'lucide-react'
import type { ApplyMode } from '@/bindings/ApplyMode'
import type { EpisodeFact } from '@/bindings/EpisodeFact'
import type { SectionDef } from '@/bindings/SectionDef'
import type { SectionDraft } from '@/bindings/SectionDraft'
import { formatDate } from '@/components/bits'
import { RedactionDialog } from '@/components/fact-draft'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { api, errorMessage, q } from '@/lib/api'

/** Overview above the sections: what drafts would draw on, and "draft all empty sections". */
export function ReportDraftCard({
  reportId,
  sections,
  isEmpty,
  onDraft,
}: {
  reportId: string
  sections: SectionDef[]
  isEmpty: (key: string) => boolean
  onDraft: (keys: string[]) => void
}) {
  const { t } = useTranslation()
  const llm = useQuery(q.llm())
  const sources = useQuery(q.draftSources(reportId))
  const facts = sources.data ?? []
  const sessions = new Set(facts.map((f) => f.session_id)).size
  const empty = sections.filter((s) => isEmpty(s.key)).map((s) => s.key)
  const active = llm.data?.config.active

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-3">
        <CardTitle className="flex items-center gap-2 text-base">
          <Sparkles className="size-4 text-primary" /> {t('draft.title')}
        </CardTitle>
        <Button size="sm" onClick={() => onDraft(empty)} disabled={!active || facts.length === 0 || empty.length === 0}>
          {t('draft.draftEmpty')}
        </Button>
      </CardHeader>
      <CardContent className="space-y-2 text-sm text-muted-foreground">
        {!active ? (
          <p>
            {t('facts.noProvider')}{' '}
            <Link to="/settings" className="underline">
              {t('facts.settingsLink')}
            </Link>
          </p>
        ) : facts.length === 0 ? (
          <p>{t('draft.noSources')}</p>
        ) : (
          <>
            <p>{t('draft.intro')}</p>
            <p className="text-foreground">{t('draft.sources', { count: facts.length, sessions })}</p>
            {empty.length === 0 && <p>{t('draft.allFilled')}</p>}
          </>
        )}
      </CardContent>
    </Card>
  )
}

/** Redaction preview + optional instruction for drafting `keys`, then the run. */
export function DraftDialog({
  reportId,
  patientId,
  keys,
  sections,
  onClose,
}: {
  reportId: string
  patientId: string
  keys: string[]
  sections: SectionDef[]
  onClose: () => void
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [instruction, setInstruction] = useState('')
  // The preview follows the instruction once typing pauses.
  const [sent, setSent] = useState('')
  useEffect(() => {
    const id = setTimeout(() => setSent(instruction.trim()), 400)
    return () => clearTimeout(id)
  }, [instruction])
  const settled = sent === instruction.trim()

  return (
    <RedactionDialog
      reportId={reportId}
      patientId={patientId}
      previewKey={['draftPreview', reportId, keys, sent]}
      loadPreview={() => api.draftPreview(reportId, keys, sent || null)}
      progressEvent="draft-progress"
      canRun={settled}
      labels={{
        send: t('draft.send'),
        runLocal: t('draft.runLocal'),
        running: t('draft.progress.running'),
        part: (done, total) => t('draft.progress.part', { done, total }),
        parts: (count) => t('draft.parts', { count }),
      }}
      onRun={async (digest, requestId) => {
        const r = await api.draftRun(reportId, keys, sent || null, digest, requestId)
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
          <span className="text-muted-foreground">{t('draft.sectionsLabel')}:</span>{' '}
          {keys.map((k) => sections.find((s) => s.key === k)?.heading ?? k).join(' · ')}
        </p>
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
        <span className="font-medium">{t('draft.proposal')}</span>
        <span className="font-mono text-xs text-muted-foreground">{draft.model}</span>
      </div>
      {draft.sentences.length === 0 ? (
        <p className="text-muted-foreground">{t('draft.empty')}</p>
      ) : (
        <>
          <p className="text-[13px] text-muted-foreground">{t('draft.proposalHint')}</p>
          <Sentences draft={draft} noteSections={noteSections} />
        </>
      )}
      <div className="flex flex-wrap items-center gap-2 pt-1">
        {draft.sentences.length > 0 &&
          (sectionEmpty ? (
            <Button size="sm" disabled={busy} onClick={act(() => onApply('replace'))}>
              {t('draft.use')}
            </Button>
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

function Sentences({ draft, noteSections }: { draft: SectionDraft; noteSections: SectionDef[] }) {
  return (
    <ol className="space-y-1">
      {draft.sentences.map((s) => (
        <li key={s.id} className="flex flex-wrap items-baseline gap-x-1.5">
          <span className="text-foreground">{s.text}</span>
          {s.sources.map((f) => (
            <FactChip key={f.fact_id} fact={f} noteSections={noteSections} />
          ))}
        </li>
      ))}
    </ol>
  )
}

function FactChip({ fact, noteSections }: { fact: EpisodeFact; noteSections: SectionDef[] }) {
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
