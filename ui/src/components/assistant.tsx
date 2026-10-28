// The assistant (slice 25): a chat that reads through tools and only proposes. Each proposal is a
// card she applies or discards; "Was ich getan habe" lists the tool calls of an answer. Opened
// from a patient or session page, the thread is limited to that patient.
import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from 'react'
import { Link, useMatch } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import {
  ArrowUp,
  CalendarClock,
  CalendarPlus,
  Check,
  ChevronRight,
  FileText,
  ListTodo,
  Loader2,
  Mic,
  MicOff,
  NotebookPen,
  Plus,
  ShieldCheck,
  Square,
  TriangleAlert,
  Volume2,
  X,
} from 'lucide-react'
import type { AgentProposal } from '@/bindings/AgentProposal'
import type { ChatMessage } from '@/bindings/ChatMessage'
import type { ChatScope } from '@/bindings/ChatScope'
import type { ProposalKind } from '@/bindings/ProposalKind'
import { StatusPill } from '@/components/bits'
import { invalidateSessions } from '@/components/session-dialog'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage, q } from '@/lib/api'
import { currentZone, groupTurns, proposalTarget, type Turn } from '@/lib/assistant'
import { useCaptureDevice } from '@/lib/capture'
import { registerTarget, startDictation, stopDictation, targetPhase, useDictation } from '@/lib/dictation'
import { cn } from '@/lib/utils'

const KIND_ICON: Record<ProposalKind, typeof CalendarPlus> = {
  session: CalendarPlus,
  reschedule: CalendarClock,
  task: ListTodo,
  section_text: FileText,
  note_text: FileText,
  day_plan: NotebookPen,
}

/** What an applied proposal may have changed. */
function invalidateApplied(qc: QueryClient) {
  invalidateSessions(qc)
  for (const key of ['tasks', 'dayNote', 'report', 'reports', 'chat']) void qc.invalidateQueries({ queryKey: [key] })
}

type Pending = { requestId: string; question: string; text: string; stored: boolean }

/** A thread (`threadId`), or a new one created in `scope` on the first question. */
export function AssistantChat({
  threadId,
  scope,
  refId,
  onThread,
  compact = false,
}: {
  threadId: string | null
  scope: ChatScope
  refId?: string
  onThread: (id: string) => void
  compact?: boolean
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const view = useQuery({ ...q.chat(threadId ?? ''), enabled: !!threadId })
  const llm = useQuery(q.llm())
  const active = llm.data?.config.active ?? null
  const tools = useQuery({ ...q.agentTools(), enabled: !!active })
  const [input, setInput] = useState('')
  const [pending, setPending] = useState<Pending | null>(null)
  const scroller = useRef<HTMLDivElement>(null)

  const turns = useMemo(
    () => (view.data ? groupTurns(view.data.messages, view.data.steps, view.data.proposals) : []),
    [view.data],
  )
  useEffect(() => {
    scroller.current?.scrollTo({ top: scroller.current.scrollHeight })
  }, [view.data?.messages.length, view.data?.proposals.length, pending?.text, pending?.stored])

  const send = async (question = input) => {
    const text = question.trim()
    if (!text || pending) return
    let id = threadId
    try {
      if (!id) {
        id = (await api.createChatThread(scope, refId ?? null)).id
        onThread(id)
      }
    } catch (e) {
      toast.error(errorMessage(e))
      return
    }
    const requestId = crypto.randomUUID()
    const thread = id
    setPending({ requestId, question: text, text: '', stored: false })
    setInput('')
    let stored = false
    const unlisten = await Promise.all([
      api.onAgentText(requestId, (shown) => setPending((p) => (p?.requestId === requestId ? { ...p, text: shown } : p))),
      api.onAgentChanged(thread, () => {
        stored = true
        void qc.invalidateQueries({ queryKey: ['chat', thread] }).then(() =>
          setPending((p) => (p?.requestId === requestId ? { ...p, text: '', stored: true } : p)),
        )
      }),
    ])
    try {
      const end = await api.chatSend(thread, text, currentZone(), requestId)
      if (end === 'cancelled') toast(t('assistant.cancelled'))
    } catch (e) {
      toast.error(errorMessage(e))
      // Refused before it was stored (no model chosen, a typed placeholder): give the text back.
      if (!stored) setInput(text)
    } finally {
      for (const u of unlisten) u()
      await qc.invalidateQueries({ queryKey: ['chat'] })
      setPending(null)
    }
  }

  const empty = turns.length === 0 && !pending
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="assistant-chat">
      {!llm.isLoading && !active ? (
        <Notice>
          {t('assistant.noModel')}{' '}
          <Link to="/settings" className="underline">
            {t('assistant.toSettings')}
          </Link>
        </Notice>
      ) : tools.data === false ? (
        <Notice>{t('assistant.noTools')}</Notice>
      ) : null}
      <div ref={scroller} className={cn('min-h-0 flex-1 space-y-5 overflow-y-auto', compact ? 'p-4' : 'px-8 py-6')}>
        {empty && <Intro scope={scope} onPick={(text) => void send(text)} />}
        {turns.map((turn, i) => (
          <TurnView key={turn.question?.id ?? i} turn={turn} threadId={threadId ?? ''} />
        ))}
        {pending && (
          <>
            {!pending.stored && <Question text={pending.question} />}
            <div className="flex items-start gap-2 text-sm text-muted-foreground" data-testid="assistant-thinking">
              <Loader2 className="mt-0.5 size-4 shrink-0 animate-spin" />
              <span className="whitespace-pre-wrap">{pending.text || t('assistant.thinking')}</span>
            </div>
          </>
        )}
      </div>
      <Composer
        value={input}
        onChange={setInput}
        onSend={() => void send()}
        running={!!pending}
        onCancel={() => pending && void api.llmCancel(pending.requestId)}
        compact={compact}
      />
    </div>
  )
}

function Notice({ children }: { children: ReactNode }) {
  return (
    <div className="flex items-start gap-2 border-b bg-warning/10 px-4 py-2 text-xs text-foreground" data-testid="assistant-notice">
      <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
      <span>{children}</span>
    </div>
  )
}

function Intro({ scope, onPick }: { scope: ChatScope; onPick: (text: string) => void }) {
  const { t } = useTranslation()
  const examples = t(`assistant.examples.${scope}`, { returnObjects: true }) as string[]
  return (
    <div className="space-y-3 text-sm">
      <p className="text-muted-foreground">{t('assistant.intro')}</p>
      <div className="flex flex-wrap gap-2">
        {examples.map((e) => (
          <button
            key={e}
            type="button"
            onClick={() => onPick(e)}
            className="rounded-full border bg-card px-3 py-1 text-xs hover:bg-foreground/5"
          >
            {e}
          </button>
        ))}
      </div>
      <p className="flex items-start gap-1.5 text-xs text-muted-foreground">
        <ShieldCheck className="mt-0.5 size-3.5 shrink-0 text-primary" /> {t('assistant.privacy')}
      </p>
    </div>
  )
}

function Question({ text }: { text: string }) {
  return (
    <div className="flex justify-end">
      <div className="max-w-[85%] rounded-2xl rounded-br-sm bg-primary/10 px-3.5 py-2 text-sm whitespace-pre-wrap" data-testid="assistant-question">
        {text}
      </div>
    </div>
  )
}

function TurnView({ turn, threadId }: { turn: Turn; threadId: string }) {
  const { t } = useTranslation()
  return (
    <div className="space-y-3">
      {turn.question && <Question text={turn.question.content} />}
      {turn.steps.length > 0 && (
        <details className="group text-xs text-muted-foreground" data-testid="assistant-steps">
          <summary className="flex cursor-pointer list-none items-center gap-1 select-none hover:text-foreground">
            <ChevronRight className="size-3.5 transition-transform group-open:rotate-90" />
            {t('assistant.steps', { count: turn.steps.length })}
          </summary>
          <ul className="mt-1.5 space-y-1 border-l pl-4">
            {turn.steps.map((s) => (
              <li key={s.id} className={cn('flex items-center gap-1.5', s.failed && 'text-destructive')}>
                {s.failed ? <X className="size-3" /> : <Check className="size-3" />}
                {s.label}
              </li>
            ))}
          </ul>
        </details>
      )}
      {turn.proposals.map((p) => (
        <ProposalCard key={p.id} proposal={p} hint={turn.hints[p.id]} />
      ))}
      {turn.answers.map((a) => (
        <Answer key={a.id} message={a} threadId={threadId} />
      ))}
    </div>
  )
}

function Answer({ message, threadId }: { message: ChatMessage; threadId: string }) {
  const { t } = useTranslation()
  const tts = useQuery(q.tts())
  const audio = useRef<HTMLAudioElement | null>(null)
  const speak = useMutation({
    mutationFn: () => api.chatSpeak(threadId, message.id),
    onSuccess: (buf) => {
      audio.current?.pause()
      const url = URL.createObjectURL(new Blob([buf], { type: 'audio/mpeg' }))
      const el = new Audio(url)
      el.onended = () => URL.revokeObjectURL(url)
      audio.current = el
      el.play().catch((e) => toast.error(errorMessage(e)))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  useEffect(() => () => audio.current?.pause(), [])
  return (
    <div className="group/answer text-sm leading-relaxed whitespace-pre-wrap" data-testid="assistant-answer">
      {message.content}
      {tts.data?.config.enabled && !tts.data.problem && (
        <button
          type="button"
          onClick={() => speak.mutate()}
          title={t('assistant.readAloud')}
          aria-label={t('assistant.readAloud')}
          className="ml-1.5 inline-flex translate-y-0.5 text-muted-foreground opacity-0 group-hover/answer:opacity-100 hover:text-foreground"
        >
          {speak.isPending ? <Loader2 className="size-3.5 animate-spin" /> : <Volume2 className="size-3.5" />}
        </button>
      )}
    </div>
  )
}

function ProposalCard({ proposal: p, hint }: { proposal: AgentProposal; hint?: string }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const decide = useMutation({
    mutationFn: (apply: boolean) => (apply ? api.applyProposal(p.id) : api.discardProposal(p.id)),
    onSuccess: (done) => {
      invalidateApplied(qc)
      if (done.status === 'applied') toast.success(t('assistant.applied'))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const Icon = KIND_ICON[p.kind]
  const text = (p.payload as { text?: string } | null)?.text
  const target = proposalTarget(p)
  return (
    <div className="rounded-lg border bg-card p-3 shadow-xs" data-testid="proposal-card" data-status={p.status} data-kind={p.kind}>
      <div className="flex items-start gap-2.5">
        <span className="flex size-7 shrink-0 items-center justify-center rounded-md bg-accent text-accent-foreground">
          <Icon className="size-4" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-xs text-muted-foreground">{t(`assistant.kind.${p.kind}`)}</div>
          <div className="text-sm font-medium">{p.summary}</div>
        </div>
      </div>
      {hint && (
        <p className="mt-2 flex items-start gap-1.5 rounded bg-warning/10 px-2 py-1 text-xs">
          <TriangleAlert className="mt-0.5 size-3 shrink-0" /> {hint}
        </p>
      )}
      {text && <p className="mt-2 max-h-48 overflow-y-auto rounded bg-muted/50 p-2 text-sm whitespace-pre-wrap">{text}</p>}
      <div className="mt-3 flex items-center gap-2">
        {p.status === 'open' ? (
          <>
            <Button size="sm" onClick={() => decide.mutate(true)} disabled={decide.isPending} data-testid="proposal-apply">
              {decide.isPending && decide.variables ? <Loader2 className="animate-spin" /> : <Check />}
              {t('assistant.apply')}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => decide.mutate(false)} disabled={decide.isPending} data-testid="proposal-discard">
              {t('assistant.discard')}
            </Button>
          </>
        ) : (
          <>
            <StatusPill tone={p.status === 'applied' ? 'success' : 'neutral'}>{t(`assistant.status.${p.status}`)}</StatusPill>
            {target?.to === '/sessions/$sessionId' && (
              <Link to={target.to} params={{ sessionId: target.id }} className="text-xs underline">
                {t('assistant.open')}
              </Link>
            )}
            {target?.to === '/reports/$reportId' && (
              <Link to={target.to} params={{ reportId: target.id }} className="text-xs underline">
                {t('assistant.open')}
              </Link>
            )}
            {target?.to === '/' && (
              <Link to="/" className="text-xs underline">
                {t('assistant.toToday')}
              </Link>
            )}
          </>
        )}
      </div>
    </div>
  )
}

/** The question box: Enter sends, Shift+Enter breaks the line; the mic dictates into it. */
function Composer({
  value,
  onChange,
  onSend,
  running,
  onCancel,
  compact,
}: {
  value: string
  onChange: (v: string | ((v: string) => string)) => void
  onSend: () => void
  running: boolean
  onCancel: () => void
  compact: boolean
}) {
  const { t } = useTranslation()
  const id = useId()
  const d = useDictation()
  const { startId } = useCaptureDevice()
  const phase = targetPhase(d, id)
  const on = d.targetId === id
  useEffect(
    () => registerTarget(id, (text) => onChange((v) => (v.trim() ? `${v.trimEnd()} ${text}` : text))),
    [id, onChange],
  )
  return (
    <div className={cn('border-t bg-card', compact ? 'p-3' : 'px-8 py-4')}>
      <div className="flex items-end gap-2 rounded-lg border bg-background p-1.5 shadow-xs focus-within:ring-2 focus-within:ring-ring/40">
        <Textarea
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault()
              onSend()
            }
          }}
          placeholder={phase === 'off' ? t('assistant.placeholder') : t(`dictation.phase.${phase}`)}
          rows={2}
          className="max-h-40 min-h-10 flex-1 resize-none border-0 bg-transparent shadow-none focus-visible:ring-0"
          data-testid="assistant-input"
        />
        <Button
          type="button"
          size="icon"
          variant="ghost"
          className={cn('size-8 shrink-0', on && 'bg-destructive/10 text-destructive hover:bg-destructive/15')}
          onClick={() => void (on ? stopDictation() : startDictation(id, null, startId))}
          title={on ? t('dictation.stop') : t('dictation.start')}
          aria-label={on ? t('dictation.stop') : t('dictation.start')}
          aria-pressed={on}
        >
          {phase === 'starting' || phase === 'loading' ? <Loader2 className="animate-spin" /> : on ? <MicOff /> : <Mic />}
        </Button>
        {running ? (
          <Button type="button" size="icon" variant="outline" className="size-8 shrink-0" onClick={onCancel} title={t('assistant.stop')} aria-label={t('assistant.stop')} data-testid="assistant-stop">
            <Square className="size-3.5" />
          </Button>
        ) : (
          <Button type="button" size="icon" className="size-8 shrink-0" onClick={onSend} disabled={!value.trim()} aria-label={t('assistant.send')} data-testid="assistant-send">
            <ArrowUp />
          </Button>
        )}
      </div>
    </div>
  )
}

/** The side panel: limited to the patient or session of the current page, else across the app. */
export function AssistantPanel() {
  const patientId = useMatch({ from: '/patients/$patientId', shouldThrow: false })?.params.patientId
  const sessionId = useMatch({ from: '/sessions/$sessionId', shouldThrow: false })?.params.sessionId
  const scope: ChatScope = sessionId ? 'session' : patientId ? 'patient' : 'global'
  const refId = sessionId ?? patientId
  return <ScopedChat key={`${scope}:${refId ?? ''}`} scope={scope} refId={refId} />
}

function ScopedChat({ scope, refId }: { scope: ChatScope; refId?: string }) {
  const { t } = useTranslation()
  const threads = useQuery(q.chatThreads(scope, refId))
  // `undefined`: the newest thread of this scope; `null`: a new one.
  const [chosen, setChosen] = useState<string | null | undefined>(undefined)
  const threadId = chosen === undefined ? (threads.data?.[0]?.id ?? null) : chosen
  const current = threads.data?.find((th) => th.id === threadId)
  const label = scope === 'global' ? t('assistant.scope.global') : t(`assistant.scope.${scope}`, { name: current?.patient_name ?? '…' })
  if (threads.isLoading) return null
  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between gap-2 border-b px-4 py-2 text-xs text-muted-foreground">
        <span className="truncate" data-testid="assistant-scope">
          {scope === 'global' || current ? label : t(`assistant.scope.${scope}New`)}
        </span>
        <div className="flex shrink-0 items-center gap-1">
          <Button variant="ghost" size="sm" className="h-7 px-2 text-xs" onClick={() => setChosen(null)} disabled={threadId === null}>
            <Plus /> {t('assistant.new')}
          </Button>
          <Button asChild variant="ghost" size="sm" className="h-7 px-2 text-xs">
            <Link to="/assistant" search={{ thread: threadId ?? undefined }}>
              {t('assistant.all')}
            </Link>
          </Button>
        </div>
      </div>
      <div className="min-h-0 flex-1">
        <AssistantChat threadId={threadId} scope={scope} refId={refId} onThread={setChosen} compact />
      </div>
    </div>
  )
}
