// A local OpenAI-compatible endpoint for the recording: fixed answers for Jonas Albrecht's
// session (facts, key points), Bericht drafts built from the facts thera sends and a Gutachter-Check.
// It answers by the system prompt like the E2E mocks: "sentences" = draft, "points" = summary,
// "hints" = Gutachter-Check, else facts.
import http from 'node:http'

/** Jonas's demo transcript, 1-based segment numbers ([S1] …). */
const FACTS = [
  { section: 'verlauf', text: 'Pat. berichtet von Bauchschmerzen am Vorabend einer Mathearbeit und dem Wunsch, morgens nicht zur Schule zu gehen.', segments: [2, 3] },
  { section: 'verlauf', text: 'In der Arbeit zunächst Blockade, nach etwa zehn Minuten Bewältigung durch Beginn mit der leichtesten Aufgabe.', segments: [5, 7] },
  { section: 'verlauf', text: 'Abwertende Reaktion des Vaters verstärkt Schuldgefühle.', segments: [9, 11] },
  { section: 'interventionen', text: 'Eigene Bewältigungsstrategie verstärkt und schriftlich festgehalten.', segments: [8] },
  { section: 'vereinbarungen', text: 'Gemeinsamer Termin mit den Eltern angeboten.', segments: [12] },
]

const POINTS = [
  { text: 'Bauchschmerzen und Schulvermeidung vor einer Mathearbeit.', sources: ['S2', 'S3'] },
  { text: 'Anfangs Blockade, dann selbst gefundene Strategie: mit der leichtesten Aufgabe beginnen.', sources: ['S5', 'S7'] },
  { text: 'Vater reagiert abwertend, Pat. fühlt sich danach schlechter.', sources: ['S9', 'S11'] },
  { text: 'Gemeinsamer Termin mit den Eltern angeregt.', sources: ['S12'] },
]

/** Up to three sentences from the facts in the request (`[F1] (label) text`). */
function draft(user) {
  const facts = [...user.matchAll(/^\[F(\d+)\] \([^)]*\) (.+)$/gm)].slice(0, 3)
  return { sentences: facts.map(([, n, text]) => ({ text: text.trim(), facts: [Number(n)] })) }
}

/** Gutachter-Check hints for the first two sections sent ([A1] …). */
function hints(user) {
  const sections = [...user.matchAll(/^\[A(\d+)\] /gm)].map(([, n]) => Number(n))
  return {
    hints: [
      { section: sections[1] ?? null, level: 'wichtig', text: 'Ein Gutachter würde fragen: Woran zeigt sich, dass die Kurzzeittherapie nicht ausreicht?' },
      { section: sections[0] ?? null, level: 'hinweis', text: 'Die Ziele für die Langzeittherapie könnten konkreter benannt werden.' },
    ],
  }
}

function answer(messages) {
  const system = messages.find((m) => m.role === 'system')?.content ?? ''
  const user = messages.filter((m) => m.role === 'user').map((m) => m.content).join('\n')
  if (system.includes('"sentences"')) return draft(user)
  if (system.includes('"points"')) return { points: user.includes('Mathearbeit') ? POINTS : [] }
  if (system.includes('"hints"')) return hints(user)
  return { facts: user.includes('Mathearbeit') ? FACTS : [] }
}

export function startMockLlm({ delayMs = 1800 } = {}) {
  const server = http.createServer((req, res) => {
    let body = ''
    req.on('data', (c) => (body += c))
    req.on('end', () => {
      if (req.method === 'GET' && req.url.endsWith('/models')) {
        res.writeHead(200, { 'Content-Type': 'application/json' })
        return res.end(JSON.stringify({ object: 'list', data: [{ id: 'thera-demo', object: 'model' }] }))
      }
      if (!req.url.endsWith('/chat/completions')) {
        res.writeHead(404)
        return res.end()
      }
      const content = JSON.stringify(answer(JSON.parse(body).messages ?? []))
      setTimeout(() => {
        res.writeHead(200, { 'Content-Type': 'application/json' })
        res.end(
          JSON.stringify({
            id: 'mock',
            object: 'chat.completion',
            model: 'thera-demo',
            choices: [{ index: 0, message: { role: 'assistant', content }, finish_reason: 'stop' }],
            usage: { prompt_tokens: Math.round(body.length / 4), completion_tokens: Math.round(content.length / 4), total_tokens: 0 },
          }),
        )
      }, delayMs)
    })
  })
  return new Promise((resolve) =>
    server.listen(0, '127.0.0.1', () => resolve({ url: `http://127.0.0.1:${server.address().port}/v1`, close: () => server.close() })),
  )
}
