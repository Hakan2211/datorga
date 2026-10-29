import { useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { KeyRound, ShieldAlert, Sparkles } from 'lucide-react'
import { api, errorMessage, isTauri } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { RecoveryKitPanel } from '@/components/recovery-kit'
import { RestoreBackup } from '@/components/backup-restore'
import { DemoEnterButton } from '@/components/demo'

/** Opens the encrypted store before anything else renders. Shows the recovery key once on first run. */
export function UnlockGate({ children }: { children: ReactNode }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const init = useQuery({ queryKey: ['init'], queryFn: api.init, enabled: isTauri(), staleTime: Infinity })
  const [rotatedKey, setRotatedKey] = useState<string | null>(null)
  const [recoveryInput, setRecoveryInput] = useState('')
  const rotate = useMutation({ mutationFn: api.rotateRecoveryKey, onSuccess: (r) => setRotatedKey(r.recovery_key) })
  const setup = useMutation({
    mutationFn: api.setup,
    onSuccess: (r) => qc.setQueryData(['init'], r),
  })
  const recover = useMutation({
    mutationFn: () => api.recover(recoveryInput),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['init'] }),
  })

  if (!isTauri()) return <Centered><p className="text-muted-foreground">{t('setup.outsideTauri')}</p></Centered>
  if (init.isPending) return <Centered><p className="text-muted-foreground">{t('common.loading')}</p></Centered>
  if (init.isError) return <Centered><p className="text-destructive">{errorMessage(init.error)}</p></Centered>

  if (init.data.status === 'new') {
    return (
      <Centered>
        <Card className="w-[34rem]">
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Sparkles className="size-5" />
              {t('setup.welcomeTitle')}
            </CardTitle>
            <CardDescription>{t('setup.welcomeBody')}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {setup.isError && <p className="text-sm text-destructive">{errorMessage(setup.error)}</p>}
            <div className="flex flex-wrap gap-2">
              <Button onClick={() => setup.mutate()} disabled={setup.isPending}>
                {t('setup.newSetup')}
              </Button>
              <DemoEnterButton />
            </div>
            <div className="space-y-2 border-t pt-4">
              <p className="text-sm text-muted-foreground">{t('setup.restoreHint')}</p>
              <RestoreBackup replacesData={false} />
            </div>
          </CardContent>
        </Card>
      </Centered>
    )
  }

  if (init.data.status === 'recovery_required') {
    return (
      <Centered>
        <Card className="w-[34rem]">
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <ShieldAlert className="size-5" />
              {t('setup.recoveryRequiredTitle')}
            </CardTitle>
            <CardDescription>{t('setup.recoveryRequiredBody')}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            <Label htmlFor="rk">{t('setup.recoveryInput')}</Label>
            <Input id="rk" className="font-mono" value={recoveryInput} onChange={(e) => setRecoveryInput(e.target.value)} />
            {recover.isError && <p className="text-sm text-destructive">{errorMessage(recover.error)}</p>}
            <Button onClick={() => recover.mutate()} disabled={recover.isPending}>
              {t('setup.recover')}
            </Button>
            <div className="space-y-2 border-t pt-4">
              <p className="text-sm text-muted-foreground">{t('setup.recoverFromBackupHint')}</p>
              <RestoreBackup replacesData={false} />
            </div>
          </CardContent>
        </Card>
      </Centered>
    )
  }

  // Fresh key (first run, or replacing one that was never confirmed): show it once with the kit.
  const freshKey = init.data.recovery_key ?? rotatedKey
  if (freshKey && !init.data.recovery_confirmed) {
    return (
      <Centered>
        <Card className="w-[38rem]">
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <KeyRound className="size-5" />
              {t('setup.recoveryTitle')}
            </CardTitle>
            <CardDescription>{t('setup.recoveryBody')}</CardDescription>
          </CardHeader>
          <CardContent>
            <RecoveryKitPanel
              recoveryKey={freshKey}
              rotated={rotatedKey !== null}
              onDone={() => setRotatedKey(null)}
              doneLabel={t('common.open')}
            />
          </CardContent>
        </Card>
      </Centered>
    )
  }

  if (!init.data.recovery_confirmed) {
    return (
      <Centered>
        <Card className="w-[34rem]">
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <ShieldAlert className="size-5" />
              {t('setup.unconfirmedTitle')}
            </CardTitle>
            <CardDescription>{t('setup.unconfirmedBody')}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            {rotate.isError && <p className="text-sm text-destructive">{errorMessage(rotate.error)}</p>}
            <Button onClick={() => rotate.mutate()} disabled={rotate.isPending}>
              <KeyRound /> {t('recoveryKit.rotate')}
            </Button>
          </CardContent>
        </Card>
      </Centered>
    )
  }

  return <>{children}</>
}

function Centered({ children }: { children: ReactNode }) {
  return <div className="flex min-h-screen items-center justify-center p-6">{children}</div>
}
