// Hilfe (slice 34): articles bundled as Markdown in `content/hilfe/*.md`, offline. A small
// parser covers what the articles use (headings, paragraphs, lists, notes, bold, keys, links),
// so no Markdown library and no raw HTML.

/** The explainer video (slice 35, `docs/video/skript.md`), hosted on bunny.net. `null` until
 * it is published: the Hilfe card then says "erscheint in Kürze" and the tour shows no link. */
export const VIDEO_URL: string | null = 'https://memdia-cdn.b-cdn.net/theralo-in-5-minuten.mp4'

export type Block =
  | { type: 'h2' | 'h3' | 'p' | 'note'; text: string }
  | { type: 'ul' | 'ol'; items: string[] }

export type HelpArticle = {
  slug: string
  title: string
  summary: string
  order: number
  keywords: string[]
  blocks: Block[]
  /** Plain text of the body, for search and snippets. */
  text: string
}

export type Inline =
  | { type: 'text'; text: string }
  | { type: 'bold'; text: string }
  | { type: 'key'; text: string }
  | { type: 'link'; text: string; href: string }

/** `---` front matter (key: value lines) and the Markdown body. */
export function parseArticle(slug: string, source: string): HelpArticle {
  const src = source.replace(/\r\n/g, '\n')
  const meta: Record<string, string> = {}
  let body = src
  const fm = /^---\n([\s\S]*?)\n---\n?/.exec(src)
  if (fm) {
    for (const line of fm[1].split('\n')) {
      const i = line.indexOf(':')
      if (i > 0) meta[line.slice(0, i).trim()] = line.slice(i + 1).trim()
    }
    body = src.slice(fm[0].length)
  }
  const blocks = parseBlocks(body)
  return {
    slug,
    title: meta.title ?? slug,
    summary: meta.summary ?? '',
    order: Number(meta.order ?? 99),
    keywords: (meta.keywords ?? '').split(',').map((k) => k.trim()).filter(Boolean),
    blocks,
    text: blocks
      .flatMap((b) => ('items' in b ? b.items : [b.text]))
      .map(plain)
      .join('\n'),
  }
}

export function parseBlocks(body: string): Block[] {
  const blocks: Block[] = []
  let para: string[] = []
  let note: string[] = []
  let list: { type: 'ul' | 'ol'; items: string[] } | null = null
  const flush = () => {
    if (para.length) blocks.push({ type: 'p', text: para.join(' ') })
    if (note.length) blocks.push({ type: 'note', text: note.join(' ') })
    if (list) blocks.push(list)
    para = []
    note = []
    list = null
  }
  for (const raw of body.split('\n')) {
    const line = raw.trim()
    if (!line) {
      flush()
      continue
    }
    const heading = /^(#{2,3})\s+(.*)$/.exec(line)
    const bullet = /^[-*]\s+(.*)$/.exec(line)
    const numbered = /^\d+\.\s+(.*)$/.exec(line)
    if (heading) {
      flush()
      blocks.push({ type: heading[1].length === 2 ? 'h2' : 'h3', text: heading[2] })
    } else if (line.startsWith('>')) {
      if (!note.length) flush()
      note.push(line.replace(/^>\s?/, ''))
    } else if (bullet || numbered) {
      const type = bullet ? 'ul' : 'ol'
      if (para.length || note.length || (list && list.type !== type)) flush()
      list ??= { type, items: [] }
      list.items.push((bullet ?? numbered)![1])
    } else if (list && raw.startsWith('  ')) {
      // A wrapped list item.
      list.items[list.items.length - 1] += ` ${line}`
    } else {
      if (list || note.length) flush()
      para.push(line)
    }
  }
  flush()
  return blocks
}

/** `**bold**`, `` `Strg+K` `` (a key) and `[text](href)`. */
export function parseInline(text: string): Inline[] {
  const out: Inline[] = []
  const re = /\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)\]\(([^)\s]+)\)/g
  let last = 0
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m.index > last) out.push({ type: 'text', text: text.slice(last, m.index) })
    if (m[1] !== undefined) out.push({ type: 'bold', text: m[1] })
    else if (m[2] !== undefined) out.push({ type: 'key', text: m[2] })
    else out.push({ type: 'link', text: m[3], href: m[4] })
    last = m.index + m[0].length
  }
  if (last < text.length) out.push({ type: 'text', text: text.slice(last) })
  return out
}

function plain(text: string): string {
  return parseInline(text)
    .map((i) => i.text)
    .join('')
}

/** Where a link in an article goes: another article, a page of the app, or the web. */
export function linkTarget(href: string): { kind: 'article'; slug: string } | { kind: 'page'; to: string } | { kind: 'web'; url: string } {
  if (href.startsWith('hilfe:')) return { kind: 'article', slug: href.slice(6) }
  if (href.startsWith('/')) return { kind: 'page', to: href }
  return { kind: 'web', url: href }
}

const fold = (s: string) => s.toLocaleLowerCase('de').normalize('NFKD').replace(/\p{Diacritic}/gu, '').replace(/ß/g, 'ss')

export type HelpHit = { article: HelpArticle; snippet: string | null }

/**
 * Articles containing every word of `query`, best first: title before keywords before
 * summary before body. The snippet is the body line with the first word, if the body has it.
 */
export function searchHelp(articles: HelpArticle[], query: string): HelpHit[] {
  const words = fold(query).split(/\s+/).filter(Boolean)
  if (!words.length) return articles.map((article) => ({ article, snippet: null }))
  const hits: { hit: HelpHit; score: number }[] = []
  for (const article of articles) {
    const fields = {
      title: fold(article.title),
      keywords: fold(article.keywords.join(' ')),
      summary: fold(article.summary),
      body: fold(article.text),
    }
    let score = 0
    let all = true
    for (const w of words) {
      const s =
        (fields.title.includes(w) ? 10 : 0) +
        (fields.keywords.includes(w) ? 5 : 0) +
        (fields.summary.includes(w) ? 3 : 0) +
        (fields.body.includes(w) ? 1 : 0)
      if (!s) all = false
      score += s
    }
    if (!all) continue
    const line = article.text.split('\n').find((l) => fold(l).includes(words[0])) ?? null
    hits.push({ hit: { article, snippet: line && snippet(line, words[0]) }, score })
  }
  return hits.sort((a, b) => b.score - a.score || a.hit.article.order - b.hit.article.order).map((h) => h.hit)
}

/** About 120 characters of `line` around `word`. */
function snippet(line: string, word: string): string {
  if (line.length <= 120) return line
  const at = Math.max(0, fold(line).indexOf(word) - 40)
  const cut = line.slice(at, at + 120)
  return `${at > 0 ? '…' : ''}${cut}${at + 120 < line.length ? '…' : ''}`
}

const sources = import.meta.glob('../content/hilfe/*.md', { query: '?raw', import: 'default', eager: true }) as Record<string, string>

/** All bundled articles in nav order. */
export const HELP_ARTICLES: HelpArticle[] = Object.entries(sources)
  .map(([path, src]) => parseArticle(path.replace(/^.*\/(.+)\.md$/, '$1'), src))
  .sort((a, b) => a.order - b.order)

export function helpArticle(slug: string): HelpArticle | undefined {
  return HELP_ARTICLES.find((a) => a.slug === slug)
}

/** Slugs the "?" links on cards may point to; a test checks each has an article. */
export const HELP = {
  start: 'erste-schritte',
  capture: 'sitzung-aufnehmen',
  autopilot: 'autopilot',
  note: 'notiz-und-fakten',
  report: 'bericht',
  privacy: 'datenschutz',
  ai: 'ki-anbieter',
  backup: 'datensicherung',
  keys: 'tastenkuerzel',
} as const
export type HelpSlug = (typeof HELP)[keyof typeof HELP]
