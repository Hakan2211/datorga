import { useState } from 'react'
import { Link } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Pencil, Plus, Sparkles, X } from 'lucide-react'
import type { SessionSummary } from '@/bindings/SessionSummary'
import type { SummaryPointView } from '@/bindings/SummaryPointView'
import { SectionCard, StatusPill } from '@/components/bits'
import { RedactionDialog, clock } from '@/components/fact-draft'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage, q } from '@/lib/api'
import { cn } from '@/lib/utils'
import { HELP } from '@/lib/help'

/** Refreshes everything that shows a session's key points. */
function useSetSummary(sessionId: string, patientId: string) {
  const qc = useQueryClient()
  return (s: SessionSummary | null) => {
    qc.setQueryData(q.sessionSummary(sessionId).queryKey, s)
    void qc.invalidateQueries({ queryKey: ['timeline', patientId] })
  }
}

/** "KI-Zusammenfassung" or "bearbeitet", as a small pill. */
export function SummaryStatus({ summary }: { summary: SessionSummary }) {
  const { t } = useTranslation()
  return (
    <StatusPill tone={summary.status === 'edited' ? 'neutral' : 'primary'}>
      <Sparkles className="size-3" /> {t(summary.status === 'edited' ? 'summary.edited' : 'summary.ai')}
    </StatusPill>
  )
}

/**
 * The key points, each with its sources one click away; "bearbeiten" edits them in place
 * (shared by the session page and the patient timeline).
 */
export function SummaryPoints({
  summary,
  patientId,
  editing,
  onDone,
  compact = false,
}: {
  summary: SessionSummary
  patientId: string
  editing: boolean
  onDone: () => void
  compact?: boolean
}) {
  const { t } = useTranslation()
  const setSummary = useSetSummary(summary.session_id, patientId)
  const [rows, setRows] = useState<SummaryPointView[]>(summary.points)
  const save = useMutation({
    mutationFn: () =>
      api.summaryEdit(
        summary.session_id,
        rows.map((p) => ({ text: p.text, segments: p.citations.map((c) => c.segment_id), note: p.note })),
      ),
    onSuccess: (s) => {
      setSummary(s)
      toast.success(t('common.saved'))
      onDone()
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  if (editing) {
    const set = (i: number, text: string) => setRows((all) => all.map((p, k) => (k === i ? { ...p, text } : p)))
    return (
      <div className="space-y-2" data-testid="summary-edit">
        {rows.map((p, i) => (
          <div key={i} className="flex items-start gap-2">
            <Textarea
              value={p.text}
              onChange={(e) => set(i, e.target.value)}
              rows={2}
              className="min-h-0 text-sm"
              aria-label={t('summary.pointLabel', { n: i + 1 })}
            />
            <Button
              size="icon"
              variant="ghost"
              onClick={() => setRows((all) => all.filter((_, k) => k !== i))}
              aria-label={t('summary.removePoint')}
            >
              <X />
            </Button>
          </div>
        ))}
        <div className="flex items-center gap-2">
          <Button size="sm" variant="ghost" onClick={() => setRows((all) => [...all, { text: '', citations: [], note: [] }])}>
            <Plus /> {t('summary.addPoint')}
          </Button>
          <span className="flex-1" />
          <Button size="sm" variant="ghost" onClick={onDone}>
            {t('common.cancel')}
          </Button>
          <Button size="sm" onClick={() => save.mutate()} disabled={save.isPending}>
            {t('common.save')}
          </Button>
        </div>
      </div>
    )
  }
  if (summary.points.length === 0) {
    return <p className="text-sm text-muted-foreground">{t('summary.nothing')}</p>
  }
  return (
    <ul className={cn('space-y-1.5', compact ? 'text-[13px]' : 'text-sm')} data-testid="summary-points">
      {summary.points.map((p, i) => (
        <PointRow key={i} point={p} />
      ))}
    </ul>
  )
}

function PointRow({ point }: { point: SummaryPointView }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const sources = point.citations.length + point.note.length
  return (
    <li className="flex gap-2">
      <span className="mt-2 size-1.5 shrink-0 rounded-full bg-primary/60" />
      <div className="min-w-0 flex-1">
        <span>{point.text}</span>
        {sources > 0 && (
          <button
            type="button"
            onClick={() => setOpen(!open)}
            className="ml-2 text-xs text-muted-foreground underline-offset-2 hover:underline"
            aria-expanded={open}
          >
            {t('summary.sources', { count: sources })}
          </button>
        )}
        {open && (
          <ul className="mt-1 space-y-0.5 border-l-2 pl-2 text-xs text-muted-foreground">
            {point.citations.map((c) => (
              <li key={c.segment_id}>
                <span className="font-mono tabular-nums">{clock(c.start_ms)}</span> „{c.text.trim()}“
              </li>
            ))}
            {point.note.map((key) => (
              <li key={key}>{t('summary.fromNote', { section: t(`summary.noteSection.${key}`, key) })}</li>
            ))}
          </ul>
        )}
      </div>
    </li>
  )
}

/**
 * The session's key points on the session page: made on request from the confirmed transcript
 * and the note (after the redaction preview), editable in place.
 */
export function SummaryCard({ sessionId, patientId, ready }: { sessionId: string; patientId: string; ready: boolean }) {
  const { t } = useTranslation()
  const llm = useQuery(q.llm())
  const summary = useQuery(q.sessionSummary(sessionId))
  const setSummary = useSetSummary(sessionId, patientId)
  const [previewOpen, setPreviewOpen] = useState(false)
  const [editing, setEditing] = useState(false)
  const active = llm.data?.config.active
  const s = summary.data

  return (
    <SectionCard
      help={HELP.note}
      icon={Sparkles}
      title={
        <span className="flex items-center gap-3">
          {t('summary.title')}
          {s && <SummaryStatus summary={s} />}
        </span>
      }
      actions={
        <span className="flex gap-2">
          {s && !editing && (
            <Button size="sm" variant="ghost" onClick={() => setEditing(true)} data-testid="summary-edit-open">
              <Pencil /> {t('common.edit')}
            </Button>
          )}
          <Button
            size="sm"
            variant={s ? 'outline' : 'default'}
            onClick={() => setPreviewOpen(true)}
            disabled={!active || !ready || editing}
            data-testid="summary-run"
          >
            {s ? t('summary.again') : t('summary.make')}
          </Button>
        </span>
      }
      contentClassName="space-y-3 text-sm"
    >
      {!active ? (
        <p className="text-muted-foreground">
          {t('facts.noProvider')}{' '}
          <Link to="/settings" className="underline">
            {t('facts.settingsLink')}
          </Link>
        </p>
      ) : !s ? (
        <p className="text-muted-foreground">{ready ? t('summary.intro') : t('summary.notReady')}</p>
      ) : (
        <SummaryPoints key={`${s.updated_at}-${editing}`} summary={s} patientId={patientId} editing={editing} onDone={() => setEditing(false)} />
      )}
      {previewOpen && (
        <RedactionDialog
          reportId={sessionId}
          patientId={patientId}
          previewKey={['summaryPreview', sessionId]}
          loadPreview={() => api.summaryPreview(sessionId)}
          progressEvent="summary-progress"
          labels={{
            send: t('summary.send'),
            runLocal: t('summary.runLocal'),
            running: t('summary.running'),
            part: (done, total) => t('facts.progress.chunk', { done, total }),
            parts: (count) => t('facts.preview.chunks', { count }),
          }}
          onRun={async (digest, requestId) => {
            const r = await api.summaryRun(sessionId, digest, requestId)
            if (r.cancelled) {
              toast.info(t('summary.cancelled'))
              return
            }
            setSummary(r.summary)
            toast.success(
              t('summary.done', { count: r.summary?.points.length ?? 0 }) +
                (r.dropped ? ` ${t('facts.dropped', { count: r.dropped })}` : ''),
            )
          }}
          onClose={() => setPreviewOpen(false)}
        />
      )}
    </SectionCard>
  )
}
