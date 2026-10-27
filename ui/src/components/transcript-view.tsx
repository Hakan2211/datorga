import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { CheckCircle2 } from 'lucide-react'
import type { Transcript } from '@/bindings/Transcript'
import type { TranscriptSegment } from '@/bindings/TranscriptSegment'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage } from '@/lib/api'

/** Segment list with timestamps. Drafts are editable; confirming deletes the audio. */
export function TranscriptView({ transcript, sessionId }: { transcript: Transcript; sessionId: string }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const draft = transcript.status === 'draft'
  const confirm = useMutation({
    mutationFn: () => api.confirmTranscript(transcript.id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['session', sessionId] })
      toast.success(t('transcript.confirmed'))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <div className="space-y-3 rounded-md border bg-muted/30 p-3">
      {draft && <p className="text-sm text-muted-foreground">{t('transcript.reviewNote')}</p>}
      {transcript.segments.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t('transcript.empty')}</p>
      ) : (
        <ol className="space-y-1.5">
          {transcript.segments.map((s) => (
            <SegmentRow key={s.idx} transcriptId={transcript.id} segment={s} editable={draft} />
          ))}
        </ol>
      )}
      <div className="flex items-center gap-3 text-xs text-muted-foreground">
        <span>
          {transcript.engine} · {transcript.model}
        </span>
        {draft && (
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button size="sm" className="ml-auto" disabled={confirm.isPending}>
                <CheckCircle2 /> {t('transcript.confirm')}
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>{t('transcript.confirmTitle')}</AlertDialogTitle>
                <AlertDialogDescription>{t('transcript.confirmBody')}</AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
                <AlertDialogAction onClick={() => confirm.mutate()}>{t('transcript.confirmAction')}</AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        )}
      </div>
    </div>
  )
}

function SegmentRow({
  transcriptId,
  segment,
  editable,
}: {
  transcriptId: string
  segment: TranscriptSegment
  editable: boolean
}) {
  const { t } = useTranslation()
  const [text, setText] = useState(segment.text)
  const [saved, setSaved] = useState(segment.text)
  const [edited, setEdited] = useState(segment.edited)
  // Saved on blur. Local state mirrors the server's text and edited flag without a refetch.
  const save = useMutation({
    mutationFn: (value: string) => api.editSegment(transcriptId, segment.idx, value),
    onSuccess: (_, value) => {
      setSaved(value)
      setEdited(true)
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <li className="grid grid-cols-[4.5rem_1fr] items-start gap-2 text-sm">
      <span className="pt-1.5 font-mono text-xs tabular-nums text-muted-foreground">
        {formatTimestamp(segment.start_ms)}
        {edited && <span className="block font-sans">{t('transcript.edited')}</span>}
      </span>
      {editable ? (
        <Textarea
          value={text}
          rows={Math.max(1, Math.ceil(text.length / 110))}
          className="min-h-0 resize-none bg-background py-1"
          onChange={(e) => setText(e.target.value)}
          onBlur={() => {
            if (text.trim() !== saved) save.mutate(text.trim())
          }}
        />
      ) : (
        <p className="py-1">{text}</p>
      )}
    </li>
  )
}

export function formatTimestamp(ms: number): string {
  const s = Math.floor(ms / 1000)
  const h = Math.floor(s / 3600)
  const mm = String(Math.floor((s % 3600) / 60)).padStart(2, '0')
  const ss = String(s % 60).padStart(2, '0')
  return h ? `${h}:${mm}:${ss}` : `${mm}:${ss}`
}
