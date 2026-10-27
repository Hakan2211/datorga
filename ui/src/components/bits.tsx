import { useTranslation } from 'react-i18next'
import type { ReportStatus } from '@/bindings/ReportStatus'
import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'

const statusStyle: Record<ReportStatus, string> = {
  draft: 'bg-secondary text-secondary-foreground',
  in_review: 'bg-warning/20 text-foreground',
  signed: 'bg-success/15 text-success',
  exported: 'bg-accent text-accent-foreground',
}

export function StatusBadge({ status }: { status: ReportStatus }) {
  const { t } = useTranslation()
  return (
    <Badge variant="secondary" className={cn('font-normal', statusStyle[status])}>
      {t(`enums.status.${status}`)}
    </Badge>
  )
}

export function formatDate(iso: string | null | undefined, withTime = false): string {
  if (!iso) return '—'
  const d = new Date(iso.length === 10 ? `${iso}T00:00:00` : iso)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleString('de-DE', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    ...(withTime ? { hour: '2-digit', minute: '2-digit' } : {}),
  })
}
