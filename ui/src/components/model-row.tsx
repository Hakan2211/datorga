// One downloadable model (speech or language): size, state, use/download/import/remove, progress.
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { CheckCircle2, Download, FolderOpen, Trash2 } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { Transfer } from '@/lib/api'

export function ModelRow({
  title,
  bytes,
  description,
  badges,
  installed,
  active,
  transfer,
  busy,
  onUse,
  onDownload,
  onImport,
  onDelete,
}: {
  title: string
  bytes: number
  description: ReactNode
  /** Extra badges after the title (fit, recommendation, …). */
  badges?: ReactNode
  installed: boolean
  active: boolean
  transfer: Transfer | null
  busy: boolean
  onUse: () => void
  onDownload: () => void
  onImport: () => void
  onDelete: () => void
}) {
  const { t } = useTranslation()
  const moving = !!transfer && !transfer.error
  const pct = transfer ? Math.floor((transfer.received / transfer.total) * 100) : 0
  return (
    <li className="space-y-2 p-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-medium">{title}</span>
        <span className="text-muted-foreground">{formatBytes(bytes)}</span>
        {active && (
          <Badge variant="secondary" className="bg-success/15 font-normal text-success">
            {t('settings.stt.active')}
          </Badge>
        )}
        {badges}
        {installed && (
          <span className="flex items-center gap-1 text-success">
            <CheckCircle2 className="size-3.5" /> {t('settings.stt.installed')}
          </span>
        )}
        <span className="ml-auto flex gap-1">
          {installed && !active && (
            <Button size="sm" variant="outline" onClick={onUse} disabled={busy}>
              {t('settings.stt.use')}
            </Button>
          )}
          {!installed && !moving && (
            <>
              <Button size="sm" variant="outline" onClick={onDownload} disabled={busy}>
                <Download /> {t('settings.stt.download')}
              </Button>
              <Button size="sm" variant="ghost" onClick={onImport} disabled={busy}>
                <FolderOpen /> {t('settings.stt.import')}
              </Button>
            </>
          )}
          {installed && (
            <Button size="sm" variant="ghost" onClick={onDelete} disabled={busy} aria-label={t('settings.stt.delete')}>
              <Trash2 />
            </Button>
          )}
        </span>
      </div>
      <p className="text-muted-foreground">{description}</p>
      {moving && (
        <div className="flex items-center gap-3">
          <div className="h-2 flex-1 overflow-hidden rounded-full bg-muted">
            <div className="h-full bg-primary transition-[width]" style={{ width: `${pct}%` }} />
          </div>
          <span className="font-mono text-xs tabular-nums">{pct} %</span>
        </div>
      )}
      {transfer?.error && (
        <p className="text-destructive">
          {t('settings.stt.failed')}: {transfer.error}
        </p>
      )}
    </li>
  )
}

export function formatBytes(n: number): string {
  return n >= 1e9 ? `${(n / 1e9).toFixed(1).replace('.', ',')} GB` : `${Math.round(n / 1e6)} MB`
}
