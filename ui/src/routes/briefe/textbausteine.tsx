import { useState } from 'react'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Copy, Pencil, Plus, RotateCcw, Search, TextQuote } from 'lucide-react'
import { Page, PageHeader } from '@/components/app-shell'
import { EmptyState, FilterChip } from '@/components/bits'
import { TextBlockFormDialog } from '@/components/briefe/textblock-form'
import { DeleteButton } from '@/components/delete-button'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { api, errorMessage, q } from '@/lib/api'
import { blockSnippet, letterTemplates } from '@/lib/letters'

export const Route = createFileRoute('/briefe/textbausteine')({ component: TextBlocks })

function TextBlocks() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [query, setQuery] = useState('')
  const [type, setType] = useState<string | null>(null)
  const blocks = useQuery(q.textblocks())
  const templates = useQuery(q.templates())
  const types = letterTemplates(templates.data ?? [], null, ['bericht', 'brief']).map((x) => x.template)
  const title = (key: string) => types.find((x) => x.key === key)?.title ?? key
  const needle = query.trim().toLowerCase()
  const shown = (blocks.data ?? []).filter(
    (b) =>
      (!type || b.template_keys.length === 0 || b.template_keys.includes(type)) &&
      (!needle || b.title.toLowerCase().includes(needle) || blockSnippet(b.body, 100_000).toLowerCase().includes(needle)),
  )
  const restore = useMutation({
    mutationFn: api.restoreTextblocks,
    onSuccess: (n) => {
      void qc.invalidateQueries({ queryKey: ['textblocks'] })
      toast.success(t('textblocks.restored', { count: n }))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <>
      <PageHeader
        title={t('textblocks.title')}
        subtitle={
          <span>
            <Link to="/briefe" className="hover:underline">
              {t('briefe.title')}
            </Link>{' '}
            · {t('textblocks.subtitle')}
          </span>
        }
        actions={
          <>
            <Button variant="outline" onClick={() => restore.mutate()} disabled={restore.isPending} data-testid="textblocks-restore">
              <RotateCcw /> {t('textblocks.restore')}
            </Button>
            <TextBlockFormDialog
              trigger={
                <Button data-testid="textblock-new">
                  <Plus /> {t('textblocks.new')}
                </Button>
              }
            />
          </>
        }
      />
      <Page>
        <p className="max-w-3xl text-sm text-muted-foreground">{t('textblocks.intro')}</p>
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative w-80">
            <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t('textblocks.search')} className="pl-9" />
          </div>
          <FilterChip active={!type} onClick={() => setType(null)}>
            {t('textblocks.allTypes')}
          </FilterChip>
          {types.map((x) => (
            <FilterChip key={x.key} active={type === x.key} onClick={() => setType(type === x.key ? null : x.key)}>
              {x.title}
            </FilterChip>
          ))}
        </div>
        {blocks.data?.length === 0 ? (
          <EmptyState icon={TextQuote} title={t('textblocks.emptyTitle')} description={t('textblocks.emptyBody')} />
        ) : (
          <div className="grid gap-3 lg:grid-cols-2">
            {shown.map((b) => (
              <div key={b.id} className="flex flex-col gap-2 rounded-lg border bg-card p-4" data-testid="textblock-row">
                <div className="flex items-start justify-between gap-2">
                  <div className="min-w-0">
                    <p className="font-medium">{b.title}</p>
                    <div className="mt-1 flex flex-wrap gap-1">
                      {b.origin === 'builtin' && <Badge variant="secondary">{t('textblocks.builtin')}</Badge>}
                      {b.template_keys.length === 0 ? (
                        <Badge variant="outline">{t('textblocks.allTypes')}</Badge>
                      ) : (
                        b.template_keys.map((k) => (
                          <Badge key={k} variant="outline">
                            {title(k)}
                          </Badge>
                        ))
                      )}
                    </div>
                  </div>
                  <div className="flex shrink-0">
                    <TextBlockFormDialog
                      initial={{ ...b, title: t('textblocks.copyOf', { title: b.title }) }}
                      trigger={
                        <Button variant="ghost" size="icon" aria-label={t('textblocks.duplicate')} title={t('textblocks.duplicate')}>
                          <Copy />
                        </Button>
                      }
                    />
                    <TextBlockFormDialog
                      block={b}
                      trigger={
                        <Button variant="ghost" size="icon" aria-label={t('textblocks.edit')} title={t('textblocks.edit')} data-testid="textblock-edit">
                          <Pencil />
                        </Button>
                      }
                    />
                    <DeleteButton
                      title={t('textblocks.delete')}
                      description={b.origin === 'builtin' ? t('textblocks.deleteBuiltin') : t('textblocks.deleteText')}
                      action={() => api.deleteTextblock(b.id)}
                    />
                  </div>
                </div>
                <p className="line-clamp-3 text-sm text-muted-foreground">{blockSnippet(b.body, 400)}</p>
                <p className="mt-auto text-xs text-muted-foreground tabular-nums">{t('textblocks.uses', { count: b.use_count })}</p>
              </div>
            ))}
          </div>
        )}
      </Page>
    </>
  )
}
