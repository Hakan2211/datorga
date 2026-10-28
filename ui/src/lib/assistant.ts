// The assistant (slice 25): pure helpers for the chat. The backend has no time zone database, so
// the WebView describes its zone (offset plus DST changes around now); messages are grouped into
// turns for display.
import type { AgentProposal } from '@/bindings/AgentProposal'
import type { ChatMessage } from '@/bindings/ChatMessage'
import type { Zone } from '@/bindings/Zone'

const HOUR = 3_600_000
const DAY = 24 * HOUR
const SPAN_DAYS = 400

/** The local zone around `now`: offset (minutes east of UTC) before the first change, and every
 *  change within ±400 days, to the hour (DST switches on the hour in UTC). */
export function currentZone(now = new Date(), offsetAt = (ms: number) => -new Date(ms).getTimezoneOffset()): Zone {
  const start = Math.floor((now.getTime() - SPAN_DAYS * DAY) / HOUR) * HOUR
  const end = now.getTime() + SPAN_DAYS * DAY
  const base = offsetAt(start)
  const changes: Zone['changes'] = []
  let prev = base
  for (let day = start + DAY; day <= end; day += DAY) {
    const offset = offsetAt(day)
    if (offset === prev) continue
    let at = day - DAY
    while (offsetAt(at) === prev) at += HOUR
    changes.push({ at: new Date(at).toISOString().replace(/\.\d{3}Z$/, 'Z'), offset })
    prev = offset
  }
  return { base, changes }
}

export type Step = { id: string; label: string; failed: boolean }

/** One question and what followed it. */
export type Turn = {
  question: ChatMessage | null
  steps: Step[]
  proposals: AgentProposal[]
  /** Assistant text, in order (also text the model wrote next to its tool calls). */
  answers: ChatMessage[]
  /** Hints a tool added to a proposal ("überschneidet sich …"), by proposal id. */
  hints: Record<string, string>
}

export function groupTurns(messages: ChatMessage[], steps: Record<string, string>, proposals: AgentProposal[]): Turn[] {
  const turns: Turn[] = []
  const results = new Map(messages.filter((m) => m.role === 'tool').map((m) => [m.tool_call_id, m]))
  let turn: Turn | null = null
  for (const m of messages) {
    if (m.role === 'user' || !turn) {
      turn = { question: m.role === 'user' ? m : null, steps: [], proposals: [], answers: [], hints: {} }
      turns.push(turn)
      if (m.role === 'user') continue
    }
    if (m.role === 'assistant') {
      for (const c of m.tool_calls) {
        const result = results.get(c.id)
        turn.steps.push({ id: c.id, label: steps[c.id] ?? c.name, failed: !!result?.content.startsWith('Fehler:') })
      }
      if (m.content.trim()) turn.answers.push(m)
    } else if (m.role === 'tool') {
      for (const p of proposals.filter((p) => p.message_id === m.id)) {
        turn.proposals.push(p)
        const hint = m.content.split('\n').find((l) => l.startsWith('Hinweis: '))
        if (hint) turn.hints[p.id] = hint.slice('Hinweis: '.length)
      }
    }
  }
  return turns
}

/** Where an applied proposal leads. */
export function proposalTarget(p: AgentProposal): { to: '/sessions/$sessionId'; id: string } | { to: '/reports/$reportId'; id: string } | { to: '/' } | null {
  if (p.status !== 'applied' || !p.result_id) return null
  switch (p.kind) {
    case 'session':
    case 'reschedule':
      return { to: '/sessions/$sessionId', id: p.result_id }
    case 'section_text':
    case 'note_text':
      return { to: '/reports/$reportId', id: p.result_id }
    default:
      return { to: '/' }
  }
}
