import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { ShieldAlert, ShieldCheck } from 'lucide-react'
import { formatDate } from '@/components/bits'
import { Card, CardContent } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { api, q } from '@/lib/api'

export const Route = createFileRoute('/settings/protokoll')({ component: Audit })

function Audit() {
  const { t } = useTranslation()
  const events = useQuery(q.audit())
  const verify = useQuery({ queryKey: ['audit-verify'], queryFn: api.auditVerify })

  return (
    <>
      <p className="flex items-center justify-between gap-4 text-sm text-muted-foreground">
        {t('settings.auditHint')}
        {verify.data === null ? (
          <span className="flex shrink-0 items-center gap-1.5 text-success">
            <ShieldCheck className="size-4" /> {t('audit.intact')}
          </span>
        ) : verify.data !== undefined ? (
          <span className="flex shrink-0 items-center gap-1.5 text-destructive">
            <ShieldAlert className="size-4" /> {t('audit.broken')} #{verify.data}
          </span>
        ) : null}
      </p>
      <Card>
        <CardContent className="pt-2">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-16">#</TableHead>
                <TableHead>{t('audit.at')}</TableHead>
                <TableHead>{t('audit.action')}</TableHead>
                <TableHead>{t('audit.entity')}</TableHead>
                <TableHead>Hash</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {events.data?.map((e) => (
                <TableRow key={e.seq}>
                  <TableCell className="tabular-nums text-muted-foreground">{e.seq}</TableCell>
                  <TableCell>{formatDate(e.at, true)}</TableCell>
                  <TableCell className="font-mono text-xs">{e.action}</TableCell>
                  <TableCell className="font-mono text-xs text-muted-foreground">
                    {e.entity_type} {e.entity_id?.slice(0, 8)}
                  </TableCell>
                  <TableCell className="font-mono text-xs text-muted-foreground">{e.hash.slice(0, 12)}…</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </>
  )
}
