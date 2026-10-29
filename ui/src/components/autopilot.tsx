import { useEffect, useRef, useState } from 'react'
import { Link, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { AlertTriangle, CheckCircle2, CircleDashed, Eye, FilePlus2, Loader2, MinusCircle, RotateCcw, ShieldCheck, Workflow, X } from 'lucide-react'
import type { ApplicationKind } from '@/bindings/ApplicationKind'
import type { AutopilotStep } from '@/bindings/AutopilotStep'
import type { AutopilotStepKind } from '@/bindings/AutopilotStepKind'
import { SectionCard } from '@/components/bits'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { api, errorMessage, jobActive, q, type DraftChoice } from '@/lib/api'
import { cn } from '@/lib/utils'
import { HELP } from '@/lib/help'

const TOKEN = /(\[(?:PERSON|ORT|ARBEITGEBER|DATUM)_\d+\])/

/**
 * App-wide: a finished transcription announces itself (it waits for her review), and each
 * autopilot step refreshes what it changed.
 */
export function useAutopilotEvents() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const navigate = useNavigate()
  useEffect(() => {
    const ready = api.onTranscriptReady((e) => {
      void qc.invalidateQueries({ queryKey: ['session', e.session_id] })
      void qc.invalidateQueries({ queryKey: ['calendar'] })
      toast.success(t('transcript.readyToast', { name: e.patient_name }), {
        action: { label: t('transcript.readyAction'), onClick: () => void navigate({ to: '/sessions/$sessionId', params: { sessionId: e.session_id } }) },
      })
    })
    const changed = api.onAutopilotChanged((sessionId) => {
      for (const key of [['autopilot', sessionId], ['session', sessionId], ['sessionSummary', sessionId], ['noteFacts'], ['sessions'], ['calendar'], ['report']]) {
        void qc.invalidateQueries({ queryKey: key })
      }
    })
    return () => {
      void ready.then((f) => f())
      void changed.then((f) => f())
    }
  }, [qc, t, navigate])
}

const statusIcon = {
  pending: CircleDashed,
  running: Loader2,
  done: CheckCircle2,
  skipped: MinusCircle,
  failed: AlertTriangle,
} as const

/** The session's autopilot run: one line per step, retry on failure, what was sent. */
export function AutopilotCard({ sessionId, noteId }: { sessionId: string; noteId: string | null }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const run = useQuery(q.autopilotRun(sessionId))
  const [sent, setSent] = useState<AutopilotStep | null>(null)
  const retry = useMutation({
    mutationFn: (jobId: string) => api.retryAutopilot(jobId),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['autopilot', sessionId] }),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const data = run.data
  if (!data) return null
  const active = jobActive(data.job)
  const retryable = !active && data.steps.some((s) => s.status === 'failed' || s.status === 'skipped')
  const failed = data.steps.some((s) => s.status === 'failed')

  return (
    <SectionCard
      help={HELP.autopilot}
      icon={Workflow}
      title={
        <span className="flex items-center gap-2">
          {t('autopilot.title')}
          {active && (
            <span className="flex items-center gap-1 text-xs font-normal text-muted-foreground">
              <Loader2 className="size-3.5 animate-spin" /> {t(data.job.status === 'queued' ? 'autopilot.queued' : 'autopilot.running')}
            </span>
          )}
        </span>
      }
      actions={
        retryable && (
          <Button size="sm" variant={failed ? 'default' : 'outline'} onClick={() => retry.mutate(data.job.id)} disabled={retry.isPending}>
            <RotateCcw /> {t('autopilot.retry')}
          </Button>
        )
      }
      contentClassName="space-y-3 text-sm"
    >
      <ul className="space-y-1.5" data-testid="autopilot-steps">
        {data.steps.map((s) => (
          <StepRow key={s.step} step={s} onSent={() => setSent(s)} />
        ))}
      </ul>
      {!active && noteId && (
        <p className="flex flex-wrap items-center gap-2 text-muted-foreground">
          {t('autopilot.review')}
          <Link to="/reports/$reportId" params={{ reportId: noteId }} className="text-foreground underline">
            {t('flow.session.writeNote')}
          </Link>
        </p>
      )}
      {sent && <SentDialog jobId={data.job.id} step={sent} onClose={() => setSent(null)} />}
    </SectionCard>
  )
}

function StepRow({ step, onSent }: { step: AutopilotStep; onSent: () => void }) {
  const { t } = useTranslation()
  const Icon = statusIcon[step.status]
  return (
    <li className="flex flex-wrap items-center gap-x-2 gap-y-0.5" data-step={step.step} data-status={step.status}>
      <Icon
        className={cn(
          'size-4 shrink-0',
          step.status === 'running' && 'animate-spin text-primary',
          step.status === 'done' && 'text-success',
          step.status === 'failed' && 'text-destructive',
          (step.status === 'pending' || step.status === 'skipped') && 'text-muted-foreground',
        )}
      />
      <span className="font-medium">{t(`autopilot.step.${step.step}`)}</span>
      <span className="text-muted-foreground">· {t(`autopilot.status.${step.status}`)}</span>
      {step.detail && <span className={cn('text-[13px]', step.status === 'failed' ? 'text-destructive' : 'text-muted-foreground')}>– {step.detail}</span>}
      {step.sent && (
        <Button size="sm" variant="ghost" className="ml-auto h-7" onClick={onSent} data-testid={`sent-${step.step}`}>
          <Eye /> {t('autopilot.sent')}
        </Button>
      )}
    </li>
  )
}

/** "Was wurde gesendet?" on the fact card: only when the autopilot sent this note's facts. */
export function SentLink({ sessionId, step }: { sessionId: string; step: AutopilotStepKind }) {
  const { t } = useTranslation()
  const run = useQuery(q.autopilotRun(sessionId))
  const [open, setOpen] = useState(false)
  const job = run.data?.job
  const done = run.data?.steps.find((s) => s.step === step && s.sent)
  if (!job || !done) return null
  return (
    <>
      <Button size="sm" variant="ghost" onClick={() => setOpen(true)} data-testid={`sent-link-${step}`}>
        <Eye /> {t('autopilot.sent')}
      </Button>
      {open && <SentDialog jobId={job.id} step={done} onClose={() => setOpen(false)} />}
    </>
  )
}

function SentDialog({ jobId, step, onClose }: { jobId: string; step: AutopilotStep; onClose: () => void }) {
  const { t } = useTranslation()
  const sent = useQuery({ queryKey: ['autopilot', 'sent', jobId, step.step], queryFn: () => api.autopilotSent(jobId, step.step), gcTime: 0 })
  const data = sent.data
  const target = [step.provider, step.model].filter(Boolean).join(' · ')
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90vh] grid-rows-[auto_minmax(0,1fr)] sm:max-w-3xl" data-testid="sent-dialog">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldCheck className="size-5 text-primary" /> {t('autopilot.sent')} · {t(`autopilot.step.${step.step}`)}
          </DialogTitle>
          <DialogDescription>{t('autopilot.sentIntro', { target })}</DialogDescription>
        </DialogHeader>
        <div className="min-h-0 space-y-4 overflow-y-auto pr-1 text-sm">
          {sent.isError && <p className="text-destructive">{errorMessage(sent.error)}</p>}
          {sent.isSuccess && !data && <p className="text-muted-foreground">{t('autopilot.sentNone')}</p>}
          {data && (
            <>
              <section className="space-y-1.5">
                <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">{t('facts.preview.tokens')}</h3>
                {data.tokens.length === 0 ? (
                  <p className="text-muted-foreground">{t('facts.preview.noTokens')}</p>
                ) : (
                  <ul className="flex flex-wrap gap-1.5">
                    {data.tokens.map((tk) => (
                      <li key={tk.token} className="rounded bg-muted px-2 py-0.5 text-[13px]">
                        <span className="font-mono text-primary">{tk.token}</span> = {tk.original}
                      </li>
                    ))}
                  </ul>
                )}
              </section>
              <section className="space-y-1.5">
                <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">{t('facts.preview.outgoing')}</h3>
                <div
                  className="max-h-96 space-y-3 overflow-y-auto rounded-md border bg-muted/30 p-3 font-mono text-[12.5px] leading-relaxed whitespace-pre-wrap"
                  data-testid="sent-outgoing"
                >
                  {data.messages.map((m, i) => (
                    <div key={i}>
                      {m.split(TOKEN).map((part, j) =>
                        j % 2 === 1 ? (
                          <mark key={j} className="rounded bg-primary/15 px-0.5 text-primary">
                            {part}
                          </mark>
                        ) : (
                          part
                        ),
                      )}
                    </div>
                  ))}
                </div>
              </section>
            </>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}

/**
 * "Antrag anlegen" from a due contingent: creates the application and its Bericht (diagnoses
 * from the Behandlungsfall) and opens it; with autopilot on the Bericht drafts itself.
 */
export function usePrepareApplication() {
  const qc = useQueryClient()
  const navigate = useNavigate()
  return useMutation({
    mutationFn: async ({ episodeId, kind }: { episodeId: string; kind: ApplicationKind }) => {
      const app = await api.createApplication(episodeId, kind)
      return { app, autopilot: await api.autopilotEnabled() }
    },
    onSuccess: ({ app, autopilot }) => {
      for (const key of [['applications', app.episode_id], ['reports'], ['calendar'], ['contingent'], ['timeline']]) {
        void qc.invalidateQueries({ queryKey: key })
      }
      if (app.report_id) {
        void navigate({ to: '/reports/$reportId', params: { reportId: app.report_id }, search: autopilot ? { autodraft: true } : {} })
      }
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
}

/**
 * Autopilot for a new Bericht: drafts all AI-allowed sections as proposals in one cancelable
 * run, without the preview (the user's decision). Nothing is applied.
 */
export function AutoDraft({ reportId, keys, onDone }: { reportId: string; keys: string[]; onDone: () => void }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const sources = useQuery(q.draftSources(reportId))
  const [requestId] = useState(() => crypto.randomUUID())
  const [state, setState] = useState<'running' | 'nothing' | 'done'>('running')
  const started = useRef(false)
  useEffect(() => {
    if (started.current || !sources.data) return
    started.current = true
    if (sources.data.facts.length === 0 || keys.length === 0) {
      setState('nothing')
      return
    }
    const choice: DraftChoice = { sections: keys, instruction: null, sessionIds: null, withExisting: false, allFacts: false }
    api
      .draftPreview(reportId, choice)
      .then((p) => api.draftRun(reportId, choice, p.digest, requestId))
      .then((r) => {
        qc.setQueryData(q.reportDrafts(reportId).queryKey, r.drafts)
        setState('done')
        toast.success(t('autopilot.draftDone', { count: r.drafts.filter((d) => d.status === 'proposed').length }))
        onDone()
      })
      .catch((e) => {
        toast.error(errorMessage(e))
        onDone()
      })
  }, [sources.data, keys, reportId, requestId, qc, t, onDone])

  if (state === 'done') return null
  return (
    <div className="flex items-center gap-3 rounded-lg border border-primary/30 bg-primary/5 px-4 py-3 text-sm" data-testid="autodraft">
      {state === 'running' ? <Loader2 className="size-4 animate-spin text-primary" /> : <FilePlus2 className="size-4 text-muted-foreground" />}
      <span className="flex-1">{t(state === 'running' ? 'autopilot.drafting' : 'autopilot.draftNothing')}</span>
      {state === 'running' ? (
        <Button size="sm" variant="ghost" onClick={() => api.llmCancel(requestId).catch((e) => toast.error(errorMessage(e)))}>
          <X /> {t('transcript.cancel')}
        </Button>
      ) : (
        <Button size="sm" variant="ghost" onClick={onDone} aria-label={t('common.close')}>
          <X />
        </Button>
      )}
    </div>
  )
}
