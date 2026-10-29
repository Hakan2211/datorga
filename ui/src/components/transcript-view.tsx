import { useCallback, useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { ArrowLeftRight, CheckCircle2, ChevronDown, Pause, Play, Users, UserRound } from 'lucide-react'
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage } from '@/lib/api'
import { speakerLabels, THERAPIST, PATIENT } from '@/lib/speakers'
import { cn } from '@/lib/utils'

/**
 * Segment list with timestamps. Drafts are editable; confirming deletes the audio. While the audio
 * exists (`playable`), a timestamp plays its segment and playback runs on through the next ones.
 * A session recording with separated speakers shows who speaks per segment (slice 27). `separation`
 * is given for a session recording whose audio still exists: she can then separate its speakers
 * (again) as 2–4 people. `busy`: a job runs on the recording, so it can't be confirmed yet.
 */
export function TranscriptView({
  transcript,
  sessionId,
  playable = false,
  separation,
  busy = false,
}: {
  transcript: Transcript
  sessionId: string
  playable?: boolean
  separation?: { speakers: number }
  busy?: boolean
}) {
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
  const speakers = transcript.diarization ? speakerLabels(transcript.segments) : []

  return (
    <div className="space-y-3 rounded-md border bg-muted/30 p-3">
      {draft && <p className="text-sm text-muted-foreground">{t('transcript.reviewNote')}</p>}
      {(speakers.length > 0 || (draft && separation)) && (
        <SpeakerBar
          transcript={transcript}
          sessionId={sessionId}
          labels={speakers}
          editable={draft}
          separation={draft && !busy ? separation : undefined}
        />
      )}
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
              sessionId={sessionId}
              segment={s}
              speakers={transcript.diarization ? speakers : null}
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
              <Button size="sm" className="ml-auto" disabled={confirm.isPending || busy}>
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

/** Who is who in this recording, and the fixes that apply to all its segments. */
function SpeakerBar({
  transcript,
  sessionId,
  labels,
  editable,
  separation,
}: {
  transcript: Transcript
  sessionId: string
  labels: string[]
  editable: boolean
  separation?: { speakers: number }
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const done = () => void qc.invalidateQueries({ queryKey: ['session', sessionId] })
  const engine = useQuery({ queryKey: ['diarize'], queryFn: api.diarize, enabled: !!separation }).data?.engine
  const [count, setCount] = useState(separation?.speakers ?? 2)
  const separate = useMutation({
    mutationFn: () => api.rediarize(transcript.source_id, count),
    onSuccess: done,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const swap = useMutation({
    mutationFn: () => api.swapSpeakers(transcript.id),
    onSuccess: done,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const assign = useMutation({
    mutationFn: (label: string) => api.assignTherapist(transcript.id, label),
    onSuccess: () => {
      done()
      void qc.invalidateQueries({ queryKey: ['diarize'] })
      toast.success(t('transcript.speakers.learned'))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const named = labels.includes(THERAPIST)
  const unnamed = labels.filter((l) => l !== THERAPIST && l !== PATIENT)
  const canSeparate = !!separation && !!engine
  if (labels.length === 0 && !canSeparate) return null

  return (
    <div className="space-y-2 rounded-md border bg-background p-2.5 text-sm" data-testid="speaker-bar">
      {labels.length > 0 && (
        <p className="flex items-start gap-2 text-muted-foreground">
          <UserRound className="mt-0.5 size-4 shrink-0" />
          {named ? t('transcript.speakers.recognized') : t('transcript.speakers.unrecognized')}
        </p>
      )}
      {editable && (
        <div className="flex flex-wrap gap-2">
          {unnamed.map((l) => (
            <Button key={l} size="sm" variant={named ? 'outline' : 'default'} disabled={assign.isPending} onClick={() => assign.mutate(l)}>
              {t('transcript.speakers.isMe', { name: speakerName(t, l) })}
            </Button>
          ))}
          {/* Therapist ↔ patient only makes sense with two speakers. */}
          {named && labels.length === 2 && (
            <Button size="sm" variant="outline" disabled={swap.isPending} onClick={() => swap.mutate()}>
              <ArrowLeftRight /> {t('transcript.speakers.swap')}
            </Button>
          )}
        </div>
      )}
      {canSeparate && (
        <div className="flex flex-wrap items-center gap-2" data-testid="speaker-count">
          <Users className="size-4 text-muted-foreground" />
          <span className="text-muted-foreground">{t('transcript.speakers.count')}</span>
          <span className="inline-flex rounded-md border p-0.5" role="radiogroup" aria-label={t('transcript.speakers.count')}>
            {[2, 3, 4].map((n) => (
              <button
                key={n}
                type="button"
                role="radio"
                aria-checked={count === n}
                onClick={() => setCount(n)}
                className={cn(
                  'rounded px-2 py-0.5 text-xs tabular-nums',
                  count === n ? 'bg-primary text-primary-foreground' : 'text-muted-foreground hover:bg-muted',
                )}
              >
                {n}
              </button>
            ))}
          </span>
          <Button
            size="sm"
            variant="outline"
            disabled={separate.isPending || (labels.length > 0 && count === separation.speakers)}
            onClick={() => separate.mutate()}
            title={t('transcript.speakers.countHint')}
          >
            {labels.length > 0 ? t('transcript.speakers.reseparate') : t('transcript.speakers.separate')}
          </Button>
        </div>
      )}
    </div>
  )
}

function speakerName(t: (key: string, o?: Record<string, unknown>) => string, label: string): string {
  if (label === THERAPIST) return t('transcript.speakers.therapist')
  if (label === PATIENT) return t('transcript.speakers.patient')
  return t('transcript.speakers.speaker', { n: label.replace('speaker_', '') })
}

/** The segment's speaker; a draft's chip opens a menu to correct this one segment. */
function SpeakerChip({
  transcriptId,
  sessionId,
  segment,
  labels,
  editable,
}: {
  transcriptId: string
  sessionId: string
  segment: TranscriptSegment
  labels: string[]
  editable: boolean
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const set = useMutation({
    mutationFn: (label: string) => api.setSegmentSpeaker(transcriptId, segment.idx, label),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['session', sessionId] }),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const label = segment.speaker
  const style = cn(
    'inline-flex h-6 max-w-full items-center gap-1 truncate rounded-full px-2 text-xs font-medium',
    label === THERAPIST && 'bg-primary/10 text-primary',
    label === PATIENT && 'bg-amber-500/15 text-amber-800 dark:text-amber-300',
    label !== THERAPIST && label !== PATIENT && 'border border-dashed text-muted-foreground',
  )
  const text = label ? speakerName(t, label) : t('transcript.speakers.none')
  if (!editable) return <span className={style} data-speaker={label ?? ''}>{text}</span>
  const choices = [THERAPIST, PATIENT, ...labels.filter((l) => l !== THERAPIST && l !== PATIENT)]
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button type="button" className={cn(style, 'hover:opacity-80')} data-speaker={label ?? ''} title={t('transcript.speakers.change')}>
          {text}
          {segment.speaker_edited && <span aria-hidden>·</span>}
          <ChevronDown className="size-3 opacity-60" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        {choices.map((l) => (
          <DropdownMenuItem key={l} disabled={l === label} onSelect={() => set.mutate(l)}>
            {speakerName(t, l)}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function SegmentRow({
  transcriptId,
  sessionId,
  segment,
  speakers,
  editable,
  current,
  onPlay,
}: {
  transcriptId: string
  sessionId: string
  segment: TranscriptSegment
  /** The recording's speaker labels; null when speakers weren't separated. */
  speakers: string[] | null
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
      className={cn(
        'grid items-start gap-2 rounded-md text-sm',
        speakers ? 'grid-cols-[4.5rem_7.5rem_1fr]' : 'grid-cols-[4.5rem_1fr]',
        current && 'bg-accent/70',
      )}
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
      {speakers && (
        <span className="pt-1">
          <SpeakerChip transcriptId={transcriptId} sessionId={sessionId} segment={segment} labels={speakers} editable={editable} />
        </span>
      )}
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
