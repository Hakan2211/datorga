import { useState } from 'react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { FilePlus2, FileStack, Pencil, SlidersHorizontal } from 'lucide-react'
import type { Template } from '@/bindings/Template'
import { Page, PageHeader } from '@/components/app-shell'
import { SectionCard } from '@/components/bits'
import { DeleteButton } from '@/components/delete-button'
import { StyleExemplarsCard } from '@/components/style-exemplars'
import { TemplateBuilderDialog } from '@/components/template-builder'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { api, errorMessage, q } from '@/lib/api'
import { blankTemplate, isOwn, putDraft } from '@/lib/templates'

export const Route = createFileRoute('/templates/')({ component: Templates })

function Templates() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const templates = useQuery(q.templates())
  const own = (templates.data ?? []).filter(isOwn)
  const builtin = (templates.data ?? []).filter((x) => !isOwn(x))
  const copy = useMutation({
    mutationFn: (key: string) => api.templateCopy(key),
    onSuccess: (draft) => void navigate({ to: '/templates/edit', search: { draft: putDraft(draft) } }),
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <>
      <PageHeader title={t('templates.title')} />
      <Page>
        <OwnTemplatesCard own={own} />
        <StyleExemplarsCard />
        <h2 className="text-sm font-medium text-muted-foreground">{t('templates.builtinTitle')}</h2>
        <div className="grid grid-cols-2 gap-4">
          {builtin.map((tpl) => (
            <Card key={tpl.key}>
              <CardHeader>
                <CardTitle className="flex items-start justify-between gap-3 text-base">
                  {tpl.title}
                  <Badge variant="secondary">{t('templates.builtin')}</Badge>
                </CardTitle>
                <CardDescription>
                  {[tpl.source, `v${tpl.version}`, tpl.report_kinds.map((k) => t(`enums.kind.${k}`)).join(', ')].filter(Boolean).join(' · ')}
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-3">
                <div>
                  <div className="mb-1 text-xs font-medium text-muted-foreground uppercase">{t('templates.sections')}</div>
                  <ol className="space-y-1 text-sm">
                    {tpl.sections.map((s) => (
                      <li key={s.key} className="flex gap-2">
                        <span className="w-5 shrink-0 text-right text-muted-foreground">{s.number ? `${s.number}.` : '·'}</span>
                        <span className="line-clamp-2">{s.heading || t('letterAi.opening')}</span>
                      </li>
                    ))}
                  </ol>
                </div>
                {tpl.layout === 'letter' && (
                  <Button variant="outline" size="sm" title={t('templates.adaptHint')} disabled={copy.isPending} onClick={() => copy.mutate(tpl.key)} data-testid="tpl-adapt">
                    <SlidersHorizontal /> {t('templates.adapt')}
                  </Button>
                )}
              </CardContent>
            </Card>
          ))}
        </div>
      </Page>
    </>
  )
}

/** "Eigene Vorlagen": hers, from a form or letter (builder), blank, or adapted from a built-in one. */
function OwnTemplatesCard({ own }: { own: Template[] }) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const [building, setBuilding] = useState(false)
  const blank = () => void navigate({ to: '/templates/edit', search: { draft: putDraft(blankTemplate(t('templates.blankTitle'))) } })
  return (
    <SectionCard
      icon={FileStack}
      title={t('templates.ownTitle')}
      description={t('templates.ownIntro')}
      actions={
        <>
          <Button size="sm" variant="outline" onClick={blank} data-testid="tpl-blank">
            <FilePlus2 /> {t('templates.blank')}
          </Button>
          <Button size="sm" onClick={() => setBuilding(true)} data-testid="tpl-build">
            <FileStack /> {t('templates.fromDocument')}
          </Button>
        </>
      }
      contentClassName="text-sm"
    >
      {own.length === 0 ? (
        <p className="rounded-md border border-dashed p-4 text-muted-foreground">{t('templates.ownEmpty')}</p>
      ) : (
        <ul className="divide-y rounded-md border">
          {own.map((x) => (
            <li key={x.key} className="flex items-center gap-3 px-3 py-2" data-testid="tpl-own">
              <div className="min-w-0 flex-1">
                <p className="truncate font-medium">{x.title}</p>
                <p className="text-xs text-muted-foreground">
                  {t(`tplEditor.category.${x.category}`)} · {t('tplEditor.sections', { count: x.sections.length })} · {t('tplEditor.version', { version: x.version })}
                </p>
              </div>
              <Button variant="ghost" size="icon" aria-label={t('templates.edit')} title={t('templates.edit')} asChild>
                <Link to="/templates/edit" search={{ key: x.key }} data-testid="tpl-edit">
                  <Pencil />
                </Link>
              </Button>
              <OwnDelete tpl={x} />
            </li>
          ))}
        </ul>
      )}
      <TemplateBuilderDialog open={building} onOpenChange={setBuilding} />
    </SectionCard>
  )
}

function OwnDelete({ tpl }: { tpl: Template }) {
  const { t } = useTranslation()
  const uses = useQuery({ queryKey: ['templates', 'uses', tpl.key], queryFn: () => api.templateUses(tpl.key) })
  return (
    <DeleteButton
      title={t('templates.deleteTitle')}
      description={`${t('templates.deleteBody', { title: tpl.title })}${uses.data ? ` ${t('templates.deleteUses', { count: uses.data })}` : ''}`}
      action={() => api.templateDelete(tpl.key)}
      testId="tpl-delete"
    />
  )
}
