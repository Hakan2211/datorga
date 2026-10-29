import { describe, expect, it } from 'vitest'
import { HELP, HELP_ARTICLES, helpArticle, linkTarget, parseArticle, parseBlocks, parseInline, searchHelp } from './help'

describe('help articles', () => {
  it('parses front matter and blocks', () => {
    const a = parseArticle(
      'x',
      '---\ntitle: Titel\nsummary: Kurz\norder: 3\nkeywords: eins, zwei\n---\nAbsatz eins\nweiter.\n\n## Kopf\n\n- a\n- b\n\n1. erst\n2. dann\n\n> Hinweis\n> geht weiter\n',
    )
    expect(a).toMatchObject({ slug: 'x', title: 'Titel', summary: 'Kurz', order: 3, keywords: ['eins', 'zwei'] })
    expect(a.blocks).toEqual([
      { type: 'p', text: 'Absatz eins weiter.' },
      { type: 'h2', text: 'Kopf' },
      { type: 'ul', items: ['a', 'b'] },
      { type: 'ol', items: ['erst', 'dann'] },
      { type: 'note', text: 'Hinweis geht weiter' },
    ])
  })

  it('ends a list at a paragraph and keeps notes apart', () => {
    expect(parseBlocks('- a\nText\n\n> eins\n\n> zwei')).toEqual([
      { type: 'ul', items: ['a'] },
      { type: 'p', text: 'Text' },
      { type: 'note', text: 'eins' },
      { type: 'note', text: 'zwei' },
    ])
  })

  it('parses bold, keys and links inline', () => {
    expect(parseInline('Mit **fett**, `Strg+K` und [Hilfe](hilfe:autopilot).')).toEqual([
      { type: 'text', text: 'Mit ' },
      { type: 'bold', text: 'fett' },
      { type: 'text', text: ', ' },
      { type: 'key', text: 'Strg+K' },
      { type: 'text', text: ' und ' },
      { type: 'link', text: 'Hilfe', href: 'hilfe:autopilot' },
      { type: 'text', text: '.' },
    ])
    expect(linkTarget('hilfe:bericht')).toEqual({ kind: 'article', slug: 'bericht' })
    expect(linkTarget('/settings')).toEqual({ kind: 'page', to: '/settings' })
    expect(linkTarget('https://openrouter.ai')).toEqual({ kind: 'web', url: 'https://openrouter.ai' })
  })

  it('bundles the nine planned articles in nav order', () => {
    expect(HELP_ARTICLES.map((a) => a.title)).toEqual([
      'Erste Schritte',
      'Sitzung aufnehmen',
      'Autopilot',
      'Notiz & Fakten',
      'Bericht schreiben',
      'Datenschutz & Pseudonymisierung',
      'KI-Anbieter & Kosten',
      'Datensicherung & Notfall-Kit',
      'Tastenkürzel',
    ])
    for (const slug of Object.values(HELP)) expect(helpArticle(slug), slug).toBeDefined()
  })

  it('links only to articles and pages that exist', () => {
    const pages = ['/', '/calendar', '/patients', '/sessions', '/reports', '/assistant', '/settings', '/hilfe']
    for (const a of HELP_ARTICLES) {
      for (const b of a.blocks) {
        for (const text of 'items' in b ? b.items : [b.text]) {
          for (const part of parseInline(text)) {
            if (part.type !== 'link') continue
            const target = linkTarget(part.href)
            if (target.kind === 'article') expect(helpArticle(target.slug), `${a.slug} → ${part.href}`).toBeDefined()
            if (target.kind === 'page') expect(pages, `${a.slug} → ${part.href}`).toContain(target.to)
          }
        }
      }
    }
  })
})

describe('searchHelp', () => {
  it('returns everything for an empty query', () => {
    expect(searchHelp(HELP_ARTICLES, '  ')).toHaveLength(HELP_ARTICLES.length)
  })

  it('ranks a title match first and needs every word', () => {
    const hits = searchHelp(HELP_ARTICLES, 'autopilot')
    expect(hits[0].article.slug).toBe('autopilot')
    expect(hits.length).toBeGreaterThan(1)
    expect(searchHelp(HELP_ARTICLES, 'autopilot xyzzy')).toEqual([])
  })

  it('ignores case and umlauts and shows a snippet from the body', () => {
    expect(searchHelp(HELP_ARTICLES, 'schlusselbund').length).toBeGreaterThan(0)
    const hits = searchHelp(HELP_ARTICLES, 'SCHLÜSSELBUND')
    expect(hits.map((h) => h.article.slug)).toContain('datensicherung')
    const hit = hits.find((h) => h.article.slug === 'datensicherung')!
    expect(hit.snippet).toMatch(/Schlüsselbund/)
    expect(hit.snippet!.length).toBeLessThanOrEqual(122)
  })

  it('finds articles by keyword', () => {
    expect(searchHelp(HELP_ARTICLES, 'openrouter')[0].article.slug).toBe('ki-anbieter')
  })
})
