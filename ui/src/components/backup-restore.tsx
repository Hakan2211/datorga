import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { open } from '@tauri-apps/plugin-dialog'
import { ArchiveRestore } from 'lucide-react'
import { api, errorMessage, type BackupHeaderView } from '@/lib/api'
import { formatDate } from '@/components/bits'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'

/**
 * "Aus Datensicherung wiederherstellen": pick a .therabak, show what it is, ask for the
 * Notfall-Schlüssel unless the keychain already holds the backup's key, then restore.
 * `replacesData` = an install with data (Settings): the warning says where it goes.
 */
export function RestoreBackup({ replacesData }: { replacesData: boolean }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [picked, setPicked] = useState<{ path: string; header: BackupHeaderView } | null>(null)
  const [recoveryKey, setRecoveryKey] = useState('')

  const pick = useMutation({
    mutationFn: async () => {
      const path = await open({ multiple: false, filters: [{ name: t('backup.fileType'), extensions: ['therabak'] }] })
      if (typeof path !== 'string') return null
      return { path, header: await api.inspectBackup(path) }
    },
    onSuccess: (p) => {
      if (!p) return
      setPicked(p)
      setRecoveryKey('')
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  const restore = useMutation({
    mutationFn: () => api.restoreBackup(picked!.path, picked!.header.keychain_matches ? null : recoveryKey),
    onSuccess: async (done) => {
      toast.success(
        done.previous ? t('backup.restoredWithPrevious', { path: done.previous }) : t('backup.restored'),
        { duration: 15000 },
      )
      setPicked(null)
      // Everything cached belongs to the replaced database; `init` is refetched so the gate opens.
      qc.removeQueries({ predicate: (query) => query.queryKey[0] !== 'init' })
      await qc.resetQueries({ queryKey: ['init'] })
    },
  })

  if (!picked) {
    return (
      <Button variant="outline" onClick={() => pick.mutate()} disabled={pick.isPending}>
        <ArchiveRestore /> {t('backup.restore')}
      </Button>
    )
  }

  const needsKey = !picked.header.keychain_matches
  return (
    <div className="space-y-3 rounded-md border p-4 text-sm">
      <p>
        {t('backup.pickedInfo', {
          date: formatDate(picked.header.created_at, true),
          version: picked.header.app_version,
        })}
      </p>
      {replacesData && <p className="font-medium">{t('backup.replaceWarning')}</p>}
      {needsKey && (
        <div className="space-y-1.5">
          <Label htmlFor="restore-key">{t('backup.keyNeeded')}</Label>
          <Input id="restore-key" className="font-mono" value={recoveryKey} onChange={(e) => setRecoveryKey(e.target.value)} />
        </div>
      )}
      {restore.isError && <p className="text-destructive">{errorMessage(restore.error)}</p>}
      <div className="flex gap-2">
        <Button
          variant={replacesData ? 'destructive' : 'default'}
          onClick={() => restore.mutate()}
          disabled={restore.isPending || (needsKey && !recoveryKey.trim())}
        >
          {restore.isPending ? t('backup.restoring') : t('backup.restoreNow')}
        </Button>
        <Button variant="ghost" onClick={() => setPicked(null)} disabled={restore.isPending}>
          {t('common.cancel')}
        </Button>
      </div>
    </div>
  )
}
