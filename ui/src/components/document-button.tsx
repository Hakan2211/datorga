// "Datei oder Scan" (slice 49): picks a PDF, Word file, text file, photo or scan and reads it on
// this computer (the PDF's text layer, else the Windows text recognition). Nothing is uploaded.
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { FileUp, Loader2 } from 'lucide-react'
import type { Imported } from '@/bindings/Imported'
import { Button } from '@/components/ui/button'
import { api, errorMessage } from '@/lib/api'
import { importSummary, pickDocument } from '@/lib/documents'

export function DocumentButton({
  purpose,
  onRead,
  label,
  variant = 'outline',
  className,
  testId,
}: {
  purpose: 'request' | 'style'
  onRead: (imported: Imported) => void | Promise<void>
  label?: string
  variant?: 'outline' | 'ghost' | 'secondary' | 'default'
  className?: string
  testId?: string
}) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)

  const run = async () => {
    const path = await pickDocument(t('documents.pickTitle'), t('documents.filter'))
    if (!path) return
    setBusy(true)
    try {
      const imported = await api.readDocument(path, purpose)
      await onRead(imported)
      const s = importSummary(imported)
      const skipped = s.skipped ? ` ${t('documents.skipped', { count: s.skipped })}` : ''
      toast.success(t(s.key, { count: s.pages }) + skipped)
    } catch (e) {
      toast.error(errorMessage(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Button type="button" variant={variant} size="sm" className={className} disabled={busy} onClick={() => void run()} data-testid={testId}>
      {busy ? <Loader2 className="animate-spin" /> : <FileUp />} {busy ? t('documents.reading') : (label ?? t('documents.pick'))}
    </Button>
  )
}
