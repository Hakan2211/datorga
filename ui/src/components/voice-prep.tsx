// Voice prep before a session (slice 26): preview of the brief, a GPT-Live conversation over
// WebRTC (Rust creates the session, so the OpenAI key never reaches JS), then a prep sheet.
import { useEffect, useRef, useState } from 'react'
import { Link } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { AlertTriangle, AudioLines, Loader2, Mic, MicOff, PhoneOff, Sparkles } from 'lucide-react'
import type { PrepTurn } from '@/bindings/PrepTurn'
import type { SessionPrep } from '@/bindings/SessionPrep'
import type { TokenView } from '@/bindings/TokenView'
import { formatDate } from '@/components/bits'
import { RichField } from '@/components/rich-field'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { api, errorMessage, q, type PrepPreview } from '@/lib/api'
import { cn } from '@/lib/utils'

const DEFAULT_MIC = 'default'
/** The microphone chosen last time (WebView device id; not the capture panel's cpal id). */
const MIC_KEY = 'thera.prepMic'

/** Input level 0..1 of `stream`, for a meter. */
function useLevel(stream: MediaStream | null) {
  const [level, setLevel] = useState(0)
  useEffect(() => {
    if (!stream || stream.getAudioTracks().length === 0) return
    const ctx = new AudioContext()
    void ctx.resume()
    const analyser = ctx.createAnalyser()
    analyser.fftSize = 512
    ctx.createMediaStreamSource(stream).connect(analyser)
    const buf = new Float32Array(analyser.fftSize)
    let frame = 0
    const tick = () => {
      analyser.getFloatTimeDomainData(buf)
      const rms = Math.sqrt(buf.reduce((a, v) => a + v * v, 0) / buf.length)
      setLevel(Math.min(1, rms * 6))
      frame = requestAnimationFrame(tick)
    }
    tick()
    return () => {
      cancelAnimationFrame(frame)
      void ctx.close()
    }
  }, [stream])
  return level
}

function Meter({ label, level, testId }: { label: string; level: number; testId?: string }) {
  return (
    <div className="flex items-center gap-2 text-xs text-muted-foreground" data-testid={testId} data-level={level.toFixed(2)}>
      <span className="w-20 shrink-0">{label}</span>
      <div className="h-1.5 w-32 overflow-hidden rounded-full bg-muted">
        <div className="h-full bg-primary transition-[width] duration-75" style={{ width: `${Math.round(level * 100)}%` }} />
      </div>
    </div>
  )
}

type Start = { digest: string; tokens: TokenView[]; deviceId: string | null }

/** `primary`: the start button is the page's main action (off when the page has its own). */
export function PrepCard({ sessionId, primary = true }: { sessionId: string; primary?: boolean }) {
  const { t } = useTranslation()
  const prep = useQuery(q.prep(sessionId))
  const live = useQuery(q.live())
  const recording = useQuery(q.recording())
  const [stage, setStage] = useState<'idle' | 'preview' | Start>('idle')
  const enabled = live.data?.config.enabled ?? false
  const problem = live.data?.problem ?? null
  const busyRecording = !!recording.data

  return (
    <Card data-testid="prep-card">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <AudioLines className="size-4" /> {t('prep.title')}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-4 text-sm">
        {!prep.data && <p className="text-muted-foreground">{t('prep.intro')}</p>}
        {prep.data && <PrepResult prep={prep.data} />}
        <div className="flex flex-wrap items-center gap-3">
          <Button
            variant={prep.data || !primary ? 'outline' : 'default'}
            disabled={!enabled || !!problem || busyRecording}
            onClick={() => setStage('preview')}
          >
            <Mic className="size-4" /> {prep.data ? t('prep.again') : t('prep.start')}
          </Button>
          {!enabled && (
            <span className="text-muted-foreground">
              {t('prep.disabled')}{' '}
              <Link to="/settings/sprache" className="underline">
                {t('prep.toSettings')}
              </Link>
            </span>
          )}
          {enabled && problem && (
            <span className="flex items-center gap-1 text-destructive">
              <AlertTriangle className="size-4" /> {problem}
            </span>
          )}
          {enabled && busyRecording && <span className="text-muted-foreground">{t('prep.recording')}</span>}
        </div>
      </CardContent>
      {stage === 'preview' && (
        <PreviewDialog sessionId={sessionId} onClose={() => setStage('idle')} onBegin={(s) => setStage(s)} />
      )}
      {typeof stage === 'object' && (
        <LiveDialog
          sessionId={sessionId}
          start={stage}
          usdPerMinute={live.data?.usd_per_minute ?? 0}
          noBackend={live.data?.no_backend ?? ''}
          onDone={() => setStage('idle')}
        />
      )}
    </Card>
  )
}

function PreviewDialog({ sessionId, onClose, onBegin }: { sessionId: string; onClose: () => void; onBegin: (s: Start) => void }) {
  const { t } = useTranslation()
  const preview = useQuery({ queryKey: ['prep-preview', sessionId], queryFn: () => api.prepPreview(sessionId), gcTime: 0 })
  const [mics, setMics] = useState<MediaDeviceInfo[]>([])
  const [mic, setMic] = useState(() => localStorage.getItem(MIC_KEY) ?? DEFAULT_MIC)
  const [test, setTest] = useState<MediaStream | null>(null)
  const [micError, setMicError] = useState<string | null>(null)
  const level = useLevel(test)
  // Opens the chosen mic for the meter (this also asks for permission, which the device names need).
  useEffect(() => {
    let stream: MediaStream | null = null
    let gone = false
    navigator.mediaDevices
      .getUserMedia({ audio: mic === DEFAULT_MIC ? true : { deviceId: { exact: mic } } })
      .then(async (st) => {
        if (gone) return st.getTracks().forEach((tr) => tr.stop())
        stream = st
        setTest(st)
        setMicError(null)
        const all = await navigator.mediaDevices.enumerateDevices()
        setMics(all.filter((x) => x.kind === 'audioinput' && x.deviceId && x.deviceId !== 'default' && x.deviceId !== 'communications'))
      })
      .catch((e) => {
        setMicError(errorMessage(e))
        // A remembered device that is gone: fall back to the default.
        if (mic !== DEFAULT_MIC) setMic(DEFAULT_MIC)
      })
    return () => {
      gone = true
      stream?.getTracks().forEach((tr) => tr.stop())
      setTest(null)
    }
  }, [mic])
  const data: PrepPreview | undefined = preview.data
  const empty = data && data.stats.notes === 0 && data.stats.facts === 0

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90vh] grid-rows-[auto_minmax(0,1fr)_auto] sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>{t('prep.previewTitle')}</DialogTitle>
          <DialogDescription>{t('prep.previewIntro')}</DialogDescription>
        </DialogHeader>
        <div className="min-h-0 space-y-3 overflow-y-auto text-sm">
          {preview.isError && <p className="text-destructive">{errorMessage(preview.error)}</p>}
          {data && (
            <>
              <p className="text-muted-foreground">
                {t('prep.stats', data.stats)}
                {data.stats.cut > 0 && ` · ${t('prep.statsCut', data.stats)}`}
              </p>
              {empty ? (
                <p className="rounded-md border p-3">{t('prep.empty')}</p>
              ) : (
                <pre className="whitespace-pre-wrap rounded-md border bg-muted/40 p-3 font-sans text-[13px]" data-testid="prep-brief">
                  {data.messages[0]}
                </pre>
              )}
              {data.tokens.length > 0 && (
                <table className="w-full text-[13px]">
                  <tbody>
                    {data.tokens.map((tk) => (
                      <tr key={tk.token} className="border-t">
                        <td className="py-1 pr-3 font-mono">{tk.token}</td>
                        <td className="py-1">{tk.original}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </>
          )}
        </div>
        <div className="space-y-3 border-t pt-3 text-sm">
          <p className="flex gap-2 text-[13px]">
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" /> {t('prep.noNames')}
          </p>
          <div className="flex flex-wrap items-end justify-between gap-3">
            <div className="space-y-1.5">
              <Label>{t('prep.mic')}</Label>
              <Select value={mic} onValueChange={setMic}>
                <SelectTrigger className="w-72">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={DEFAULT_MIC}>{t('prep.micDefault')}</SelectItem>
                  {mics.map((m) => (
                    <SelectItem key={m.deviceId} value={m.deviceId}>
                      {m.label || m.deviceId.slice(0, 8)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Meter label={t('prep.micTest')} level={level} testId="prep-mic-meter" />
              {micError && <p className="text-xs text-destructive">{t('prep.micDenied', { error: micError })}</p>}
            </div>
            <Button
              disabled={!data || !!micError}
              onClick={() => {
                if (!data) return
                localStorage.setItem(MIC_KEY, mic)
                onBegin({ digest: data.digest, tokens: data.tokens, deviceId: mic === DEFAULT_MIC ? null : mic })
              }}
            >
              <Mic className="size-4" /> {t('prep.begin')}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}

/** Puts the originals back for display and storage (on this computer only). */
const restore = (text: string, tokens: TokenView[]) => tokens.reduce((s, tk) => s.split(tk.token).join(tk.original), text)

type Closed = { seconds: number | null; reason: string }

function LiveDialog({
  sessionId,
  start,
  usdPerMinute,
  noBackend,
  onDone,
}: {
  sessionId: string
  start: Start
  usdPerMinute: number
  noBackend: string
  onDone: () => void
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [status, setStatus] = useState<'connecting' | 'live' | 'ending'>('connecting')
  const [turns, setTurns] = useState<PrepTurn[]>([])
  const [seconds, setSeconds] = useState<number | null>(null)
  const [muted, setMuted] = useState(false)
  const [micStream, setMicStream] = useState<MediaStream | null>(null)
  const [remote, setRemote] = useState<MediaStream | null>(null)
  const micLevel = useLevel(micStream)
  const remoteLevel = useLevel(remote)
  const audioEl = useRef<HTMLAudioElement>(null)
  const conn = useRef<{ pc: RTCPeerConnection; dc: RTCDataChannel; mic: MediaStream } | null>(null)
  const turnsRef = useRef<PrepTurn[]>([])
  const secondsRef = useRef<number | null>(null)
  const closedRef = useRef<((c: Closed | null) => void) | null>(null)
  const finishing = useRef(false)
  const scroller = useRef<HTMLDivElement>(null)

  const addDelta = (speaker: PrepTurn['speaker'], delta: string) => {
    const list = turnsRef.current
    const last = list[list.length - 1]
    turnsRef.current = last?.speaker === speaker ? [...list.slice(0, -1), { speaker, text: last.text + delta }] : [...list, { speaker, text: delta }]
    setTurns(turnsRef.current)
  }

  const release = () => {
    const c = conn.current
    if (!c) return
    c.mic.getTracks().forEach((tr) => tr.stop())
    c.dc.close()
    c.pc.close()
    if (audioEl.current) audioEl.current.srcObject = null
    conn.current = null
  }

  /** Stores the conversation (if anything was said) and closes the dialog. */
  const finish = async (closed: Closed | null) => {
    if (finishing.current) return
    finishing.current = true
    release()
    if (!closed) toast.warning(t('prep.usageUnconfirmed'))
    const said = turnsRef.current
      .map((tn) => ({ ...tn, text: restore(tn.text, start.tokens).trim() }))
      .filter((tn) => tn.text)
    const billed = closed?.seconds ?? secondsRef.current
    const reason = closed?.reason ?? 'connection_lost'
    try {
      // Nothing said is still billed: then only the seconds go into the audit log.
      if (said.length > 0) await api.prepFinish(sessionId, said, billed, reason)
      else if (billed != null) await api.prepAbort(sessionId, billed, reason)
    } catch (e) {
      toast.error(errorMessage(e))
    }
    void qc.invalidateQueries({ queryKey: ['prep', sessionId] })
    onDone()
  }

  const end = async () => {
    if (finishing.current || status === 'ending') return
    setStatus('ending')
    const c = conn.current
    const closed = new Promise<Closed | null>((resolve) => {
      closedRef.current = resolve
      setTimeout(() => resolve(null), 15000)
    })
    if (c?.dc.readyState === 'open') c.dc.send(JSON.stringify({ type: 'session.close', event_id: crypto.randomUUID() }))
    else closedRef.current?.(null)
    await finish(await closed)
  }

  useEffect(() => {
    let cancelled = false
    ;(async () => {
      let mic: MediaStream
      try {
        mic = await navigator.mediaDevices.getUserMedia({
          audio: start.deviceId ? { deviceId: { exact: start.deviceId } } : true,
        })
      } catch (e) {
        toast.error(t('prep.micDenied', { error: errorMessage(e) }))
        onDone()
        return
      }
      if (cancelled) return mic.getTracks().forEach((tr) => tr.stop())
      setMicStream(mic)
      const pc = new RTCPeerConnection()
      pc.ontrack = (e) => {
        const stream = e.streams[0] ?? new MediaStream([e.track])
        setRemote(stream)
        const el = audioEl.current
        if (el) {
          el.srcObject = stream
          // Explicit play: autoplay of a stream that arrives later isn't guaranteed.
          el.play().catch((err) => toast.error(t('prep.playFailed', { error: errorMessage(err) })))
        }
      }
      mic.getAudioTracks().forEach((tr) => pc.addTrack(tr, mic))
      const dc = pc.createDataChannel('oai-events')
      conn.current = { pc, dc, mic }
      dc.onmessage = (m) => {
        let ev: Record<string, any>
        try {
          ev = JSON.parse(m.data)
        } catch {
          return
        }
        switch (ev.type) {
          case 'session.started':
            setStatus('live')
            break
          case 'session.input_transcript.delta':
            addDelta('therapist', ev.delta ?? '')
            break
          case 'session.output_transcript.delta':
            addDelta('assistant', ev.delta ?? '')
            break
          case 'session.usage.updated':
            secondsRef.current = ev.usage?.seconds ?? secondsRef.current
            setSeconds(secondsRef.current)
            break
          case 'session.delegation.created':
            // Client delegation: nothing to delegate to, so say so instead of leaving it hanging.
            dc.send(
              JSON.stringify({
                type: 'session.commentary.append',
                event_id: crypto.randomUUID(),
                delegation_id: ev.delegation?.id ?? null,
                content: noBackend,
              }),
            )
            break
          case 'error':
            toast.error(ev.error?.message ?? 'Fehler')
            break
          case 'session.closed': {
            const closed = { seconds: ev.usage?.seconds ?? secondsRef.current, reason: ev.reason ?? 'close_requested' }
            if (closedRef.current) closedRef.current(closed)
            else void finish(closed)
            break
          }
        }
      }
      pc.onconnectionstatechange = () => {
        if (pc.connectionState === 'failed' && !closedRef.current) void finish(null)
      }
      try {
        await pc.setLocalDescription(await pc.createOffer())
        await iceGathered(pc, 10000)
        const answer = await api.prepStart(sessionId, start.digest, pc.localDescription!.sdp)
        if (cancelled) return
        await pc.setRemoteDescription({ type: 'answer', sdp: answer.sdp })
      } catch (e) {
        toast.error(errorMessage(e))
        release()
        onDone()
      }
    })()
    return () => {
      cancelled = true
      release()
    }
    // Runs once per conversation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    scroller.current?.scrollTo({ top: scroller.current.scrollHeight })
  }, [turns])

  const toggleMute = () => {
    const c = conn.current
    if (!c) return
    const next = !muted
    c.mic.getAudioTracks().forEach((tr) => (tr.enabled = !next))
    if (c.dc.readyState === 'open')
      c.dc.send(JSON.stringify({ type: next ? 'session.input_audio.mute' : 'session.input_audio.unmute', event_id: crypto.randomUUID() }))
    setMuted(next)
  }

  const secs = seconds ?? 0
  const time = `${Math.floor(secs / 60)}:${String(Math.floor(secs % 60)).padStart(2, '0')}`

  return (
    <Dialog open onOpenChange={(o) => !o && void end()}>
      <DialogContent
        className="max-h-[90vh] grid-rows-[auto_minmax(0,1fr)_auto] sm:max-w-2xl"
        data-testid="prep-live"
        // A stray click or Escape must not hang up a running (billed) conversation.
        onInteractOutside={(e) => e.preventDefault()}
        onEscapeKeyDown={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {status === 'connecting' ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <span className={cn('size-2.5 rounded-full', status === 'live' ? 'animate-pulse bg-primary' : 'bg-muted-foreground')} />
            )}
            {status === 'connecting' ? t('prep.connecting') : status === 'ending' ? t('prep.ending') : t('prep.live')}
          </DialogTitle>
          <DialogDescription>
            {t('prep.minutes', { time, cost: ((secs / 60) * usdPerMinute).toFixed(2) })} · {t('prep.noNames')}
          </DialogDescription>
        </DialogHeader>
        <audio ref={audioEl} autoPlay className="hidden" />
        <div ref={scroller} className="min-h-48 space-y-3 overflow-y-auto text-sm" data-testid="prep-captions">
          {turns.length === 0 && status === 'live' && <p className="text-muted-foreground">{t('prep.listening')}</p>}
          {turns.map((tn, i) => (
            <div key={i} className={cn('rounded-md p-2', tn.speaker === 'assistant' ? 'bg-muted/50' : 'border')}>
              <p className="text-xs font-medium text-muted-foreground">
                {tn.speaker === 'assistant' ? t('prep.assistant') : t('prep.therapist')}
              </p>
              <p>{restore(tn.text, start.tokens)}</p>
            </div>
          ))}
        </div>
        <div className="flex flex-wrap items-center justify-end gap-2 border-t pt-3">
          <div className="mr-auto space-y-1">
            <Meter label={t('prep.therapist')} level={muted ? 0 : micLevel} testId="prep-meter-mic" />
            <Meter label={t('prep.assistant')} level={remoteLevel} testId="prep-meter-remote" />
          </div>
          <Button variant="outline" disabled={status !== 'live'} onClick={toggleMute}>
            {muted ? <Mic className="size-4" /> : <MicOff className="size-4" />}
            {muted ? t('prep.unmute') : t('prep.mute')}
          </Button>
          <Button variant="outline" className="border-destructive text-destructive" disabled={status === 'ending'} onClick={() => void end()}>
            <PhoneOff className="size-4" /> {t('prep.end')}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}

function iceGathered(pc: RTCPeerConnection, timeoutMs: number) {
  if (pc.iceGatheringState === 'complete') return Promise.resolve()
  return new Promise<void>((resolve) => {
    const done = () => {
      if (pc.iceGatheringState === 'complete') resolve()
    }
    pc.addEventListener('icegatheringstatechange', done)
    setTimeout(resolve, timeoutMs)
  })
}

function PrepResult({ prep }: { prep: SessionPrep }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [requestId, setRequestId] = useState<string | null>(null)
  const [streamed, setStreamed] = useState('')
  // Remounts the editor when a new sheet arrives (not on every saved keystroke).
  const [version, setVersion] = useState(0)
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const run = useMutation({
    mutationFn: async () => {
      const id = crypto.randomUUID()
      setStreamed('')
      setRequestId(id)
      // Listen before starting, so the first pieces aren't missed.
      const off = await api.onLlmDelta(id, (text) => setStreamed((s) => s + text))
      try {
        return await api.prepSheetRun(prep.session_id, id)
      } finally {
        off()
      }
    },
    onSuccess: () => setVersion((v) => v + 1),
    onSettled: () => {
      setRequestId(null)
      void qc.invalidateQueries({ queryKey: ['prep', prep.session_id] })
      void qc.invalidateQueries({ queryKey: ['llm', 'spend'] })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const save = (html: string) => {
    if (saveTimer.current) clearTimeout(saveTimer.current)
    saveTimer.current = setTimeout(() => {
      api.setPrepSheet(prep.session_id, html).catch((e) => toast.error(errorMessage(e)))
    }, 800)
  }

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between gap-2">
        <Label>{t('prep.sheet')}</Label>
        {run.isPending ? (
          <Button variant="ghost" size="sm" onClick={() => requestId && void api.llmCancel(requestId)}>
            {t('prep.cancel')}
          </Button>
        ) : (
          <Button variant={prep.sheet ? 'ghost' : 'default'} size="sm" onClick={() => run.mutate()}>
            <Sparkles className="size-4" /> {prep.sheet ? t('prep.sheetAgain') : t('prep.sheetCreate')}
          </Button>
        )}
      </div>
      {run.isPending ? (
        <div className="space-y-1 rounded-md border p-3">
          <p className="flex items-center gap-2 text-muted-foreground">
            <Loader2 className="size-4 animate-spin" /> {t('prep.sheetRunning')}
          </p>
          {streamed && <pre className="whitespace-pre-wrap font-sans text-[13px] text-muted-foreground">{streamed}</pre>}
        </div>
      ) : prep.sheet ? (
        <div data-testid="prep-sheet">
          <RichField key={version} value={prep.sheet} onChange={save} />
        </div>
      ) : (
        <p className="text-muted-foreground">{t('prep.sheetHint')}</p>
      )}
      <details className="rounded-md border p-2">
        <summary className="cursor-pointer text-muted-foreground">
          {t('prep.conversation', { count: prep.transcript.length })} · {formatDate(prep.created_at, true)}
        </summary>
        <div className="mt-2 space-y-2">
          {prep.transcript.map((tn, i) => (
            <p key={i}>
              <span className="font-medium">{tn.speaker === 'assistant' ? t('prep.assistant') : t('prep.therapist')}:</span> {tn.text}
            </p>
          ))}
        </div>
      </details>
    </div>
  )
}
