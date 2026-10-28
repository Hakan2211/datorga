// Diktat-Werkstatt (slice 24): a big free-text area she thinks out loud into, the text appearing
// live; "Entwurf erstellen" then turns it into a proposal per section of the Sitzungsnotiz or
// Bericht, reviewed and applied like any other draft (replace / append / reject).
import { useCallback, useEffect, useId, useRef, useState } from 'react'
import { Link } from '@tanstack/react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { EditorContent, useEditor } from '@tiptap/react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { ArrowRight, Loader2, Mic, MicOff, ShieldCheck, Sparkles } from 'lucide-react'
import type { ApplyMode } from '@/bindings/ApplyMode'
import type { SectionDraft } from '@/bindings/SectionDraft'
import { Page, PageHeader } from '@/components/app-shell'
import { RedactionDialog } from '@/components/fact-draft'
import { DraftProposal } from '@/components/report-draft'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { api, errorMessage, q, type ReportView, type WorkshopChoice } from '@/lib/api'
import { useCaptureDevice } from '@/lib/capture'
import { registerTarget, startDictation, stopAndSettle, stopDictation, targetPhase, useDictation } from '@/lib/dictation'
import { insertDictated, richExtensions, setDictationPending } from '@/lib/editor'
import { isBlank } from '@/lib/report'
import { cn } from '@/lib/utils'

const SAVE_DELAY_MS = 700

export function Workshop({ view, title }: { view: ReportView; title: string }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const { report, template, patient } = view
  const text = useQuery({ queryKey: ['workshop', report.id], queryFn: () => api.workshopGet(report.id), staleTime: Infinity })
  const readOnly = report.status === 'signed' || report.status === 'exported'
  // Saves the text still waiting for its debounce (set by the pad).
  const flush = useRef<() => Promise<void>>(async () => {})
  const reportLink = (
    <Button asChild variant="outline" size="sm">
      <Link to="/reports/$reportId" params={{ reportId: report.id }}>
        {t(report.kind === 'sitzungsnotiz' ? 'workshop.toNote' : 'workshop.toReport')} <ArrowRight />
      </Link>
    </Button>
  )

  return (
    <div className="flex min-h-full flex-col">
      <PageHeader
        title={t('workshop.title', { what: title })}
        subtitle={
          <span className="flex items-center gap-2">
            <Link to="/patients/$patientId" params={{ patientId: patient.id }} className="hover:underline">
              {patient.first_name} {patient.last_name}
            </Link>
            <Badge variant="outline" className="font-mono">
              {patient.chiffre ?? t('patients.noChiffre')}
            </Badge>
            <span className="flex items-center gap-1 rounded-full bg-primary/10 px-2.5 py-0.5 text-xs text-primary">
              <ShieldCheck className="size-3.5" /> {t('workshop.privacy')}
            </span>
          </span>
        }
        actions={reportLink}
      />
      <Page>
        {readOnly ? (
          <p className="rounded-md bg-success/10 px-4 py-2 text-sm text-success">{t('workshop.readOnly')}</p>
        ) : text.data === undefined ? null : (
          <div className="grid grid-cols-[minmax(0,1fr)_22rem] gap-6">
            <DictationPad
              reportId={report.id}
              initial={text.data}
              onSaved={(body) => qc.setQueryData(['workshop', report.id], body)}
              flushRef={flush}
            />
            <ProposalPanel view={view} text={text.data} beforeDraft={() => flush.current()} />
          </div>
        )}
        <p className="text-xs text-muted-foreground">{t('workshop.hint', { template: template.title })}</p>
      </Page>
    </div>
  )
}

/** The free-text area with the big mic. Saved as she goes (debounced). */
function DictationPad({
  reportId,
  initial,
  onSaved,
  flushRef,
}: {
  reportId: string
  initial: string
  onSaved: (body: string) => void
  flushRef: React.RefObject<() => Promise<void>>
}) {
  const { t } = useTranslation()
  const id = useId()
  const d = useDictation()
  const { startId } = useCaptureDevice()
  const phase = targetPhase(d, id)
  const on = d.targetId === id
  const [words, setWords] = useState(0)
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined)
  const queued = useRef<string | null>(null)

  const save = useCallback(
    (body: string) => {
      queued.current = null
      return api
        .workshopSet(reportId, body)
        .then(() => onSaved(body))
        .catch((e) => toast.error(errorMessage(e)))
    },
    [reportId, onSaved],
  )
  // Before a draft: let the last dictated piece arrive, then save what is still queued.
  flushRef.current = async () => {
    if (d.targetId === id || d.finishing.includes(id)) await stopAndSettle(id)
    clearTimeout(timer.current)
    if (queued.current !== null) await save(queued.current)
  }

  const editor = useEditor({
    extensions: richExtensions({ placeholder: t('workshop.placeholder') }),
    content: initial,
    immediatelyRender: false,
    editorProps: { attributes: { spellcheck: 'true', lang: 'de', class: 'workshop-text', 'data-testid': 'workshop-text' } },
    onCreate: ({ editor }) => setWords(countWords(editor.getText())),
    onUpdate: ({ editor }) => {
      setWords(countWords(editor.getText()))
      const body = editor.isEmpty ? '' : editor.getHTML()
      queued.current = body
      clearTimeout(timer.current)
      timer.current = setTimeout(() => save(body), SAVE_DELAY_MS)
    },
  })

  // Leaving the page must not drop text still waiting for its debounce.
  useEffect(
    () => () => {
      clearTimeout(timer.current)
      if (queued.current !== null) void api.workshopSet(reportId, queued.current)
    },
    [reportId],
  )
  useEffect(() => {
    if (!editor) return
    return registerTarget(id, (text) => {
      // Each piece goes to the end of the text unless she placed the cursor elsewhere.
      insertDictated(editor, text)
    })
  }, [id, editor])
  useEffect(() => {
    if (editor) setDictationPending(editor, phase === 'recognizing')
  }, [editor, phase])
  useEffect(() => {
    editor?.commands.focus('end')
  }, [editor])

  const level = on ? Math.min(1, (d.status?.level ?? 0) * 3) : 0
  return (
    <div className="flex min-w-0 flex-col gap-5">
      <Card className="min-h-[24rem] flex-1 gap-0 py-0">
        <CardContent className="prose-editor flex-1 px-7 py-6 font-serif text-[17px] leading-relaxed">
          <EditorContent editor={editor} />
        </CardContent>
        <div className="flex items-center justify-between border-t px-5 py-2 text-xs text-muted-foreground">
          <span className="flex items-center gap-1.5" data-testid="workshop-phase">
            <span className={cn('size-2 rounded-full bg-muted-foreground/40', on && 'bg-success')} />
            {t(`dictation.phase.${phase}`)}
          </span>
          <span>{t('workshop.words', { count: words })}</span>
        </div>
      </Card>
      <div className="flex flex-col items-center gap-2">
        <button
          type="button"
          data-testid="workshop-mic"
          aria-label={on ? t('dictation.stop') : t('dictation.start')}
          aria-pressed={on}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => void (on ? stopDictation() : startDictation(id, reportId, startId))}
          className={cn(
            'grid size-16 place-items-center rounded-full bg-primary text-primary-foreground shadow-md transition-transform hover:scale-105',
            on && 'bg-destructive text-white',
          )}
          style={{ boxShadow: on ? `0 0 0 ${6 + level * 18}px color-mix(in oklch, var(--destructive) 18%, transparent)` : undefined }}
        >
          {phase === 'starting' || phase === 'loading' ? (
            <Loader2 className="size-6 animate-spin" />
          ) : on ? (
            <MicOff className="size-6" />
          ) : (
            <Mic className="size-6" />
          )}
        </button>
        <span className="text-sm text-muted-foreground">
          {on ? t(`dictation.phase.${phase}`) : t('workshop.micHint')}
          {on && d.status && <span className="ml-1.5 text-xs">· {t(`dictation.engine.${d.status.engine}`)}</span>}
        </span>
      </div>
    </div>
  )
}

function countWords(text: string): number {
  return text.split(/\s+/).filter((w) => /\w/.test(w)).length
}

/** "Entwurf erstellen" and the proposals it produced, per section. */
function ProposalPanel({ view, text, beforeDraft }: { view: ReportView; text: string; beforeDraft: () => Promise<void> }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const { report, template, patient } = view
  const llm = useQuery(q.llm())
  const drafts = useQuery(q.reportDrafts(report.id))
  const [open, setOpen] = useState(false)
  const [preparing, setPreparing] = useState(false)
  const [instruction, setInstruction] = useState('')
  const current = bodies(view)
  const filled = template.sections.some((s) => s.field === 'prose' && !isBlank(current[s.key] ?? ''))
  const [withExisting, setWithExisting] = useState(filled)
  const proposals = (drafts.data ?? []).filter((d) => d.status === 'proposed' && d.source === 'dictation')
  const active = llm.data?.config.active
  const choice: WorkshopChoice = { instruction: instruction.trim() || null, withExisting: withExisting && filled }
  const previewKey = ['workshopPreview', report.id, choice]
  const heading = (key: string) => template.sections.find((s) => s.key === key)?.heading ?? key

  const refreshReport = () => {
    void qc.invalidateQueries({ queryKey: ['report', report.id] })
    void qc.invalidateQueries({ queryKey: ['reportDrafts', report.id] })
    void qc.invalidateQueries({ queryKey: ['checklist', report.id] })
  }
  const apply = async (d: SectionDraft, mode: ApplyMode) => {
    await api.applyDraft(report.id, d.id, mode)
    refreshReport()
    toast.success(t('draft.applied'))
  }
  const reject = async (d: SectionDraft) => {
    qc.setQueryData(q.reportDrafts(report.id).queryKey, await api.rejectDraft(report.id, d.id))
  }

  return (
    <aside className="space-y-4">
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2 text-base">
            <Sparkles className="size-4 text-primary" /> {t('workshop.draftTitle')}
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3 text-sm">
          <p className="text-muted-foreground">{t('workshop.draftIntro')}</p>
          {!active ? (
            <p className="text-muted-foreground">
              {t('facts.noProvider')}{' '}
              <Link to="/settings" className="underline">
                {t('facts.settingsLink')}
              </Link>
            </p>
          ) : (
            <>
              <div className="space-y-1">
                <Label htmlFor="workshop-instruction" className="text-xs">
                  {t('draft.instruction')}
                </Label>
                <Input
                  id="workshop-instruction"
                  value={instruction}
                  maxLength={500}
                  onChange={(e) => setInstruction(e.target.value)}
                  placeholder={t('draft.instructionPlaceholder')}
                />
              </div>
              {filled && (
                <label className="flex items-start gap-2 text-[13px]">
                  <Checkbox className="mt-0.5" checked={withExisting} onCheckedChange={(v) => setWithExisting(v === true)} />
                  {t('draft.withExisting')}
                </label>
              )}
              <Button
                className="w-full"
                disabled={isBlank(text) || preparing}
                onClick={() => {
                  setPreparing(true)
                  beforeDraft()
                    .then(() => setOpen(true))
                    .finally(() => setPreparing(false))
                }}
                data-testid="workshop-draft"
              >
                <Sparkles /> {t('workshop.draft')}
              </Button>
              {isBlank(text) && <p className="text-[13px] text-muted-foreground">{t('workshop.empty')}</p>}
            </>
          )}
        </CardContent>
      </Card>
      {proposals.length > 0 && (
        <div className="space-y-3" data-testid="workshop-proposals">
          {proposals.map((d) => (
            <div key={d.id} className="space-y-1.5">
              <p className="text-sm font-medium">{heading(d.section_key)}</p>
              <DraftProposal
                draft={d}
                sectionEmpty={isBlank(current[d.section_key] ?? '')}
                noteSections={[]}
                onApply={(mode) => apply(d, mode)}
                onReject={() => reject(d)}
              />
            </div>
          ))}
        </div>
      )}
      {open && (
        <RedactionDialog
          reportId={report.id}
          patientId={patient.id}
          previewKey={previewKey}
          loadPreview={() => api.workshopPreview(report.id, choice)}
          progressEvent="draft-progress"
          labels={{
            send: t('draft.send'),
            runLocal: t('draft.runLocal'),
            running: t('workshop.running'),
            part: () => t('workshop.running'),
            parts: () => t('workshop.parts'),
          }}
          onRun={async (digest, requestId) => {
            const r = await api.workshopRun(report.id, choice, digest, requestId)
            qc.setQueryData(q.reportDrafts(report.id).queryKey, r.drafts)
            if (r.cancelled) toast.info(t('facts.cancelledNothing'))
            else if (r.sentences === 0) toast.info(t('workshop.nothing'))
            else
              toast.success(
                t('workshop.done', { count: r.drafts.filter((d) => d.status === 'proposed' && d.source === 'dictation').length }) +
                  (r.dropped ? ` ${t('draft.dropped', { count: r.dropped })}` : ''),
              )
          }}
          onClose={() => setOpen(false)}
        />
      )}
    </aside>
  )
}

function bodies(view: ReportView): Record<string, string> {
  return Object.fromEntries(view.report.sections.map((s) => [s.key, s.body]))
}
