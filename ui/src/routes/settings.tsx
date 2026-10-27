import { useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { open } from '@tauri-apps/plugin-dialog'
import { AlertTriangle, DatabaseBackup, FolderOpen, KeyRound, Lock, LockOpen } from 'lucide-react'
import { Page, PageHeader } from '@/components/app-shell'
import { RecoveryKitPanel } from '@/components/recovery-kit'
import { RestoreBackup } from '@/components/backup-restore'
import { LlmCard } from '@/components/llm-settings'
import { ModelRow } from '@/components/model-row'
import { formatDate } from '@/components/bits'
import { DEFAULT_REMINDER_DAYS } from '@/lib/backup'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage, q, type LastBackup } from '@/lib/api'

export const Route = createFileRoute('/settings')({ component: Settings })

function Settings() {
  const { t } = useTranslation()
  const init = useQuery({ queryKey: ['init'], queryFn: api.init, staleTime: Infinity })

  return (
    <>
      <PageHeader title={t('settings.title')} />
      <Page>
        <Card className="max-w-2xl">
          <CardHeader>
            <CardTitle className="text-base">{t('nav.reports')}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-5">
            <SettingField settingKey="therapist_name" label={t('settings.therapist')} />
            <SettingField settingKey="practice_header" label={t('settings.practice')} multiline />
          </CardContent>
        </Card>
        <SttCard />
        <LlmCard />
        <BackupCard />
        <Card className="max-w-2xl">
          <CardHeader>
            <CardTitle className="text-base">{t('settings.security')}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-5 text-sm">
            <p className="flex items-center gap-2">
              {init.data?.encrypted ? (
                <>
                  <Lock className="size-4 text-success" /> {t('privacy.encrypted')} (SQLCipher, AES-256)
                </>
              ) : (
                <>
                  <LockOpen className="size-4 text-warning" /> {t('privacy.notEncrypted')}
                </>
              )}
            </p>
            <RecoveryKeySection />
          </CardContent>
        </Card>
      </Page>
    </>
  )
}

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
          <DatabaseBackup className="size-4" /> {t('backup.title')}
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

function RecoveryKeySection() {
  const { t } = useTranslation()
  const [newKey, setNewKey] = useState<string | null>(null)
  const rotate = useMutation({
    mutationFn: api.rotateRecoveryKey,
    onSuccess: (r) => {
      setNewKey(r.recovery_key)
      if (r.rekeyed > 0) toast.success(t('backup.rekeyed', { count: r.rekeyed }))
      if (r.rekey_failed > 0) toast.error(t('backup.rekeyFailed', { count: r.rekey_failed }), { duration: 15000 })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <div className="space-y-2">
      <p className="flex items-center gap-2 font-medium">
        <KeyRound className="size-4" /> {t('recoveryKit.title')}
      </p>
      <p className="text-muted-foreground">{t('recoveryKit.intro')}</p>
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button variant="outline" disabled={rotate.isPending}>
            {t('recoveryKit.rotate')}
          </Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('recoveryKit.rotateConfirmTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('recoveryKit.rotateConfirmBody')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction onClick={() => rotate.mutate()}>{t('recoveryKit.rotateAction')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      {/* The new key is shown once: the dialog only closes via the confirmed "Fertig". */}
      <Dialog open={newKey !== null}>
        <DialogContent
          className="sm:max-w-[38rem]"
          showCloseButton={false}
          onEscapeKeyDown={(e) => e.preventDefault()}
          onPointerDownOutside={(e) => e.preventDefault()}
        >
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <KeyRound className="size-5" /> {t('recoveryKit.newTitle')}
            </DialogTitle>
            <DialogDescription>{t('recoveryKit.newBody')}</DialogDescription>
          </DialogHeader>
          {newKey && (
            <RecoveryKitPanel recoveryKey={newKey} rotated onDone={() => setNewKey(null)} doneLabel={t('recoveryKit.done')} />
          )}
        </DialogContent>
      </Dialog>
    </div>
  )
}

function SettingField({ settingKey, label, multiline }: { settingKey: string; label: string; multiline?: boolean }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const current = useQuery(q.setting<string>(settingKey))
  const [value, setValue] = useState<string | null>(null)
  const shown = value ?? current.data ?? ''
  const saveSetting = useMutation({
    mutationFn: () => api.setSetting(settingKey, shown),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['setting', settingKey] })
      toast.success(t('common.saved'))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <div className="space-y-1.5">
      <Label htmlFor={settingKey}>{label}</Label>
      <div className="flex items-start gap-2">
        {multiline ? (
          <Textarea id={settingKey} rows={2} value={shown} onChange={(e) => setValue(e.target.value)} />
        ) : (
          <Input id={settingKey} value={shown} onChange={(e) => setValue(e.target.value)} />
        )}
        <Button variant="outline" onClick={() => saveSetting.mutate()} disabled={saveSetting.isPending}>
          {t('common.save')}
        </Button>
      </div>
    </div>
  )
}

function SttCard() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const stt = useQuery(q.stt())
  const refresh = () => void qc.invalidateQueries({ queryKey: ['stt'] })
  const act = useMutation({
    mutationFn: (f: () => Promise<void>) => f(),
    onSuccess: refresh,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const importFile = async (id: string) => {
    const path = await open({ multiple: false, filters: [{ name: 'ggml', extensions: ['bin'] }] })
    if (typeof path === 'string') act.mutate(() => api.importModel(id, path))
  }
  const data = stt.data

  return (
    <Card className="max-w-2xl">
      <CardHeader>
        <CardTitle className="text-base">{t('settings.stt.title')}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4 text-sm">
        <p className="text-muted-foreground">{t('settings.stt.intro')}</p>
        {data && !data.engine && (
          <p className="flex items-center gap-2 text-destructive">
            <AlertTriangle className="size-4" /> {t('settings.stt.engineMissing')}
          </p>
        )}
        <ul className="divide-y rounded-md border">
          {data?.models.map((m) => (
            <ModelRow
              key={m.id}
              title={m.id}
              bytes={m.bytes}
              description={t(`settings.stt.models.${m.id}`)}
              installed={m.installed}
              transfer={m.transfer}
              active={data.active === m.id}
              busy={act.isPending}
              onUse={() => act.mutate(() => api.selectModel(m.id))}
              onDownload={() => act.mutate(() => api.downloadModel(m.id))}
              onImport={() => void importFile(m.id)}
              onDelete={() => act.mutate(() => api.deleteModel(m.id))}
            />
          ))}
        </ul>
        {data?.engine && (
          <p className="text-xs text-muted-foreground">
            {t('settings.stt.engine')}: {data.engine}
          </p>
        )}
      </CardContent>
    </Card>
  )
}
