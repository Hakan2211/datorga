import { describe, expect, it } from 'vitest'
import type { AgentProposal } from '@/bindings/AgentProposal'
import type { ChatMessage } from '@/bindings/ChatMessage'
import { currentZone, groupTurns, proposalTarget } from './assistant'

describe('currentZone', () => {
  it('finds the DST changes to the hour', () => {
    // Berlin-like: CEST (+120) between 2026-03-29T01:00Z and 2026-10-25T01:00Z, CET (+60) otherwise.
    const summer = (ms: number) =>
      (ms >= Date.UTC(2026, 2, 29, 1) && ms < Date.UTC(2026, 9, 25, 1)) || (ms >= Date.UTC(2027, 2, 28, 1) && ms < Date.UTC(2027, 9, 31, 1))
    const offsetAt = (ms: number) => (summer(ms) ? 120 : 60)
    const zone = currentZone(new Date('2026-10-28T13:05:00Z'), offsetAt)
    expect(zone.base).toBe(60)
    expect(zone.changes).toEqual([
      { at: '2026-03-29T01:00:00Z', offset: 120 },
      { at: '2026-10-25T01:00:00Z', offset: 60 },
      { at: '2027-03-28T01:00:00Z', offset: 120 },
      { at: '2027-10-31T01:00:00Z', offset: 60 },
    ])
  })

  it('has no changes in a zone without DST', () => {
    expect(currentZone(new Date('2026-10-28T13:05:00Z'), () => 0)).toEqual({ base: 0, changes: [] })
  })
})

const msg = (over: Partial<ChatMessage>): ChatMessage => ({
  id: 'm',
  seq: 0,
  role: 'assistant',
  content: '',
  tool_calls: [],
  tool_call_id: null,
  tool_name: null,
  created_at: '',
  ...over,
})

const proposal = (over: Partial<AgentProposal>): AgentProposal => ({
  id: 'p1',
  thread_id: 't',
  message_id: 'm3',
  kind: 'session',
  patient_id: 'x',
  summary: 'Termin',
  payload: {},
  status: 'open',
  result_id: null,
  created_at: '',
  decided_at: null,
  ...over,
})

describe('groupTurns', () => {
  it('collects steps, proposals with hints and answers per question', () => {
    const messages = [
      msg({ id: 'm1', role: 'user', content: 'Termin?' }),
      msg({ id: 'm2', tool_calls: [{ id: 'a', name: 'search_patients', arguments: {} }, { id: 'b', name: 'propose_session', arguments: {} }] }),
      msg({ id: 'm3x', role: 'tool', tool_call_id: 'a', content: 'Fehler: nicht gefunden' }),
      msg({ id: 'm3', role: 'tool', tool_call_id: 'b', content: 'Vorschlag angelegt.\nHinweis: überschneidet sich' }),
      msg({ id: 'm4', content: 'Liegt bereit.' }),
      msg({ id: 'm5', role: 'user', content: 'Danke' }),
    ]
    const turns = groupTurns(messages, { a: 'Gesucht' }, [proposal({})])
    expect(turns).toHaveLength(2)
    expect(turns[0].steps).toEqual([
      { id: 'a', label: 'Gesucht', failed: true },
      { id: 'b', label: 'propose_session', failed: false },
    ])
    expect(turns[0].proposals.map((p) => p.id)).toEqual(['p1'])
    expect(turns[0].hints).toEqual({ p1: 'überschneidet sich' })
    expect(turns[0].answers.map((a) => a.id)).toEqual(['m4'])
    expect(turns[1]).toMatchObject({ steps: [], answers: [] })
  })
})

describe('proposalTarget', () => {
  it('links what was applied', () => {
    expect(proposalTarget(proposal({}))).toBeNull()
    expect(proposalTarget(proposal({ status: 'applied', result_id: 's1' }))).toEqual({ to: '/sessions/$sessionId', id: 's1' })
    expect(proposalTarget(proposal({ status: 'applied', result_id: 'r1', kind: 'note_text' }))).toEqual({ to: '/reports/$reportId', id: 'r1' })
    expect(proposalTarget(proposal({ status: 'applied', result_id: '2026-10-30', kind: 'day_plan' }))).toEqual({ to: '/' })
  })
})
