// Live dictation (slice 24): one microphone, one dictation at a time, typed into whichever editor
// started it. The backend cuts the speech at pauses and sends each piece back as text
// (`dictation:piece`); this module routes the pieces to their editor and polls the level meter.
import { useSyncExternalStore } from 'react'
import { toast } from 'sonner'
import type { SttTarget } from '@/bindings/SttTarget'
import { api, errorMessage, type DictationStatus } from '@/lib/api'

type Target = { insert: (text: string) => void }

export type DictationState = {
  /** The editor dictating right now (its target id), or null. */
  targetId: string | null
  status: DictationStatus | null
  starting: boolean
  /** Targets whose dictation was stopped but whose last pieces are still being transcribed. */
  finishing: readonly string[]
}

let state: DictationState = { targetId: null, status: null, starting: false, finishing: [] }
const listeners = new Set<() => void>()
const targets = new Map<string, Target>()
/** Dictation id → target id, also after stopping (late pieces still go to their editor). */
const owners = new Map<string, string>()
let poll: ReturnType<typeof setInterval> | null = null
let subscribed: Promise<void> | null = null

function set(next: Partial<DictationState>) {
  state = { ...state, ...next }
  for (const l of listeners) l()
}

function ensureEvents() {
  subscribed ??= Promise.all([
    api.onDictationPiece((p) => {
      const owner = owners.get(p.dictation_id)
      if (owner && p.text) targets.get(owner)?.insert(p.text)
    }),
    api.onDictationDone((d) => {
      const owner = owners.get(d.dictation_id)
      owners.delete(d.dictation_id)
      if (owner) set({ finishing: state.finishing.filter((t) => t !== owner) })
      if (d.error) toast.error(d.error)
    }),
  ]).then(() => undefined)
  return subscribed
}

function startPolling() {
  if (poll) return
  poll = setInterval(() => {
    api
      .dictationStatus()
      .then((status) => {
        if (!status && !state.starting) {
          // Ended elsewhere (stopped from another editor, or the app's state was reset).
          stopPolling()
          set({ targetId: null, status: null })
          return
        }
        set({ status })
      })
      .catch(() => {})
  }, 120)
}

function stopPolling() {
  if (poll) clearInterval(poll)
  poll = null
}

/** Registers an editor that can receive dictated text. Returns the cleanup. */
export function registerTarget(id: string, insert: (text: string) => void): () => void {
  targets.set(id, { insert })
  return () => {
    targets.delete(id)
    // An editor going away stops its own dictation.
    if (state.targetId === id) void stopDictation()
  }
}

/** `reportId`: the report dictated into; `null` for the assistant's question box. */
export async function startDictation(targetId: string, reportId: string | null, deviceId: string | null, engine: SttTarget | null = null) {
  if (state.targetId) await stopDictation()
  await ensureEvents()
  set({ starting: true, targetId })
  try {
    // One left over from before a reload has no editor any more: stop it.
    const running = await api.dictationStatus()
    if (running && !owners.has(running.id)) await api.dictationStop(true)
    const started = await api.dictationStart(reportId, engine, deviceId)
    owners.set(started.id, targetId)
    startPolling()
  } catch (e) {
    set({ targetId: null, status: null })
    toast.error(errorMessage(e))
  } finally {
    set({ starting: false })
  }
}

/** Stops the microphone; what was said last is still transcribed and typed in. */
export async function stopDictation(discard = false) {
  const targetId = state.targetId
  if (!targetId) return
  stopPolling()
  const pending = !discard && !!state.status
  set({ targetId: null, status: null, finishing: pending ? [...state.finishing, targetId] : state.finishing })
  try {
    await api.dictationStop(discard)
  } catch (e) {
    toast.error(errorMessage(e))
  }
}

/** Stops `targetId`'s dictation and waits (up to 20 s) until its last piece is typed in. */
export async function stopAndSettle(targetId: string) {
  if (state.targetId === targetId) await stopDictation()
  const until = Date.now() + 20_000
  while (state.finishing.includes(targetId) && Date.now() < until) {
    await new Promise((r) => setTimeout(r, 100))
  }
}

export function useDictation(): DictationState {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => listeners.delete(l)
    },
    () => state,
  )
}

/** What a target shows: listening, recognizing (grey placeholder), or loading the model. */
export function targetPhase(s: DictationState, id: string): 'off' | 'starting' | 'loading' | 'listening' | 'recognizing' {
  if (s.targetId !== id) return s.finishing.includes(id) ? 'recognizing' : 'off'
  if (s.starting || !s.status) return 'starting'
  if (s.status.loading) return 'loading'
  if (s.status.speaking || s.status.pending > 0) return 'recognizing'
  return 'listening'
}
