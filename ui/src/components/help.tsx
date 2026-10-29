// Hilfe (slice 34): the article page and the renderer for the bundled Markdown.
import { Fragment, useState } from 'react'
import { Link, useNavigate } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { openUrl } from '@tauri-apps/plugin-opener'
import { ChevronRight, Compass, KeyRound, ExternalLink, Home, Info, PlayCircle, Search } from 'lucide-react'
import { Page, PageHeader } from '@/components/app-shell'
import { DemoEnterButton } from '@/components/demo'
import { KeyGuide, type GuideProvider } from '@/components/key-guide'
import { useTour } from '@/components/tour'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Kbd } from '@/components/ui/kbd'
import { HELP, HELP_ARTICLES, helpArticle, linkTarget, parseInline, searchHelp, VIDEO_URL, type Block } from '@/lib/help'
import { cn } from '@/lib/utils'

export function HelpPage({ slug }: { slug: string }) {
  const { t } = useTranslation()
  const tour = useTour()
  const [query, setQuery] = useState('')
  const article = helpArticle(slug) ?? HELP_ARTICLES[0]
  const hits = searchHelp(HELP_ARTICLES, query)

  return (
    <>
      <PageHeader
        title={t('help.title')}
        subtitle={t('help.subtitle')}
        actions={
          <>
            <DemoEnterButton />
            <Button variant="outline" onClick={tour.start} data-testid="tour-start">
              <Compass /> {t('tour.start')}
            </Button>
          </>
        }
      />
      <Page className="grid grid-cols-[17rem_minmax(0,1fr)] items-start gap-8">
        <nav className="sticky top-6 space-y-3" aria-label={t('help.articles')}>
          <div className="relative">
            <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t('help.search')}
              className="pl-9"
              data-testid="help-search"
            />
          </div>
          <ul className="space-y-0.5" data-testid="help-nav">
            {hits.map(({ article: a, snippet }) => (
              <li key={a.slug}>
                <Link
                  to="/hilfe/$slug"
                  params={{ slug: a.slug }}
                  className={cn(
                    'group flex items-center gap-2 rounded-md border-l-2 border-transparent py-2 pr-2 pl-3 transition-colors hover:bg-foreground/5',
                    a.slug === article.slug && 'border-primary bg-card shadow-xs',
                  )}
                >
                  <span className="min-w-0 flex-1">
                    <span className="block text-sm font-medium">{a.title}</span>
                    <span className="line-clamp-2 block text-xs text-muted-foreground">{snippet ?? a.summary}</span>
                  </span>
                  <ChevronRight className="size-4 shrink-0 text-muted-foreground/60" />
                </Link>
              </li>
            ))}
            {!hits.length && <li className="px-3 py-2 text-sm text-muted-foreground">{t('help.noHits')}</li>}
          </ul>
          <div className="rounded-md border bg-card p-3 text-xs text-muted-foreground">
            <div className="mb-1 flex items-center gap-1.5 font-medium text-foreground">
              <Info className="size-3.5" /> {t('help.emergencyTitle')}
            </div>
            {t('help.emergencyBody')}
          </div>
        </nav>
        <article className="max-w-3xl space-y-6" data-testid="help-article">
          <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <Home className="size-3.5" /> {t('help.title')} <ChevronRight className="size-3.5" /> {article.title}
          </div>
          <header className="space-y-1">
            <h2 className="font-serif text-3xl">{article.title}</h2>
            <p className="text-primary">{article.summary}</p>
          </header>
          <ArticleBody blocks={article.blocks} />
          {article.slug === HELP.ai && <KeyGuides />}
          <VideoCard />
        </article>
      </Page>
    </>
  )
}

const GUIDES: { id: GuideProvider; name: string }[] = [
  { id: 'openrouter', name: 'OpenRouter' },
  { id: 'requesty', name: 'Requesty' },
  { id: 'openai', name: 'OpenAI' },
]

/** The KI-Anbieter article opens the same key guides as Einstellungen. */
function KeyGuides() {
  const { t } = useTranslation()
  const [open, setOpen] = useState<GuideProvider | null>(null)
  return (
    <div className="flex flex-wrap gap-2" data-testid="help-key-guides">
      {GUIDES.map((g) => (
        <Button key={g.id} variant="outline" size="sm" onClick={() => setOpen(g.id)}>
          <KeyRound /> {t('keyGuide.title', { provider: g.name })}
        </Button>
      ))}
      {open && <KeyGuide provider={open} open onOpenChange={(o) => !o && setOpen(null)} />}
    </div>
  )
}

function VideoCard() {
  const { t } = useTranslation()
  const ready = VIDEO_URL !== null
  return (
    <button
      type="button"
      disabled={!ready}
      onClick={() => VIDEO_URL && void openUrl(VIDEO_URL)}
      className="flex w-full items-center gap-4 rounded-lg border bg-card p-4 text-left shadow-xs transition-colors enabled:hover:bg-foreground/5 disabled:opacity-70"
      data-testid="help-video"
    >
      <PlayCircle className="size-10 shrink-0 text-primary" strokeWidth={1.5} />
      <span className="flex-1">
        <span className="block font-medium">{t('help.videoTitle')}</span>
        <span className="block text-sm text-muted-foreground">{ready ? t('help.videoBody') : t('help.videoSoon')}</span>
      </span>
      {ready && <ExternalLink className="size-4 text-muted-foreground" />}
    </button>
  )
}

function ArticleBody({ blocks }: { blocks: Block[] }) {
  return (
    <div className="space-y-4 text-[15px] leading-relaxed">
      {blocks.map((b, i) => {
        switch (b.type) {
          case 'h2':
            return (
              <h3 key={i} className="pt-2 text-lg font-semibold">
                <Rich text={b.text} />
              </h3>
            )
          case 'h3':
            return (
              <h4 key={i} className="font-semibold">
                <Rich text={b.text} />
              </h4>
            )
          case 'p':
            return (
              <p key={i}>
                <Rich text={b.text} />
              </p>
            )
          case 'note':
            return (
              <p key={i} className="flex gap-2 rounded-md border border-primary/20 bg-accent px-4 py-3 text-sm text-accent-foreground">
                <Info className="mt-0.5 size-4 shrink-0" />
                <span>
                  <Rich text={b.text} />
                </span>
              </p>
            )
          case 'ul':
            return (
              <ul key={i} className="list-disc space-y-1.5 pl-5 marker:text-muted-foreground">
                {b.items.map((item, j) => (
                  <li key={j}>
                    <Rich text={item} />
                  </li>
                ))}
              </ul>
            )
          case 'ol':
            return (
              <ol key={i} className="space-y-3">
                {b.items.map((item, j) => (
                  <li key={j} className="flex gap-3">
                    <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-primary text-xs font-medium text-primary-foreground">
                      {j + 1}
                    </span>
                    <span className="pt-0.5">
                      <Rich text={item} />
                    </span>
                  </li>
                ))}
              </ol>
            )
        }
      })}
    </div>
  )
}

const LINK = 'text-primary underline-offset-2 hover:underline'

function Rich({ text }: { text: string }) {
  const navigate = useNavigate()
  return (
    <>
      {parseInline(text).map((part, i) => {
        switch (part.type) {
          case 'text':
            return <Fragment key={i}>{part.text}</Fragment>
          case 'bold':
            return <strong key={i} className="font-semibold">{part.text}</strong>
          case 'key':
            return <Kbd key={i}>{part.text}</Kbd>
          case 'link': {
            const target = linkTarget(part.href)
            if (target.kind === 'article')
              return (
                <Link key={i} to="/hilfe/$slug" params={{ slug: target.slug }} className={LINK}>
                  {part.text}
                </Link>
              )
            if (target.kind === 'page')
              return (
                <button key={i} type="button" className={LINK} onClick={() => void navigate({ to: target.to })}>
                  {part.text}
                </button>
              )
            return (
              <button key={i} type="button" className={cn(LINK, 'inline-flex items-center gap-1')} onClick={() => void openUrl(target.url)}>
                {part.text} <ExternalLink className="size-3" />
              </button>
            )
          }
        }
      })}
    </>
  )
}
