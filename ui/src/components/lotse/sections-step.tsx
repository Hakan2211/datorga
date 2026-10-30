import { useEffect, useMemo, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Check, Minus, Plus, RefreshCw, Settings2, Sparkles, X } from 'lucide-react'
import type { ApplyMode } from '@/bindings/ApplyMode'
import type { EpisodeFact } from '@/bindings/EpisodeFact'
import type { SectionDef } from '@/bindings/SectionDef'
import type { SectionDraft } from '@/bindings/SectionDraft'
import type { SentenceChoice } from '@/bindings/SentenceChoice'
import { formatDate } from '@/components/bits'
import { QuestionCard, shortHeading } from '@/components/lotse/questions-step'
import { AnswerChip, FactChip, PriorChip, useDraftSessions, type DraftPreset } from '@/components/report-draft'
import { SectionEditor } from '@/components/section-editor'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Kbd } from '@/components/ui/kbd'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage, q, type ReportView } from '@/lib/api'
import { TAG_TEMPLATE_KEY, isOpen, nextOpenSection, questionId, sectionFacts, type SectionState } from '@/lib/lotse'
import { isBlank } from '@/lib/report'
import { cn } from '@/lib/utils'

/**
 * Lotse step 3: one section at a time. Its proposal is taken over sentence by sentence (keyboard
 * first), the text below stays freely editable, and the right rail shows where a sentence comes
 * from, what is still missing and which facts belong to the section.
 */
export function SectionsStep({
  view,
  bodies,
  states,
  drafts,
  current,
  onSelect,
  readOnly,
  canDraft,
  noteSections,
  options,
  onOptions,
  onChange,
  onRelevance,
  onDecide,
  onApply,
  onReject,
  onDraft,
}: {
  view: ReportView
  bodies: Record<string, string>
  states: Record<string, SectionState>
  drafts: SectionDraft[]
  current: string
  onSelect: (key: string) => void
  readOnly: boolean
  /** An AI model is set and the Bericht can take drafts. */
  canDraft: boolean
  noteSections: SectionDef[]
  options: DraftPreset
  onOptions: (o: DraftPreset) => void
  onChange: (key: string, body: string) => void
  onRelevance: (key: string, notRelevant: boolean) => void
  onDecide: (draft: SectionDraft, choices: SentenceChoice[]) => Promise<void>
  onApply: (draft: SectionDraft, mode: ApplyMode) => Promise<void>
  onReject: (draft: SectionDraft) => Promise<void>
  onDraft: (keys: string[], preset: DraftPreset) => void
}) {
  const { t } = useTranslation()
  const { report, template } = view
  const sections = template.sections
  const def = sections.find((s) => s.key === current) ?? sections[0]
  const proposal = drafts.find((d) => d.section_key === def.key && d.status === 'proposed')
  const sources = useQuery(q.draftSources(report.id))
  const questions = useQuery(q.reportQuestions(report.id))
  const facts = useMemo(
    () => (sources.data ? sectionFacts(template.key, def, sources.data.facts, sources.data.since) : []),
    [sources.data, template.key, def],
  )
  const asks = questions.data?.questions.filter((x) => x.section_key === def.key && isOpen(x)) ?? []
  const answers = questions.data?.questions.filter((x) => x.section_key === def.key && x.answer).length ?? 0
  const notRelevant = states[def.key] === 'skipped'
  const draftable = canDraft && !readOnly && !notRelevant && def.field === 'prose' && def.ai_allowed
  const empty = isBlank(bodies[def.key] ?? '')

  // The sentence under review: the first undecided one unless she moved on.
  const undecided = proposal?.sentences.filter((s) => !s.decision) ?? []
  const [cursorId, setCursor] = useState<string | null>(null)
  const cursor = undecided.find((s) => s.id === cursorId) ?? undecided[0] ?? null
  const [editing, setEditing] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const decide = async (choices: SentenceChoice[]) => {
    if (!proposal || busy) return
    setBusy(true)
    try {
      // The cursor moves to the next undecided sentence after the one decided.
      const at = undecided.findIndex((s) => s.id === choices[0]?.sentence_id)
      const next = undecided.slice(at + 1).find((s) => !choices.some((c) => c.sentence_id === s.id))
      await onDecide(proposal, choices)
      setCursor(next?.id ?? null)
      setEditing(null)
    } catch (e) {
      toast.error(errorMessage(e))
    } finally {
      setBusy(false)
    }
  }
  const accept = (text?: string) => cursor && decide([{ sentence_id: cursor.id, accept: true, ...(text !== undefined ? { text } : {}) }])
  const reject = () => cursor && decide([{ sentence_id: cursor.id, accept: false }])
  const move = (by: number) => {
    if (!cursor) return
    const i = undecided.indexOf(cursor) + by
    if (i >= 0 && i < undecided.length) setCursor(undecided[i].id)
  }
  // N: the next section that still needs her (wrapping), else simply the next one.
  const following =
    nextOpenSection(sections.map((s) => s.key), states, def.key) ?? sections[sections.indexOf(def) + 1]?.key ?? null
  const nextSection = () => following && onSelect(following)

  // Keyboard: only while no field, menu or dialog has the focus.
  const keys = useRef({ accept, reject, move, nextSection, edit: () => cursor && setEditing(cursor.id) })
  keys.current = { accept, reject, move, nextSection, edit: () => cursor && setEditing(cursor.id) }
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey || e.defaultPrevented) return
      const el = e.target as HTMLElement | null
      if (el?.closest('input, textarea, select, [contenteditable="true"], [role="dialog"], [role="menu"], button, a')) return
      if (document.querySelector('[role="dialog"]')) return
      const k = keys.current
      const handled: Record<string, () => void> = {
        Enter: () => void k.accept(),
        Delete: () => void k.reject(),
        e: k.edit,
        E: k.edit,
        ArrowDown: () => k.move(1),
        ArrowUp: () => k.move(-1),
        n: k.nextSection,
        N: k.nextSection,
      }
      const run = handled[e.key]
      if (!run) return
      e.preventDefault()
      run()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const rejected = proposal?.sentences.filter((s) => s.decision === 'rejected').length ?? 0
  const decided = proposal?.sentences.some((s) => s.decision) ?? false

  return (
    <div className="grid grid-cols-[13rem_minmax(0,1fr)_19rem] gap-6" data-testid="lotse-sections">
      <SectionRail sections={sections} states={states} current={def.key} onSelect={onSelect} />

      <div className="min-w-0 space-y-4" data-testid="focus-section" data-section={def.key}>
        <SectionEditor
          key={def.key}
          def={def}
          body={bodies[def.key] ?? ''}
          readOnly={readOnly}
          onChange={(b) => onChange(def.key, b)}
          dictateFor={report.id}
          notRelevant={notRelevant}
          onRelevance={def.optional && !readOnly ? (off) => onRelevance(def.key, off) : undefined}
          action={
            draftable && (
              <>
                <RedraftButton hasProposal={!!proposal} onDraft={(instruction) => onDraft([def.key], { ...options, instruction })} />
                <OptionsButton reportId={report.id} options={options} onOptions={onOptions} sectionFilled={!empty} />
              </>
            )
          }
          above={
            proposal && !readOnly ? (
              <div className="space-y-3 rounded-lg border border-primary/30 bg-card p-4" data-testid="focus-proposal">
                <div className="flex items-center gap-2 text-sm">
                  <Sparkles className="size-4 text-primary" />
                  <span className="font-medium">{proposal.with_existing ? t('draft.proposalAddition') : t('draft.proposal')}</span>
                  <span className="font-mono text-xs text-muted-foreground">{proposal.model}</span>
                </div>
                {proposal.sentences.length === 0 ? (
                  <p className="text-sm text-muted-foreground">{t('draft.empty')}</p>
                ) : (
                  <ol className="space-y-2 font-serif text-[16px] leading-relaxed">
                    {proposal.sentences.map((s) =>
                      s.decision === 'rejected' ? null : s.decision === 'accepted' ? (
                        <li key={s.id} className="flex items-start gap-2 text-foreground" data-testid="sentence" data-state="accepted">
                          <Check className="mt-1.5 size-3.5 shrink-0 text-primary" />
                          <span>{s.text}{s.edited && <span className="ml-1.5 font-sans text-[11px] text-muted-foreground">{t('lotse.focus.ownWording')}</span>}</span>
                        </li>
                      ) : s.id === cursor?.id ? (
                        <li key={s.id} className="space-y-2 rounded-md border border-primary/40 bg-primary/5 p-3" data-testid="sentence" data-state="current">
                          {editing === s.id ? (
                            <SentenceEdit initial={s.text} busy={busy} onSave={(text) => void accept(text)} onCancel={() => setEditing(null)} />
                          ) : (
                            <p>{s.text}</p>
                          )}
                          <div className="flex flex-wrap items-center gap-1.5 font-sans">
                            {s.sources.map((f) => (
                              <FactChip key={f.fact_id} fact={f} noteSections={noteSections} />
                            ))}
                            {s.answers.map((a) => (
                              <AnswerChip key={a.id} answer={a} />
                            ))}
                            {s.prior.map((p) => (
                              <PriorChip key={`${p.report_id}/${p.section_key}`} prior={p} />
                            ))}
                            {editing !== s.id && (
                              <span className="ml-auto flex flex-wrap justify-end gap-1.5">
                                <Button size="sm" variant="ghost" disabled={busy} onClick={() => setEditing(s.id)} data-testid="sentence-edit">
                                  {t('lotse.focus.edit')} <Kbd>E</Kbd>
                                </Button>
                                <Button size="sm" variant="outline" disabled={busy} onClick={() => void reject()} data-testid="sentence-reject">
                                  {t('lotse.focus.reject')} <Kbd>Entf</Kbd>
                                </Button>
                                <Button size="sm" disabled={busy} onClick={() => void accept()} data-testid="sentence-accept">
                                  {t('lotse.focus.accept')} <Kbd className="bg-primary-foreground/20 text-primary-foreground">↵</Kbd>
                                </Button>
                              </span>
                            )}
                          </div>
                        </li>
                      ) : (
                        <li
                          key={s.id}
                          className="cursor-pointer rounded-md px-3 text-muted-foreground/80 hover:text-muted-foreground"
                          onClick={() => setCursor(s.id)}
                          data-testid="sentence"
                          data-state="open"
                        >
                          {s.text}
                        </li>
                      ),
                    )}
                  </ol>
                )}
                <div className="flex flex-wrap items-center gap-2 border-t pt-3 text-xs text-muted-foreground">
                  {rejected > 0 && <span>{t('lotse.focus.rejectedCount', { count: rejected })}</span>}
                  <span className="ml-auto flex flex-wrap justify-end gap-1.5">
                    {undecided.length > 0 && (
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={busy}
                        onClick={() => void decide(undecided.map((s) => ({ sentence_id: s.id, accept: false })))}
                        data-testid="sentence-reject-rest"
                      >
                        <X /> {decided ? t('lotse.focus.rejectRest') : t('draft.reject')}
                      </Button>
                    )}
                    {proposal.sentences.length === 0 && (
                      <Button size="sm" variant="ghost" disabled={busy} onClick={() => void onReject(proposal)}>
                        <X /> {t('draft.dismiss')}
                      </Button>
                    )}
                    {!decided && !empty && !proposal.with_existing && undecided.length > 0 && (
                      <Button size="sm" variant="outline" disabled={busy} onClick={() => void onApply(proposal, 'replace')} data-testid="sentence-replace">
                        {t('lotse.focus.replaceText')}
                      </Button>
                    )}
                    {undecided.length > 1 && (
                      <Button size="sm" variant="outline" disabled={busy} onClick={() => void onApply(proposal, 'append')} data-testid="sentence-accept-rest">
                        {decided ? t('lotse.focus.acceptRest') : t('lotse.focus.acceptAll')}
                      </Button>
                    )}
                  </span>
                </div>
                {undecided.length > 0 && <KeyHints />}
              </div>
            ) : (
              !readOnly &&
              !notRelevant &&
              empty &&
              def.field === 'prose' && (
                <EmptyHint
                  canDraft={draftable}
                  material={facts.length + answers > 0}
                  onDraft={() => onDraft([def.key], options)}
                />
              )
            )
          }
        />
        {!proposal && !readOnly && states[def.key] === 'done' && following && (
          <Button variant="ghost" size="sm" onClick={nextSection} data-testid="focus-next-section">
            {t('lotse.focus.nextSection')} <Kbd>N</Kbd>
          </Button>
        )}
      </div>

      <aside className="space-y-4">
        <div className="sticky top-4 space-y-4">
          {cursor && <SourcesCard sentence={cursor} />}
          {/* Questions of a written section are optional (slice 38, F9): they show only while it needs text. */}
          {(states[def.key] === 'open' || states[def.key] === 'proposal') && asks.length > 0 && (
            <Card className="gap-0" data-testid="focus-missing">
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">{t('lotse.focus.missing')}</CardTitle>
              </CardHeader>
              <CardContent className="space-y-2">
                {asks.map((x) => (
                  <QuestionCard key={questionId(x)} reportId={report.id} question={x} readOnly={readOnly} />
                ))}
              </CardContent>
            </Card>
          )}
          {def.field === 'prose' && !notRelevant && sources.data && (
            <FactsCard
              view={view}
              def={def}
              facts={facts}
              all={sources.data.facts}
              since={sources.data.since}
              readOnly={readOnly}
            />
          )}
        </div>
      </aside>
    </div>
  )
}

const DOT: Record<SectionState, string> = {
  done: 'border-primary bg-primary',
  proposal: 'border-primary bg-[linear-gradient(90deg,var(--color-primary)_50%,transparent_50%)]',
  open: 'border-muted-foreground/50',
  skipped: '',
}

function StateDot({ state }: { state: SectionState }) {
  if (state === 'skipped') return <Minus className="size-3.5 shrink-0 text-muted-foreground" />
  return <span className={cn('size-3 shrink-0 rounded-full border-2', DOT[state])} />
}

function SectionRail({
  sections,
  states,
  current,
  onSelect,
}: {
  sections: SectionDef[]
  states: Record<string, SectionState>
  current: string
  onSelect: (key: string) => void
}) {
  const { t } = useTranslation()
  return (
    <nav className="sticky top-4 space-y-4 self-start" data-testid="focus-rail">
      <ol className="space-y-0.5">
        {sections.map((s) => (
          <li key={s.key}>
            <button
              type="button"
              onClick={() => onSelect(s.key)}
              aria-current={s.key === current ? 'true' : undefined}
              data-section={s.key}
              data-state={states[s.key]}
              className={cn(
                'flex w-full items-center gap-2 rounded-md border-l-2 border-transparent px-2.5 py-2 text-left text-[13px] hover:bg-muted/60',
                s.key === current && 'border-primary bg-primary/5 font-medium',
                states[s.key] === 'skipped' && 'text-muted-foreground',
              )}
            >
              <span className="w-4 shrink-0 text-muted-foreground tabular-nums">{s.number}</span>
              <span className="min-w-0 flex-1 leading-snug">{shortHeading(s.heading)}</span>
              <StateDot state={states[s.key]} />
            </button>
          </li>
        ))}
      </ol>
      <ul className="space-y-1.5 px-2.5 text-xs text-muted-foreground">
        {(['done', 'proposal', 'open', 'skipped'] as const).map((st) => (
          <li key={st} className="flex items-center gap-2">
            <StateDot state={st} /> {t(`lotse.focus.state.${st}`)}
          </li>
        ))}
      </ul>
    </nav>
  )
}

function KeyHints() {
  const { t } = useTranslation()
  const hints: [string, string][] = [
    ['↵', t('lotse.focus.keys.accept')],
    ['Entf', t('lotse.focus.keys.reject')],
    ['E', t('lotse.focus.keys.edit')],
    ['↓ ↑', t('lotse.focus.keys.move')],
    ['N', t('lotse.focus.keys.next')],
  ]
  return (
    <p className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground" data-testid="focus-keys">
      {hints.map(([k, label]) => (
        <span key={k} className="flex items-center gap-1">
          <Kbd>{k}</Kbd> {label}
        </span>
      ))}
    </p>
  )
}

/** The sentence in her own wording: Enter takes it over, Escape goes back. */
function SentenceEdit({ initial, busy, onSave, onCancel }: { initial: string; busy: boolean; onSave: (text: string) => void; onCancel: () => void }) {
  const { t } = useTranslation()
  const [text, setText] = useState(initial)
  return (
    <div className="space-y-2 font-sans">
      <Textarea
        autoFocus
        value={text}
        rows={3}
        className="font-serif text-[16px]"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault()
            if (text.trim()) onSave(text)
          } else if (e.key === 'Escape') {
            e.preventDefault()
            onCancel()
          }
        }}
        data-testid="sentence-edit-input"
      />
      <div className="flex justify-end gap-1.5">
        <Button size="sm" variant="ghost" onClick={onCancel}>
          {t('common.cancel')}
        </Button>
        <Button size="sm" disabled={busy || !text.trim()} onClick={() => onSave(text)} data-testid="sentence-edit-save">
          {t('lotse.focus.acceptEdited')}
        </Button>
      </div>
    </div>
  )
}

function EmptyHint({ canDraft, material, onDraft }: { canDraft: boolean; material: boolean; onDraft: () => void }) {
  const { t } = useTranslation()
  return (
    <div className="flex items-center justify-between gap-3 rounded-md border border-dashed px-4 py-3 text-sm text-muted-foreground" data-testid="focus-empty">
      <span>{material ? t('lotse.focus.emptyMaterial') : t('lotse.focus.emptyNothing')}</span>
      {canDraft && material && (
        <Button size="sm" variant="outline" onClick={onDraft} data-testid="focus-draft">
          <Sparkles /> {t('lotse.focus.draft')}
        </Button>
      )}
    </div>
  )
}

/** "Neu entwerfen" with an optional instruction; the redaction preview follows. */
function RedraftButton({ hasProposal, onDraft }: { hasProposal: boolean; onDraft: (instruction: string) => void }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [instruction, setInstruction] = useState('')
  const go = () => {
    setOpen(false)
    onDraft(instruction.trim())
  }
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm" className="h-7 text-xs" data-testid="focus-redraft">
          <RefreshCw className="size-3.5" /> {hasProposal ? t('lotse.focus.redraft') : t('lotse.focus.draft')}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-80 space-y-2">
        <label htmlFor="redraft-instruction" className="text-xs font-medium">
          {t('draft.instruction')}
        </label>
        <Input
          id="redraft-instruction"
          value={instruction}
          maxLength={500}
          placeholder={t('draft.instructionPlaceholder')}
          onChange={(e) => setInstruction(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && go()}
        />
        <p className="text-xs text-muted-foreground">{hasProposal ? t('lotse.focus.redraftHint') : t('draft.instructionHint')}</p>
        <div className="flex justify-end">
          <Button size="sm" onClick={go} data-testid="focus-redraft-go">
            <Sparkles /> {t('lotse.focus.draft')}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}

/** Sessions and "ergänzen" for the next drafts (this section's and "Leere Abschnitte entwerfen"). */
function OptionsButton({
  reportId,
  options,
  onOptions,
  sectionFilled,
}: {
  reportId: string
  options: DraftPreset
  onOptions: (o: DraftPreset) => void
  sectionFilled: boolean
}) {
  const { t } = useTranslation()
  const sessions = useDraftSessions(reportId)
  const prior = useQuery(q.draftSources(reportId)).data?.prior
  const chosen = options.sessionIds === undefined ? sessions.defaults : options.sessionIds === null ? sessions.rows.map((r) => r.id) : options.sessionIds
  const set = (ids: string[]) => onOptions({ ...options, sessionIds: ids.length === sessions.rows.length ? null : ids })
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm" className="h-7 text-xs" data-testid="focus-options">
          <Settings2 className="size-3.5" /> {t('lotse.focus.options')}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-80 space-y-3 text-[13px]">
        {sessions.rows.length > 1 && (
          <div className="space-y-1.5">
            <p className="text-xs font-medium">{t('draft.sessions')}</p>
            <ul className="max-h-56 space-y-1 overflow-y-auto">
              {sessions.rows.map((r) => (
                <li key={r.id}>
                  <label className="flex items-center gap-1.5">
                    <Checkbox
                      checked={chosen.includes(r.id)}
                      onCheckedChange={(v) => set(v === true ? [...chosen, r.id] : chosen.filter((id) => id !== r.id))}
                    />
                    {t('draft.session', { n: r.no })}
                    <span className="text-muted-foreground">
                      {formatDate(r.at)} · {t('draft.factCount', { count: r.facts })}
                    </span>
                  </label>
                </li>
              ))}
            </ul>
            <p className="text-muted-foreground">
              {sessions.since ? t('draft.sessionsSince', { date: formatDate(sessions.since) }) : t('draft.sessionsHint')}
            </p>
          </div>
        )}
        <div className="space-y-1">
          <label className="flex items-center gap-2">
            <Checkbox checked={!!options.withExisting} onCheckedChange={(v) => onOptions({ ...options, withExisting: v === true })} />
            {t('draft.withExisting')}
          </label>
          <p className="pl-6 text-muted-foreground">{sectionFilled ? t('draft.withExistingHint') : t('lotse.focus.withExistingEmpty')}</p>
        </div>
        {prior && (
          <div className="space-y-1">
            <label className="flex items-center gap-2">
              <Checkbox
                checked={!options.withoutPrior}
                onCheckedChange={(v) => onOptions({ ...options, withoutPrior: v !== true })}
                data-testid="focus-with-prior"
              />
              {t('draft.withPrior')}
            </label>
            <p className="pl-6 text-muted-foreground">{t('draft.withPriorHint', { date: formatDate(prior.signed_at) })}</p>
          </div>
        )}
      </PopoverContent>
    </Popover>
  )
}

/** "Woher kommt das?": the facts and answers the sentence under review rests on. */
function SourcesCard({ sentence }: { sentence: SectionDraft['sentences'][number] }) {
  const { t } = useTranslation()
  const none = sentence.sources.length === 0 && sentence.answers.length === 0 && sentence.prior.length === 0
  return (
    <Card className="gap-0" data-testid="focus-sources">
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">{t('lotse.focus.sources')}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-2 text-[13px]">
        {none && <p className="text-muted-foreground">{t('lotse.focus.noSources')}</p>}
        {sentence.sources.map((f) => (
          <blockquote key={f.fact_id} className="rounded-md bg-muted/60 px-3 py-2">
            <p>„{f.text}“</p>
            <p className="mt-1 text-xs text-primary">
              {t('draft.session', { n: f.session_no })} · {formatDate(f.session_at)}
            </p>
          </blockquote>
        ))}
        {sentence.answers.map((a) => (
          <blockquote key={a.id} className="rounded-md bg-primary/5 px-3 py-2">
            <p className="text-xs text-muted-foreground">{a.question}</p>
            <p>„{a.text}“</p>
            <p className="mt-1 text-xs text-primary">{t('lotse.questions.own')}</p>
          </blockquote>
        ))}
        {sentence.prior.map((p) => (
          <blockquote key={`${p.report_id}/${p.section_key}`} className="rounded-md bg-sand/60 px-3 py-2">
            <p>„{p.excerpt}“</p>
            <p className="mt-1 text-xs text-sand-foreground">{t('draft.prior', { date: formatDate(p.signed_at) })}</p>
          </blockquote>
        ))}
      </CardContent>
    </Card>
  )
}

/**
 * "Fakten zu diesem Abschnitt" (G): the facts a draft of the section sees, and the other recent
 * ones under "Weitere Fakten". In the Erst-/Umwandlungsbericht she can move a fact in or out.
 */
function FactsCard({
  view,
  def,
  facts,
  all,
  since,
  readOnly,
}: {
  view: ReportView
  def: SectionDef
  facts: EpisodeFact[]
  all: EpisodeFact[]
  since: string | null
  readOnly: boolean
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const sections = view.template.sections
  const taggable = !readOnly && view.template.key === TAG_TEMPLATE_KEY && !def.when.kind?.length && def.ai_allowed
  const [more, setMore] = useState(false)
  const recent = since ? all.filter((f) => !f.session_at || f.session_at >= since) : all
  const others = taggable ? recent.filter((f) => !facts.includes(f)) : []
  const number = (key: string) => sections.find((s) => s.key === key)?.number ?? key

  const retag = (f: EpisodeFact, tags: string[]) =>
    api
      .setFactSections(view.report.id, f.fact_id, tags)
      .then(() => {
        void qc.invalidateQueries({ queryKey: ['draftSources', view.report.id] })
        void qc.invalidateQueries({ queryKey: ['reportQuestions', view.report.id] })
      })
      .catch((e) => toast.error(errorMessage(e)))

  const row = (f: EpisodeFact, inSection: boolean) => (
    <li key={f.fact_id} className="group flex items-start gap-2" data-testid="focus-fact" data-in={inSection}>
      <div className="min-w-0 flex-1">
        <p className={cn(!inSection && 'text-muted-foreground')}>{f.text}</p>
        <p className="mt-0.5 flex flex-wrap items-center gap-1 text-[11px] text-muted-foreground">
          <span className="tabular-nums">{t('draft.session', { n: f.session_no })}</span>
          {f.bericht === null ? (
            <span className="rounded bg-muted px-1">{t('lotse.focus.allSections')}</span>
          ) : (
            f.bericht
              .filter((k) => k !== def.key)
              .map((k) => (
                <span key={k} className="rounded bg-muted px-1 tabular-nums" title={sections.find((s) => s.key === k)?.heading}>
                  {t('lotse.focus.alsoIn', { number: number(k) })}
                </span>
              ))
          )}
        </p>
      </div>
      {taggable &&
        (inSection ? (
          f.bericht !== null && (
            <Button
              variant="ghost"
              size="icon"
              className="size-6 shrink-0 opacity-60 group-hover:opacity-100"
              title={t('lotse.focus.untag')}
              aria-label={t('lotse.focus.untag')}
              onClick={() => void retag(f, f.bericht!.filter((k) => k !== def.key))}
              data-testid="fact-untag"
            >
              <X className="size-3.5" />
            </Button>
          )
        ) : (
          <Button
            variant="ghost"
            size="icon"
            className="size-6 shrink-0"
            title={t('lotse.focus.tag')}
            aria-label={t('lotse.focus.tag')}
            onClick={() => void retag(f, [...(f.bericht ?? []), def.key])}
            data-testid="fact-tag"
          >
            <Plus className="size-3.5" />
          </Button>
        ))}
    </li>
  )

  return (
    <Card className="gap-0" data-testid="focus-facts">
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">{t('lotse.focus.facts', { count: facts.length })}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3 text-[13px]">
        {facts.length === 0 ? (
          <p className="text-muted-foreground">{t('lotse.focus.noFacts')}</p>
        ) : (
          <ul className="max-h-[40vh] space-y-2.5 overflow-y-auto">{facts.map((f) => row(f, true))}</ul>
        )}
        {others.length > 0 &&
          (more ? (
            <div className="space-y-2 border-t pt-3">
              <p className="text-xs font-medium text-muted-foreground">{t('lotse.focus.moreFacts', { count: others.length })}</p>
              <ul className="max-h-[40vh] space-y-2.5 overflow-y-auto">{others.map((f) => row(f, false))}</ul>
            </div>
          ) : (
            <Button variant="ghost" size="sm" className="h-7 px-2 text-xs text-primary" onClick={() => setMore(true)} data-testid="focus-more-facts">
              <Plus className="size-3.5" /> {t('lotse.focus.moreFacts', { count: others.length })}
            </Button>
          ))}
      </CardContent>
    </Card>
  )
}
