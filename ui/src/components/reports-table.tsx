import { Link } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import type { ReportSummary } from '@/bindings/ReportSummary'
import { StatusBadge, formatDate } from '@/components/bits'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

export function ReportsTable({ reports }: { reports: ReportSummary[] }) {
  const { t } = useTranslation()
  if (reports.length === 0) return <p className="py-6 text-sm text-muted-foreground">{t('common.noMatch')}</p>
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>{t('patients.title')}</TableHead>
          <TableHead>{t('patients.chiffre')}</TableHead>
          <TableHead>{t('episodes.method')}</TableHead>
          <TableHead>{t('nav.reports')}</TableHead>
          <TableHead>Status</TableHead>
          <TableHead>{t('reports.sectionsHead')}</TableHead>
          <TableHead className="text-right">{t('audit.at')}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {reports.map((r) => (
          <TableRow key={r.id} className="cursor-pointer">
            <TableCell className="font-medium">
              <Link to="/reports/$reportId" params={{ reportId: r.id }} className="hover:underline">
                {r.patient_name}
              </Link>
            </TableCell>
            <TableCell className="font-mono text-xs">{r.chiffre ?? '—'}</TableCell>
            <TableCell>{r.method ?? '—'}</TableCell>
            <TableCell>
              {r.kind ? t(`enums.kind.${r.kind}`) : '—'}
              {r.kind === 'fortfuehrung' && r.seq ? ` Nr. ${r.seq}` : ''}
            </TableCell>
            <TableCell>
              <StatusBadge status={r.status} />
            </TableCell>
            <TableCell className="text-muted-foreground tabular-nums">
              {r.sections_filled}/{r.sections_total}
            </TableCell>
            <TableCell className="text-right text-muted-foreground">{formatDate(r.updated_at, true)}</TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  )
}
