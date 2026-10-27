import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { save } from '@tauri-apps/plugin-dialog'
import { CheckCircle2, Download } from 'lucide-react'
import { api, errorMessage } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'

/**
 * Shows a freshly issued recovery key once, offers the printable Notfall-Kit and only lets the
 * user continue after confirming it is stored (persisted, so an unconfirmed key is caught on the
 * next start). Used on first run and after rotation.
 */
export function RecoveryKitPanel({
  recoveryKey,
  rotated,
  onDone,
  doneLabel,
}: {
  recoveryKey: string
  rotated: boolean
  onDone: () => void
  doneLabel: string
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [checked, setChecked] = useState(false)
  const [savedTo, setSavedTo] = useState<string | null>(null)
  const exportKit = useMutation({
    mutationFn: async () => {
      const path = await save({
        defaultPath: `thera-Notfall-Kit-${new Date().toISOString().slice(0, 10)}.pdf`,
        filters: [{ name: 'PDF', extensions: ['pdf'] }],
      })
      if (!path) return null
      await api.exportRecoveryKit(recoveryKey, rotated, path)
      return path
    },
    onSuccess: (path) => path && setSavedTo(path),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const confirm = useMutation({
    mutationFn: api.confirmRecoveryKit,
    onSuccess: () => {
      qc.setQueryData(['init'], (d: object | undefined) => d && { ...d, recovery_key: null, recovery_confirmed: true })
      onDone()
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-4 gap-x-4 gap-y-2 rounded-md bg-muted p-4 text-center font-mono text-base tracking-wider select-all">
        {recoveryKey.split('-').map((g, i) => (
          <span key={i}>{g}</span>
        ))}
      </div>
      <div className="space-y-1.5">
        <Button variant="outline" onClick={() => exportKit.mutate()} disabled={exportKit.isPending}>
          <Download /> {t('recoveryKit.savePdf')}
        </Button>
        {savedTo ? (
          <p className="flex items-start gap-1.5 text-sm text-success">
            <CheckCircle2 className="mt-0.5 size-4 shrink-0" />
            <span>
              {t('recoveryKit.saved', { path: savedTo })}
            </span>
          </p>
        ) : (
          <p className="text-sm text-muted-foreground">{t('recoveryKit.pdfHint')}</p>
        )}
      </div>
      <label className="flex items-center gap-2 text-sm">
        <Checkbox checked={checked} onCheckedChange={(v) => setChecked(v === true)} />
        {t('recoveryKit.confirm')}
      </label>
      <Button disabled={!checked || confirm.isPending} onClick={() => confirm.mutate()}>
        {doneLabel}
      </Button>
    </div>
  )
}
