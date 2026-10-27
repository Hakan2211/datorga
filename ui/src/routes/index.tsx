import { createFileRoute, Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Page, PageHeader } from '@/components/app-shell'
import { ReportsTable } from '@/components/reports-table'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { q } from '@/lib/api'

export const Route = createFileRoute('/')({ component: Dashboard })

function Dashboard() {
  const { t } = useTranslation()
  const drafts = useQuery(q.reports('draft'))
  const patients = useQuery(q.patients())

  return (
    <>
      <PageHeader title={t('dashboard.title')} subtitle={t('privacy.local')} />
      <Page>
        <div className="grid grid-cols-3 gap-4">
          <Stat label={t('dashboard.drafts')} value={drafts.data?.length} />
          <Stat label={t('dashboard.patients')} value={patients.data?.length} />
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-medium text-muted-foreground">{t('dashboard.quickStart')}</CardTitle>
            </CardHeader>
            <CardContent>
              <ol className="list-decimal space-y-1 pl-4 text-sm">
                <li>{t('dashboard.step1')}</li>
                <li>{t('dashboard.step2')}</li>
                <li>{t('dashboard.step3')}</li>
                <li>{t('dashboard.step4')}</li>
              </ol>
              <Button asChild size="sm" className="mt-3">
                <Link to="/patients">{t('nav.patients')}</Link>
              </Button>
            </CardContent>
          </Card>
        </div>
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('dashboard.drafts')}</CardTitle>
          </CardHeader>
          <CardContent>
            <ReportsTable reports={drafts.data ?? []} />
          </CardContent>
        </Card>
      </Page>
    </>
  )
}

function Stat({ label, value }: { label: string; value: number | undefined }) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-medium text-muted-foreground">{label}</CardTitle>
      </CardHeader>
      <CardContent className="text-3xl font-semibold tabular-nums">{value ?? '–'}</CardContent>
    </Card>
  )
}
