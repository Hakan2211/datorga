import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { getVersion } from '@tauri-apps/api/app'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Download, RefreshCw } from 'lucide-react'
import { SectionCard } from '@/components/bits'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { Progress } from '@/components/ui/progress'
import { Switch } from '@/components/ui/switch'
import { api, errorMessage, q, type UpdateOffer, type UpdateProgress } from '@/lib/api'

/** Setting: check for a new version at start (default on). */
export const AUTO_SETTING = 'update_auto'

/**
 * Banner above every page when the check at start found a newer version. "Später" hides it
 * until the next start; the check itself fails silently (offline).
 */
export function UpdateReminder() {
  const { t } = useTranslation()
  const [dismissed, setDismissed] = useState(false)
  const [open, setOpen] = useState(false)
  const auto = useQuery(q.setting<boolean>(AUTO_SETTING))
  const check = useQuery(q.update(!auto.isPending && auto.data !== false))
  const offer = check.data?.available
  if (!offer || (dismissed && !open)) return null
  return (
    <div role="status" className="flex flex-wrap items-center gap-3 border-b bg-primary/10 px-8 py-2.5 text-sm" data-testid="update-reminder">
      <Download className="size-4 shrink-0" />
      <span>
        <span className="font-medium">{t('update.bannerTitle', { version: offer.version })}</span> {t('update.bannerBody')}
      </span>
      <span className="ml-auto flex items-center gap-3">
        <Button size="sm" onClick={() => setOpen(true)} data-testid="update-open">
          {t('update.install')}
        </Button>
        <Button size="sm" variant="ghost" onClick={() => setDismissed(true)}>
          {t('backup.reminderLater')}
        </Button>
      </span>
      <UpdateDialog offer={offer} open={open} onOpenChange={setOpen} />
    </div>
  )
}

/** Release notes, then download with progress; the app closes when the installer starts. */
export function UpdateDialog({ offer, open, onOpenChange }: { offer: UpdateOffer; open: boolean; onOpenChange: (o: boolean) => void }) {
  const { t } = useTranslation()
  const [progress, setProgress] = useState<UpdateProgress | null>(null)
  const install = useMutation({
    mutationFn: api.updateInstall,
    onMutate: () => setProgress({ received: 0, total: null }),
    onError: (e) => {
      setProgress(null)
      toast.error(errorMessage(e))
    },
  })
  useEffect(() => {
    if (!install.isPending) return
    const off = api.onUpdateProgress(setProgress)
    return () => void off.then((f) => f())
  }, [install.isPending])

  const percent = progress?.total ? Math.min(100, Math.round((progress.received / progress.total) * 100)) : null
  const done = progress?.total != null && progress.received >= progress.total
  return (
    <Dialog open={open} onOpenChange={(o) => !install.isPending && onOpenChange(o)}>
      <DialogContent className="sm:max-w-lg" data-testid="update-dialog">
        <DialogHeader>
          <DialogTitle>{t('update.dialogTitle', { version: offer.version })}</DialogTitle>
          <DialogDescription>{t('update.dialogBody')}</DialogDescription>
        </DialogHeader>
        {offer.notes && (
          <div className="space-y-1.5 text-sm">
            <p className="font-medium">{t('update.notes')}</p>
            <p className="max-h-60 overflow-y-auto rounded-md bg-muted px-3 py-2 whitespace-pre-line text-muted-foreground" data-testid="update-notes">
              {offer.notes}
            </p>
          </div>
        )}
        {progress && (
          <div className="space-y-1.5 text-sm" data-testid="update-progress">
            <Progress value={percent ?? 0} />
            <p className="text-muted-foreground">
              {done
                ? t('update.installing')
                : percent != null
                  ? t('update.downloading', { percent })
                  : t('update.downloadingSize', { mb: (progress.received / 1e6).toFixed(0) })}
            </p>
          </div>
        )}
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={install.isPending}>
            {t('common.cancel')}
          </Button>
          <Button onClick={() => install.mutate()} disabled={install.isPending} data-testid="update-install">
            <Download /> {t('update.install')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** Einstellungen → Allgemein: installed version, manual check, auto-check switch. */
export function UpdateCard() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [open, setOpen] = useState(false)
  const auto = useQuery(q.setting<boolean>(AUTO_SETTING))
  const check = useQuery(q.update(!auto.isPending && auto.data !== false))
  const version = useQuery({ queryKey: ['appVersion'], queryFn: getVersion, staleTime: Infinity })
  const setAuto = useMutation({
    mutationFn: (v: boolean) => api.setSetting(AUTO_SETTING, v),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['setting', AUTO_SETTING] }),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const search = useMutation({
    mutationFn: api.updateCheck,
    onSuccess: (view) => {
      qc.setQueryData(q.update().queryKey, view)
      if (!view.available) toast.success(t('update.upToDate'))
    },
    onError: (e) => toast.error(t('update.checkFailed', { error: errorMessage(e) })),
  })
  const view = search.data ?? check.data
  const offer = view?.available

  return (
    <SectionCard icon={RefreshCw} title={t('update.settingsTitle')} className="max-w-2xl" contentClassName="space-y-4 text-sm">
      <div className="flex flex-wrap items-center gap-3">
        <span data-testid="update-current">{version.data && t('update.installed', { version: version.data })}</span>
        {offer && <span className="font-medium text-primary">{t('update.available', { version: offer.version })}</span>}
        <span className="ml-auto flex gap-2">
          {offer && (
            <Button onClick={() => setOpen(true)} data-testid="update-card-open">
              <Download /> {t('update.install')}
            </Button>
          )}
          <Button variant="outline" onClick={() => search.mutate()} disabled={search.isPending} data-testid="update-check">
            <RefreshCw /> {search.isPending ? t('update.checking') : t('update.check')}
          </Button>
        </span>
      </div>
      <div className="flex items-center gap-3">
        <Switch
          id="update-auto"
          checked={auto.data ?? true}
          onCheckedChange={(v) => setAuto.mutate(v)}
          disabled={auto.isPending || setAuto.isPending}
          data-testid="update-auto"
        />
        <Label htmlFor="update-auto">{t('update.auto')}</Label>
      </div>
      <p className="text-[13px] text-muted-foreground">{t('update.autoHint')}</p>
      <p className="text-[13px] text-muted-foreground">{t('update.copies')}</p>
      {offer && <UpdateDialog offer={offer} open={open} onOpenChange={setOpen} />}
    </SectionCard>
  )
}
