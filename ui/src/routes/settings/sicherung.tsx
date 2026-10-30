import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { open } from '@tauri-apps/plugin-dialog'
import { DatabaseBackup, FolderOpen } from 'lucide-react'
import { RestoreBackup } from '@/components/backup-restore'
import { HelpLink } from '@/components/help-link'
import { formatDate } from '@/components/bits'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { DEFAULT_REMINDER_DAYS } from '@/lib/backup'
import { api, errorMessage, q, type LastBackup } from '@/lib/api'
import { HELP } from '@/lib/help'

export const Route = createFileRoute('/settings/sicherung')({ component: BackupCard })

function BackupCard() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const dir = useQuery(q.setting<string>('backup_dir'))
  const last = useQuery(q.setting<LastBackup>('last_backup'))
  const reminder = useQuery(q.setting<number>('backup_reminder_days'))
  const setReminder = useMutation({
    mutationFn: (days: number) => api.setSetting('backup_reminder_days', days),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['setting', 'backup_reminder_days'] }),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const choose = useMutation({
    mutationFn: async () => {
      const picked = await open({ directory: true, multiple: false })
      if (typeof picked !== 'string') return
      await api.setSetting('backup_dir', picked)
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ['setting', 'backup_dir'] }),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const create = useMutation({
    mutationFn: api.createBackup,
    onSuccess: (done) => {
      toast.success(t('backup.done', { path: done.path }), { duration: 10000 })
      void qc.invalidateQueries({ queryKey: ['setting', 'last_backup'] })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <Card className="max-w-2xl">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <DatabaseBackup className="size-4" /> {t('backup.title')} <HelpLink slug={HELP.backup} />
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-4 text-sm">
        <p className="text-muted-foreground">{t('backup.intro')}</p>
        <div className="space-y-1.5">
          <Label>{t('backup.folder')}</Label>
          <div className="flex items-center gap-2">
            <Input readOnly value={dir.data ?? ''} placeholder={t('backup.noFolder')} />
            <Button variant="outline" onClick={() => choose.mutate()} disabled={choose.isPending}>
              <FolderOpen /> {t('backup.chooseFolder')}
            </Button>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <Button onClick={() => create.mutate()} disabled={!dir.data || create.isPending}>
            <DatabaseBackup /> {create.isPending ? t('backup.running') : t('backup.now')}
          </Button>
          <span className="text-muted-foreground">
            {last.data
              ? t('backup.last', { date: formatDate(last.data.at, true), file: last.data.file })
              : t('backup.never')}
          </span>
        </div>
        <div className="flex items-center gap-3">
          <Label htmlFor="backup-reminder">{t('backup.reminder')}</Label>
          <Select
            value={String(reminder.data ?? DEFAULT_REMINDER_DAYS)}
            onValueChange={(v) => setReminder.mutate(Number(v))}
          >
            <SelectTrigger id="backup-reminder" className="w-48">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {[1, 3, 7, 14, 30, 0].map((d) => (
                <SelectItem key={d} value={String(d)}>
                  {d === 0 ? t('backup.reminderOff') : t('backup.reminderEvery', { count: d })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-2 border-t pt-4">
          <p className="text-muted-foreground">{t('backup.restoreIntro')}</p>
          <RestoreBackup replacesData />
        </div>
      </CardContent>
    </Card>
  )
}
