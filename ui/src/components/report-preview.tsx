import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AlertTriangle, Loader2, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { api, errorMessage } from '@/lib/api'

const RENDER_DELAY_MS = 1200
// Pixels per point: sharp on a 150 % display without making each page too large to send.
const SCALE = 2

/**
 * The export as it would look now, page by page, from the same typst compile as the PDF.
 * Re-rendered once edits pause; `flush` saves pending edits first. Old pages stay visible
 * while the next render runs, so the scroll position is kept.
 */
export function ReportPreview({
  reportId,
  content,
  flush,
  onClose,
}: {
  reportId: string
  /** Changes whenever the report's text changes. */
  content: string
  flush: () => Promise<void>
  /** Without it the preview has no close button (a Lotse step where it is part of the screen). */
  onClose?: () => void
}) {
  const { t } = useTranslation()
  const [pages, setPages] = useState<string[]>([])
  const [busy, setBusy] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const flushRef = useRef(flush)
  flushRef.current = flush
  const first = useRef(true)

  useEffect(() => {
    let stale = false
    setBusy(true)
    const run = async () => {
      try {
        await flushRef.current()
        const next = await api.previewReport(reportId, SCALE)
        if (!stale) {
          setPages(next)
          setError(null)
        }
      } catch (e) {
        if (!stale) setError(errorMessage(e))
      } finally {
        if (!stale) setBusy(false)
      }
    }
    // The first render goes out right away; later ones wait until typing pauses.
    const timer = setTimeout(run, first.current ? 0 : RENDER_DELAY_MS)
    first.current = false
    return () => {
      stale = true
      clearTimeout(timer)
    }
  }, [reportId, content])

  return (
    <div className="flex h-full min-h-0 flex-col rounded-md border bg-muted/50" data-testid="report-preview">
      <div className="flex items-center gap-2 border-b bg-card px-3 py-1.5 text-xs text-muted-foreground">
        <span className="font-medium text-foreground">{t('report.preview')}</span>
        <span className="truncate">{t('report.previewHint')}</span>
        {busy && (
          <span className="ml-auto flex shrink-0 items-center gap-1">
            <Loader2 className="size-3 animate-spin" /> {t('report.previewUpdating')}
          </span>
        )}
        {onClose && (
          <Button
            variant="ghost"
            size="icon"
            className={busy ? 'size-6' : 'ml-auto size-6'}
            onClick={onClose}
            aria-label={t('report.previewClose')}
          >
            <X className="size-3.5" />
          </Button>
        )}
      </div>
      {error && (
        <p className="flex items-start gap-2 border-b bg-destructive/10 px-3 py-2 text-xs text-destructive">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" /> {t('report.previewFailed')}: {error}
        </p>
      )}
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-4">
        {pages.map((src, i) => (
          <img
            key={i}
            src={src}
            alt={t('report.page', { n: i + 1 })}
            className="mx-auto w-full max-w-[794px] bg-white shadow-md"
            draggable={false}
          />
        ))}
      </div>
    </div>
  )
}
