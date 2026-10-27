import { createFileRoute, Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Mic } from 'lucide-react'
import { Page, PageHeader } from '@/components/app-shell'
import { formatDate } from '@/components/bits'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { q } from '@/lib/api'

export const Route = createFileRoute('/sessions/')({ component: Sessions })

function Sessions() {
  const { t } = useTranslation()
  const sessions = useQuery(q.sessions())
  const patients = useQuery(q.patients())
  const name = (id: string) => {
    const p = patients.data?.find((x) => x.id === id)
    return p ? `${p.last_name}, ${p.first_name}` : '—'
  }

  return (
    <>
      <PageHeader title={t('sessions.title')} />
      <Page>
        <p className="flex items-center gap-2 rounded-md bg-accent px-4 py-3 text-sm text-accent-foreground">
          <Mic className="size-4" /> {t('sessions.comingSoon')}
        </p>
        <Card>
          <CardContent className="pt-2">
            {sessions.data?.length === 0 ? (
              <p className="py-6 text-sm text-muted-foreground">{t('common.empty')}</p>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t('sessions.date')}</TableHead>
                    <TableHead>{t('patients.title')}</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead />
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {sessions.data?.map((s) => (
                    <TableRow key={s.id}>
                      <TableCell>{formatDate(s.scheduled_at, true)}</TableCell>
                      <TableCell>
                        <Link to="/patients/$patientId" params={{ patientId: s.patient_id }} className="hover:underline">
                          {name(s.patient_id)}
                        </Link>
                      </TableCell>
                      <TableCell className="text-muted-foreground">{t(`sessions.status.${s.status}`, s.status)}</TableCell>
                      <TableCell className="text-right">
                        <Button asChild size="sm" variant="ghost">
                          <Link to="/sessions/$sessionId" params={{ sessionId: s.id }}>
                            {t('sessions.open')}
                          </Link>
                        </Button>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </CardContent>
        </Card>
      </Page>
    </>
  )
}
