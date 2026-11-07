// Quick AI writing in Briefe (slice 47): the rail of the letter page. "Mit KI schreiben" fills the
// empty sections from the ticked material in one pseudonymized request (redaction preview first);
// a pasted request ("Anschreiben") is read for its questions, which become the letter's sections;
// AI sentences without a source stay marked "ohne Beleg" until she keeps or deletes them; and the
// focused section can be re-written with an instruction and shows where its text came from.
import { useEffect, useRef, useState } from 'react'
import { Link } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { ArrowDown, ArrowUp, CalendarClock, Check, Circle, FileQuestion, ListChecks, Plus, RefreshCw, Sparkles, Trash2, X } from 'lucide-react'
import type { Letter } from '@/bindings/Letter'
import type { Material } from '@/bindings/Material'
import type { RequestSection } from '@/bindings/RequestSection'
import type { SectionDef } from '@/bindings/SectionDef'
import { SectionCard, formatDate } from '@/components/bits'
import { DocumentButton } from '@/components/document-button'
import { RedactionDialog } from '@/components/fact-draft'
import { Sentences } from '@/components/report-draft'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage, q, type LetterChoice, type ReportView } from '@/lib/api'
import { appendText } from '@/lib/documents'
import { decideUnbacked, unbackedSentences } from '@/lib/editor'
import { germanDay } from '@/lib/letters'
import { isBlank } from '@/lib/report'

const REQUEST_SAVE_MS = 700

/** Sections the AI may write (prose, AI allowed). */
export function aiSections(sections: SectionDef[]): SectionDef[] {
  return sections.filter((s) => s.field === 'prose' && s.ai_allowed)
}

/** "Mit KI schreiben": material to tick, an optional instruction, then preview and run. */
export function LetterAiCard({
  view,
  letter,
  bodies,
  flush,
  onWritten,
}: {
  view: ReportView
  letter: Letter
  bodies: Record<string, string>
  flush: () => Promise<void>
  onWritten: () => Promise<void>
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const { report, patient, template, episode } = view
  const llm = useQuery(q.llm())
  const sources = useQuery(q.letterSources(report.id))
  const [instruction, setInstruction] = useState('')
  const [open, setOpen] = useState(false)
  const [preparing, setPreparing] = useState(false)
  const material = sources.data?.material ?? letter.material
  const empty = aiSections(template.sections).filter((s) => isBlank(bodies[s.key] ?? ''))
  const choice: LetterChoice = { sections: null, instruction: instruction.trim() || null, rewrite: null }

  const setMaterial = useMutation({
    mutationFn: (m: Material) => api.setLetterMaterial(report.id, m),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['letter', report.id, 'sources'] }),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const tick = (patch: Partial<Material>) => {
    const next = { ...material, ...patch }
    qc.setQueryData(q.letterSources(report.id).queryKey, (s) => (s ? { ...s, material: next } : s))
    setMaterial.mutate(next)
  }

  const s = sources.data
  const rows: { key: keyof Material; label: string; count: number; hint?: string }[] = [
    { key: 'facts', label: t('letterAi.facts'), count: s?.facts ?? 0 },
    { key: 'answers', label: t('letterAi.answers'), count: s?.answers ?? 0 },
    {
      key: 'prior',
      label: t('letterAi.prior'),
      count: s?.prior?.sections ?? 0,
      hint: s?.prior ? t('letterAi.priorFrom', { date: formatDate(s.prior.signed_at) }) : undefined,
    },
  ]
  // Textbausteine are wording, not material: alone they don't make a letter.
  const nothing = rows.every((r) => !material[r.key] || r.count === 0) && !instruction.trim()
  rows.push({ key: 'textblocks', label: t('letterAi.textblocks'), count: s?.textblocks ?? 0, hint: t('letterAi.textblocksHint') })
  // Style exemplars too; offered once she has one for this type (Vorlagen → Ihr Schreibstil).
  if (s?.style || s?.profile)
    rows.push({ key: 'style', label: s.style ? t('letterAi.style') : t('letterAi.styleProfile'), count: s.style || 1, hint: t('letterAi.styleHint') })

  return (
    <SectionCard
      icon={Sparkles}
      title={t('letterAi.title')}
      contentClassName="space-y-3 text-sm"
    >
      {!llm.data?.config.active ? (
        <p className="text-[13px] text-muted-foreground">
          {t('facts.noProvider')}{' '}
          <Link to="/settings/ki" className="underline">
            {t('facts.settingsLink')}
          </Link>
        </p>
      ) : !episode ? (
        <p className="text-[13px] text-muted-foreground">{t('letterAi.noCase')}</p>
      ) : (
        <>
          <div className="space-y-1.5" data-testid="letter-material">
            <p className="text-xs font-medium text-muted-foreground">{t('letterAi.material')}</p>
            {rows.map((r) => (
              <label key={r.key} className="flex items-start gap-2 text-[13px]">
                <Checkbox
                  className="mt-0.5"
                  checked={material[r.key] && r.count > 0}
                  disabled={r.count === 0}
                  onCheckedChange={(v) => tick({ [r.key]: v === true })}
                  data-testid={`letter-material-${r.key}`}
                />
                <span className={r.count === 0 ? 'text-muted-foreground' : ''}>
                  {r.label} <span className="text-muted-foreground">({r.count})</span>
                  {r.hint && <span className="block text-[11px] text-muted-foreground">{r.hint}</span>}
                </span>
              </label>
            ))}
            <p className="text-[11px] text-muted-foreground">{t('letterAi.neverSent')}</p>
          </div>
          <div className="space-y-1">
            <Label htmlFor="letter-instruction" className="text-xs">
              {t('draft.instruction')}
            </Label>
            <Textarea
              id="letter-instruction"
              rows={2}
              value={instruction}
              maxLength={500}
              onChange={(e) => setInstruction(e.target.value)}
              placeholder={t('letterAi.instructionPlaceholder')}
              data-testid="letter-instruction"
            />
          </div>
          <Button
            className="w-full"
            disabled={preparing || empty.length === 0 || nothing}
            onClick={() => {
              setPreparing(true)
              flush()
                .then(() => setOpen(true))
                .catch((e) => toast.error(errorMessage(e)))
                .finally(() => setPreparing(false))
            }}
            data-testid="letter-ai-write"
          >
            <Sparkles /> {t('letterAi.write')}
          </Button>
          <p className="text-[11px] text-muted-foreground">
            {empty.length === 0 ? t('letterAi.allFilled') : nothing ? t('letterAi.nothing') : t('letterAi.writes', { count: empty.length })}
          </p>
        </>
      )}
      {open && (
        <RedactionDialog
          reportId={report.id}
          patientId={patient.id}
          previewKey={['letterPreview', report.id, choice]}
          loadPreview={() => api.letterPreview(report.id, choice)}
          progressEvent="draft-progress"
          labels={{
            send: t('letterAi.send'),
            runLocal: t('letterAi.write'),
            running: t('letterAi.running'),
            part: () => t('letterAi.running'),
            parts: () => t('letterAi.oneRequest'),
          }}
          onRun={async (digest, requestId) => {
            const r = await api.letterRun(report.id, choice, digest, requestId)
            await onWritten()
            resultToast(t, r)
          }}
          onClose={() => setOpen(false)}
        />
      )}
    </SectionCard>
  )
}

function resultToast(t: (k: string, o?: Record<string, unknown>) => string, r: { written: string[]; sentences: number; unsupported: number; dropped: number; cancelled: boolean }) {
  if (r.cancelled) toast.info(t('facts.cancelledNothing'))
  else if (r.written.length === 0) toast.info(t('letterAi.nothingWritten'))
  else
    toast.success(
      t('letterAi.done', { count: r.written.length }) +
        (r.unsupported ? ` ${t('letterAi.unsupportedNote', { count: r.unsupported })}` : '') +
        (r.dropped ? ` ${t('draft.dropped', { count: r.dropped })}` : ''),
    )
}

/** The pasted request: its text, the questions found in it, and turning them into sections. */
export function RequestCard({
  view,
  letter,
  bodies,
  flush,
  onSections,
  onDeadline,
}: {
  view: ReportView
  letter: Letter
  bodies: Record<string, string>
  flush: () => Promise<void>
  onSections: () => Promise<void>
  onDeadline: (day: string) => void
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const { report, patient, template } = view
  const llm = useQuery(q.llm())
  const sources = useQuery(q.letterSources(report.id))
  const [text, setText] = useState(letter.request_text)
  const [open, setOpen] = useState(!!letter.request_text)
  const [finding, setFinding] = useState(false)
  const [findingDialog, setFindingDialog] = useState(false)
  const [arranging, setArranging] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined)
  const pending = useRef<string | null>(null)

  const store = (l: Letter) => {
    qc.setQueryData(q.letter(report.id).queryKey, l)
    void qc.invalidateQueries({ queryKey: ['letter', report.id, 'sources'] })
    void qc.invalidateQueries({ queryKey: ['checklist', report.id] })
  }
  const saveText = (v: string) => {
    pending.current = null
    return api.saveLetterRequest(report.id, v).then(store)
  }
  useEffect(
    () => () => {
      clearTimeout(timer.current)
      if (pending.current !== null) void api.saveLetterRequest(report.id, pending.current)
    },
    [report.id],
  )
  const flushText = async () => {
    clearTimeout(timer.current)
    if (pending.current !== null) await saveText(pending.current)
  }

  const removeQuestion = useMutation({
    mutationFn: (i: number) => api.setLetterQuestions(report.id, letter.questions.filter((_, j) => j !== i).map((x) => x.text)),
    onSuccess: store,
    onError: (e) => toast.error(errorMessage(e)),
  })

  // A read document goes below what is there and is saved at once.
  const imported = async (added: string) => {
    clearTimeout(timer.current)
    const v = appendText(pending.current ?? text, added)
    setText(v)
    setOpen(true)
    await saveText(v)
  }

  const answered = (key: string | null) => !!key && !isBlank(bodies[key] ?? '')
  const deadline = sources.data?.deadline
  const linked = letter.questions.some((x) => x.section_key)
  const requestSections = template.sections.filter((s) => s.key.startsWith('frage-'))

  if (!open) {
    return (
      <div className="flex gap-2">
        <Button variant="outline" size="sm" className="flex-1" onClick={() => setOpen(true)} data-testid="letter-request-open">
          <FileQuestion /> {t('letterAi.requestAdd')}
        </Button>
        <DocumentButton purpose="request" onRead={(i) => imported(i.text)} testId="letter-request-file" />
      </div>
    )
  }
  return (
    <SectionCard
      icon={FileQuestion}
      title={t('letterAi.requestTitle')}
      contentClassName="space-y-3 text-sm"
    >
      <Textarea
        rows={4}
        value={text}
        placeholder={t('letterAi.requestPlaceholder')}
        onChange={(e) => {
          const v = e.target.value
          setText(v)
          pending.current = v
          clearTimeout(timer.current)
          timer.current = setTimeout(() => void saveText(v).catch((err) => toast.error(errorMessage(err))), REQUEST_SAVE_MS)
        }}
        className="max-h-60 text-[13px]"
        data-testid="letter-request"
      />
      <div className="flex items-start gap-2">
        <p className="flex-1 text-[11px] text-muted-foreground">{t('letterAi.requestHint')}</p>
        <DocumentButton purpose="request" onRead={(i) => imported(i.text)} testId="letter-request-file" />
      </div>
      {deadline && letter.follow_up_on !== deadline && (
        <Button variant="outline" size="sm" className="w-full" onClick={() => onDeadline(deadline)} data-testid="letter-deadline">
          <CalendarClock /> {t('letterAi.deadline', { date: germanDay(deadline) })}
        </Button>
      )}
      {llm.data?.config.active && (
        <Button
          variant="outline"
          size="sm"
          className="w-full"
          disabled={!text.trim() || finding}
          onClick={() => {
            setFinding(true)
            flushText()
              .then(() => setFindingDialog(true))
              .catch((e) => toast.error(errorMessage(e)))
              .finally(() => setFinding(false))
          }}
          data-testid="letter-find-questions"
        >
          <ListChecks /> {letter.questions.length ? t('letterAi.findAgain') : t('letterAi.find')}
        </Button>
      )}
      {letter.questions.length > 0 && (
        <div className="space-y-1.5">
          <p className="text-xs font-medium text-muted-foreground">{t('letterAi.questions', { count: letter.questions.length })}</p>
          <ol className="space-y-1" data-testid="letter-questions">
            {letter.questions.map((x, i) => (
              <li key={i} className="group flex items-start gap-1.5 text-[13px]">
                {answered(x.section_key) ? (
                  <Check className="mt-0.5 size-3.5 shrink-0 text-success" aria-label={t('letterAi.answered')} />
                ) : (
                  <Circle className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" aria-label={t('letterAi.open')} />
                )}
                <span className="flex-1">
                  {i + 1}. {x.text}
                </span>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-5 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                  aria-label={t('letterAi.removeQuestion')}
                  onClick={() => removeQuestion.mutate(i)}
                >
                  <X className="size-3" />
                </Button>
              </li>
            ))}
          </ol>
          <Button size="sm" className="w-full" variant={linked ? 'outline' : 'default'} onClick={() => setArranging(true)} data-testid="letter-arrange">
            <ListChecks /> {linked ? t('letterAi.arrange') : t('letterAi.asSections')}
          </Button>
        </div>
      )}
      {letter.questions.length === 0 && requestSections.length > 0 && (
        <Button size="sm" variant="outline" className="w-full" onClick={() => setArranging(true)}>
          <ListChecks /> {t('letterAi.arrange')}
        </Button>
      )}
      {findingDialog && (
        <RedactionDialog
          reportId={report.id}
          patientId={patient.id}
          previewKey={['letterQuestionsPreview', report.id, text]}
          loadPreview={() => api.letterQuestionsPreview(report.id)}
          progressEvent="draft-progress"
          labels={{
            send: t('letterAi.sendFind'),
            runLocal: t('letterAi.find'),
            running: t('letterAi.finding'),
            part: () => t('letterAi.finding'),
            parts: () => t('letterAi.oneRequest'),
          }}
          onRun={async (digest, requestId) => {
            const l = await api.letterQuestionsRun(report.id, digest, requestId)
            if (!l) {
              toast.info(t('facts.cancelledNothing'))
              return
            }
            store(l)
            if (l.questions.length === 0) toast.info(t('letterAi.noQuestions'))
            else toast.success(t('letterAi.found', { count: l.questions.length }))
          }}
          onClose={() => setFindingDialog(false)}
        />
      )}
      {arranging && (
        <ArrangeDialog
          view={view}
          letter={letter}
          bodies={bodies}
          onClose={() => setArranging(false)}
          onSaved={async (l) => {
            store(l)
            await flush()
            await onSections()
          }}
        />
      )}
    </SectionCard>
  )
}

/** One row of the arrange dialog. */
type Row = { key: string | null; heading: string; question: number | null }

/**
 * The request sections in her order: from the questions on first use, then rename, reorder or
 * drop them. "Vorlagenabschnitte ersetzen" lets the template's empty sections give way.
 */
function ArrangeDialog({
  view,
  letter,
  bodies,
  onClose,
  onSaved,
}: {
  view: ReportView
  letter: Letter
  bodies: Record<string, string>
  onClose: () => void
  onSaved: (l: Letter) => Promise<void>
}) {
  const { t } = useTranslation()
  const existing = view.template.sections.filter((s) => s.key.startsWith('frage-'))
  const [rows, setRows] = useState<Row[]>(() => {
    const out: Row[] = existing.map((s) => ({
      key: s.key,
      heading: s.heading,
      question: letter.questions.findIndex((x) => x.section_key === s.key),
    }))
    for (const r of out) if (r.question === -1) r.question = null
    // Questions not yet a section come after.
    letter.questions.forEach((x, i) => {
      if (!x.section_key || !existing.some((s) => s.key === x.section_key)) out.push({ key: null, heading: x.text, question: i })
    })
    return out
  })
  const [replace, setReplace] = useState(true)
  const save = useMutation({
    mutationFn: () => {
      const sections: RequestSection[] = rows.map((r) => ({ key: r.key, heading: r.heading.trim(), question: r.question }))
      return api.setLetterSections(view.report.id, sections, !replace)
    },
    onSuccess: async (l) => {
      await onSaved(l)
      toast.success(t('letterAi.arranged'))
      onClose()
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const move = (i: number, d: number) =>
    setRows((rs) => {
      const next = [...rs]
      const [r] = next.splice(i, 1)
      next.splice(i + d, 0, r)
      return next
    })
  const dropsText = rows.length === 0 && existing.some((s) => !isBlank(bodies[s.key] ?? ''))
  const removed = existing.filter((s) => !rows.some((r) => r.key === s.key) && !isBlank(bodies[s.key] ?? ''))

  return (
    <Dialog open onOpenChange={(o) => !o && !save.isPending && onClose()}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t('letterAi.arrangeTitle')}</DialogTitle>
          <DialogDescription>{t('letterAi.arrangeText')}</DialogDescription>
        </DialogHeader>
        <ol className="max-h-[50vh] space-y-1.5 overflow-y-auto" data-testid="letter-arrange-rows">
          {rows.map((r, i) => (
            <li key={i} className="flex items-center gap-1.5">
              <span className="w-5 text-right text-xs text-muted-foreground">{i + 1}.</span>
              <Input
                value={r.heading}
                onChange={(e) => setRows((rs) => rs.map((x, j) => (j === i ? { ...x, heading: e.target.value } : x)))}
                aria-label={t('letterAi.heading')}
              />
              <Button variant="ghost" size="icon" disabled={i === 0} onClick={() => move(i, -1)} aria-label={t('letterAi.up')}>
                <ArrowUp />
              </Button>
              <Button variant="ghost" size="icon" disabled={i === rows.length - 1} onClick={() => move(i, 1)} aria-label={t('letterAi.down')}>
                <ArrowDown />
              </Button>
              <Button variant="ghost" size="icon" onClick={() => setRows((rs) => rs.filter((_, j) => j !== i))} aria-label={t('letterAi.drop')}>
                <Trash2 />
              </Button>
            </li>
          ))}
        </ol>
        <Button variant="ghost" size="sm" className="justify-self-start" onClick={() => setRows((rs) => [...rs, { key: null, heading: '', question: null }])}>
          <Plus /> {t('letterAi.addSection')}
        </Button>
        <label className="flex items-start gap-2 text-sm">
          <Checkbox className="mt-0.5" checked={replace} onCheckedChange={(v) => setReplace(v === true)} data-testid="letter-arrange-replace" />
          <span>
            {t('letterAi.replaceTemplate')}
            <span className="block text-xs text-muted-foreground">{t('letterAi.replaceTemplateHint')}</span>
          </span>
        </label>
        {(removed.length > 0 || dropsText) && <p className="text-sm text-warning-foreground">{t('letterAi.dropsText', { count: removed.length })}</p>}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button disabled={save.isPending || rows.some((r) => !r.heading.trim())} onClick={() => save.mutate()} data-testid="letter-arrange-save">
            {t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** Every AI sentence still marked "ohne Beleg", to keep or delete. */
export function UnbackedCard({
  sections,
  bodies,
  onChange,
  onShow,
}: {
  sections: SectionDef[]
  bodies: Record<string, string>
  onChange: (key: string, body: string) => void
  onShow: (key: string) => void
}) {
  const { t } = useTranslation()
  const items = sections.flatMap((s) => unbackedSentences(bodies[s.key] ?? '').map((text, i) => ({ key: s.key, heading: s.heading, text, i })))
  if (items.length === 0) return null
  return (
    <SectionCard title={t('letterAi.unbackedTitle', { count: items.length })} contentClassName="space-y-2 text-sm" className="border-warning/50">
      <p className="text-[13px] text-muted-foreground">{t('letterAi.unbackedIntro')}</p>
      <ul className="space-y-2" data-testid="letter-unbacked">
        {items.map((it) => (
          <li key={`${it.key}:${it.i}`} className="rounded-md border border-warning/40 bg-warning/5 p-2">
            <button type="button" className="block text-left text-[13px] hover:underline" onClick={() => onShow(it.key)}>
              {it.heading && <span className="block text-[11px] text-muted-foreground">{it.heading}</span>}
              {it.text}
            </button>
            <div className="mt-1.5 flex gap-1.5">
              <Button size="sm" variant="outline" className="h-7" onClick={() => onChange(it.key, decideUnbacked(bodies[it.key] ?? '', it.i, true))} data-testid="unbacked-keep">
                <Check /> {t('letterAi.keep')}
              </Button>
              <Button size="sm" variant="ghost" className="h-7" onClick={() => onChange(it.key, decideUnbacked(bodies[it.key] ?? '', it.i, false))} data-testid="unbacked-delete">
                <Trash2 /> {t('letterAi.delete')}
              </Button>
            </div>
          </li>
        ))}
      </ul>
    </SectionCard>
  )
}

/** The focused section: "Neu schreiben" with an instruction, and "Woher kommt das?". */
export function ActiveSectionCard({
  view,
  sectionKey,
  bodies,
  flush,
  onWritten,
}: {
  view: ReportView
  sectionKey: string | null
  bodies: Record<string, string>
  flush: () => Promise<void>
  onWritten: () => Promise<void>
}) {
  const { t } = useTranslation()
  const { report, patient, template, episode } = view
  const llm = useQuery(q.llm())
  const drafts = useQuery(q.letterDrafts(report.id))
  const [instruction, setInstruction] = useState('')
  const [open, setOpen] = useState(false)
  const def = aiSections(template.sections).find((s) => s.key === sectionKey)
  useEffect(() => setInstruction(''), [sectionKey])
  if (!def) return null
  const draft = drafts.data?.find((d) => d.section_key === def.key)
  const filled = !isBlank(bodies[def.key] ?? '')
  const choice: LetterChoice = { sections: null, instruction: instruction.trim() || null, rewrite: def.key }
  const heading = def.heading.trim() || t('letterAi.opening')

  return (
    <SectionCard title={heading} contentClassName="space-y-3 text-sm" className="letter-active-section">
      {llm.data?.config.active && episode && filled && (
        <div className="space-y-1.5">
          <Label htmlFor="letter-rewrite" className="text-xs">
            {t('letterAi.rewriteLabel')}
          </Label>
          <div className="space-y-1.5">
            <Input
              id="letter-rewrite"
              value={instruction}
              maxLength={500}
              placeholder={t('letterAi.rewritePlaceholder')}
              onChange={(e) => setInstruction(e.target.value)}
              data-testid="letter-rewrite-instruction"
            />
            <Button
              variant="outline"
              className="w-full"
              disabled={!instruction.trim()}
              onClick={() =>
                void flush()
                  .then(() => setOpen(true))
                  .catch((e) => toast.error(errorMessage(e)))
              }
              data-testid="letter-rewrite"
            >
              <RefreshCw /> {t('letterAi.rewrite')}
            </Button>
          </div>
        </div>
      )}
      <div className="space-y-1">
        <p className="text-xs font-medium text-muted-foreground">{t('letterAi.whence')}</p>
        {draft ? (
          <div className="text-[13px]" data-testid="letter-whence">
            <p className="mb-1 text-[11px] text-muted-foreground">{t('letterAi.whenceModel', { model: draft.model, date: formatDate(draft.applied_at, true) })}</p>
            <Sentences draft={draft} noteSections={[]} />
            {draft.sentences.some((x) => x.unsupported) && <p className="mt-1 text-[11px] text-warning-foreground">{t('letterAi.whenceUnbacked')}</p>}
          </div>
        ) : (
          <p className="text-[13px] text-muted-foreground">{filled ? t('letterAi.ownText') : t('letterAi.emptySection')}</p>
        )}
      </div>
      {open && (
        <RedactionDialog
          reportId={report.id}
          patientId={patient.id}
          previewKey={['letterPreview', report.id, choice]}
          loadPreview={() => api.letterPreview(report.id, choice)}
          progressEvent="draft-progress"
          labels={{
            send: t('letterAi.sendRewrite'),
            runLocal: t('letterAi.rewrite'),
            running: t('letterAi.running'),
            part: () => t('letterAi.running'),
            parts: () => t('letterAi.oneRequest'),
          }}
          onRun={async (digest, requestId) => {
            const r = await api.letterRun(report.id, choice, digest, requestId)
            await onWritten()
            resultToast(t, r)
            if (!r.cancelled && r.written.length) setInstruction('')
          }}
          onClose={() => setOpen(false)}
        />
      )}
    </SectionCard>
  )
}
