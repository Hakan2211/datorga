import { useCallback, useEffect, useRef, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { CheckCircle2, Pause, Play } from 'lucide-react'
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
import { cn } from '@/lib/utils'

/**
 * Segment list with timestamps. Drafts are editable; confirming deletes the audio. While the audio
 * exists (`playable`), a timestamp plays its segment and playback runs on through the next ones.
 */
export function TranscriptView({ transcript, sessionId, playable = false }: { transcript: Transcript; sessionId: string; playable?: boolean }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const draft = transcript.status === 'draft'
  const player = useClipPlayer(transcript.source_id, transcript.segments)
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
      {playable && transcript.segments.length > 0 && (
        <div className="flex items-center gap-3 text-sm" data-testid="playback">
          <Button size="sm" variant="outline" onClick={player.toggle} disabled={player.loading}>
            {player.playing ? <Pause /> : <Play />} {player.playing ? t('transcript.pause') : t('transcript.listen')}
          </Button>
          <span className="text-xs text-muted-foreground">
            {player.at !== null
              ? t('transcript.playingAt', { time: formatTimestamp(transcript.segments[player.at].start_ms) })
              : t('transcript.listenHint')}
          </span>
        </div>
      )}
      {transcript.segments.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t('transcript.empty')}</p>
      ) : (
        <ol className="space-y-1.5">
          {transcript.segments.map((s, i) => (
            <SegmentRow
              key={s.idx}
              transcriptId={transcript.id}
              segment={s}
              editable={draft}
              current={player.at === i}
              onPlay={playable ? () => player.playFrom(i) : undefined}
            />
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

/** Fetches each segment's audio when it's played; nothing is cached or written to disk. */
function useClipPlayer(sourceId: string, segments: TranscriptSegment[]) {
  const audio = useRef<HTMLAudioElement | null>(null)
  const url = useRef<string | null>(null)
  const request = useRef(0)
  const [at, setAt] = useState<number | null>(null)
  const [playing, setPlaying] = useState(false)
  const [loading, setLoading] = useState(false)

  const release = () => {
    if (url.current) URL.revokeObjectURL(url.current)
    url.current = null
  }

  const playFrom = useCallback(
    async (i: number) => {
      const seg = segments[i]
      if (!seg) return
      const mine = ++request.current
      setAt(i)
      setLoading(true)
      try {
        // A little past the segment's end, so the last word isn't clipped; the backend clamps.
        const bytes = await api.sourceAudio(sourceId, seg.start_ms, seg.end_ms + CLIP_TAIL_MS)
        if (mine !== request.current) return
        const el = (audio.current ??= new Audio())
        el.onended = () => {
          if (i + 1 < segments.length) void playFrom(i + 1)
          else {
            setPlaying(false)
            setAt(null)
          }
        }
        el.onpause = () => setPlaying(false)
        el.onplay = () => setPlaying(true)
        release()
        url.current = URL.createObjectURL(new Blob([bytes], { type: 'audio/wav' }))
        el.src = url.current
        await el.play()
      } catch (e) {
        if (mine === request.current) {
          toast.error(errorMessage(e))
          setAt(null)
        }
      } finally {
        if (mine === request.current) setLoading(false)
      }
    },
    [segments, sourceId],
  )

  const toggle = () => {
    const el = audio.current
    if (at === null || !el) void playFrom(0)
    else if (playing) el.pause()
    else void el.play()
  }

  useEffect(
    () => () => {
      request.current++
      audio.current?.pause()
      release()
    },
    [],
  )

  return { at, playing, loading, playFrom, toggle }
}

const CLIP_TAIL_MS = 250

function SegmentRow({
  transcriptId,
  segment,
  editable,
  current,
  onPlay,
}: {
  transcriptId: string
  segment: TranscriptSegment
  editable: boolean
  current: boolean
  onPlay?: () => void
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
    <li
      className={cn('grid grid-cols-[4.5rem_1fr] items-start gap-2 rounded-md text-sm', current && 'bg-accent/70')}
      data-playing={current || undefined}
    >
      <span className="pt-1.5 font-mono text-xs tabular-nums text-muted-foreground">
        {onPlay ? (
          <button
            type="button"
            onClick={onPlay}
            className="group inline-flex items-center gap-1 rounded px-1 hover:bg-foreground/5 hover:text-foreground"
            title={t('transcript.playSegment')}
            data-testid="segment-play"
          >
            <Play className={cn('size-3', current ? 'text-primary' : 'opacity-40 group-hover:opacity-100')} />
            {formatTimestamp(segment.start_ms)}
          </button>
        ) : (
          formatTimestamp(segment.start_ms)
        )}
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
