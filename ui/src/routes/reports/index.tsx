import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Page, PageHeader } from '@/components/app-shell'
import { ReportsTable } from '@/components/reports-table'
import { Card, CardContent } from '@/components/ui/card'
import { q } from '@/lib/api'

export const Route = createFileRoute('/reports/')({ component: Reports })

function Reports() {
  const { t } = useTranslation()
  const reports = useQuery(q.reports())
  return (
    <>
      <PageHeader title={t('nav.reports')} />
      <Page>
        <Card>
          <CardContent className="pt-2">
            <ReportsTable reports={reports.data ?? []} />
          </CardContent>
        </Card>
      </Page>
    </>
  )
}
