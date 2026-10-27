import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Upload } from 'lucide-react'
import { Page, PageHeader } from '@/components/app-shell'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { q } from '@/lib/api'

export const Route = createFileRoute('/templates/')({ component: Templates })

function Templates() {
  const { t } = useTranslation()
  const templates = useQuery(q.templates())

  return (
    <>
      <PageHeader title={t('templates.title')} />
      <Page>
        <div className="grid grid-cols-2 gap-4">
          {templates.data?.map((tpl) => (
            <Card key={tpl.key}>
              <CardHeader>
                <CardTitle className="flex items-start justify-between gap-3 text-base">
                  {tpl.title}
                  <Badge variant="secondary">{t('templates.builtin')}</Badge>
                </CardTitle>
                <CardDescription>
                  {tpl.source} · v{tpl.version} · {tpl.report_kinds.map((k) => t(`enums.kind.${k}`)).join(', ')}
                </CardDescription>
              </CardHeader>
              <CardContent>
                <div className="mb-1 text-xs font-medium text-muted-foreground uppercase">{t('templates.sections')}</div>
                <ol className="space-y-1 text-sm">
                  {tpl.sections.map((s) => (
                    <li key={s.key} className="flex gap-2">
                      <span className="w-5 shrink-0 text-right text-muted-foreground">{s.number}.</span>
                      <span className="line-clamp-2">{s.heading}</span>
                    </li>
                  ))}
                </ol>
              </CardContent>
            </Card>
          ))}
        </div>
        <Card className="border-dashed">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Upload className="size-4" /> {t('templates.uploadTitle')}
            </CardTitle>
            <CardDescription>{t('templates.uploadBody')}</CardDescription>
          </CardHeader>
        </Card>
      </Page>
    </>
  )
}
