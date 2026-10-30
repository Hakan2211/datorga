import { useState } from 'react'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { KeyRound, Lock, LockOpen, ScrollText } from 'lucide-react'
import { RecoveryKitPanel } from '@/components/recovery-kit'
import { HelpLink } from '@/components/help-link'
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
import { api, errorMessage } from '@/lib/api'
import { HELP } from '@/lib/help'

export const Route = createFileRoute('/settings/sicherheit')({ component: Security })

function Security() {
  const { t } = useTranslation()
  const init = useQuery({ queryKey: ['init'], queryFn: api.init, staleTime: Infinity })
  return (
    <Card className="max-w-2xl">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          {t('settings.security')} <HelpLink slug={HELP.privacy} />
        </CardTitle>
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
        <div className="flex items-center justify-between gap-4 border-t pt-4">
          <p className="text-muted-foreground">{t('settings.auditHint')}</p>
          <Button asChild size="sm" variant="outline">
            <Link to="/settings/protokoll" data-testid="open-audit">
              <ScrollText /> {t('settings.auditOpen')}
            </Link>
          </Button>
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
