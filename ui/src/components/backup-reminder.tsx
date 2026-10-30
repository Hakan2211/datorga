import { useState } from 'react'
import { Link } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { DatabaseBackup } from 'lucide-react'
import { api, errorMessage, q, type LastBackup } from '@/lib/api'
import { backupDue, DEFAULT_REMINDER_DAYS } from '@/lib/backup'
import { Button } from '@/components/ui/button'

/** Banner above every page while a backup is due. "Später" hides it until the next start. */
export function BackupReminder() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [dismissed, setDismissed] = useState(false)
  const patients = useQuery(q.patients())
  const last = useQuery(q.setting<LastBackup>('last_backup'))
  const days = useQuery(q.setting<number>('backup_reminder_days'))
  const dir = useQuery(q.setting<string>('backup_dir'))
  const create = useMutation({
    mutationFn: api.createBackup,
    onSuccess: (done) => {
      toast.success(t('backup.done', { path: done.path }), { duration: 10000 })
      void qc.invalidateQueries({ queryKey: ['setting', 'last_backup'] })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  if (dismissed || !patients.data || last.isPending || days.isPending) return null
  const due = backupDue(last.data, days.data ?? DEFAULT_REMINDER_DAYS, patients.data.length > 0)
  if (due === false) return null

  return (
    <div role="status" className="flex flex-wrap items-center gap-3 border-b bg-warning/15 px-8 py-2.5 text-sm">
      <DatabaseBackup className="size-4 shrink-0" />
      <span>{due === null ? t('backup.reminderNever') : t('backup.reminderAge', { count: due })}</span>
      <span className="ml-auto flex gap-2">
        {dir.data ? (
          <Button size="sm" onClick={() => create.mutate()} disabled={create.isPending}>
            {create.isPending ? t('backup.running') : t('backup.now')}
          </Button>
        ) : (
          <Button size="sm" asChild>
            <Link to="/settings/sicherung">{t('backup.reminderSetup')}</Link>
          </Button>
        )}
        <Button size="sm" variant="ghost" onClick={() => setDismissed(true)}>
          {t('backup.reminderLater')}
        </Button>
      </span>
    </div>
  )
}
