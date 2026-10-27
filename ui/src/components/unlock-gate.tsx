import { useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { KeyRound, ShieldAlert } from 'lucide-react'
import { api, errorMessage, isTauri } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'

/** Opens the encrypted store before anything else renders. Shows the recovery key once on first run. */
export function UnlockGate({ children }: { children: ReactNode }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const init = useQuery({ queryKey: ['init'], queryFn: api.init, enabled: isTauri(), staleTime: Infinity })
  const [acknowledged, setAcknowledged] = useState(false)
  const [checked, setChecked] = useState(false)
  const [recoveryInput, setRecoveryInput] = useState('')
  const recover = useMutation({
    mutationFn: () => api.recover(recoveryInput),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['init'] }),
  })

  if (!isTauri()) return <Centered><p className="text-muted-foreground">{t('setup.outsideTauri')}</p></Centered>
  if (init.isPending) return <Centered><p className="text-muted-foreground">{t('common.loading')}</p></Centered>
  if (init.isError) return <Centered><p className="text-destructive">{errorMessage(init.error)}</p></Centered>

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
          </CardContent>
        </Card>
      </Centered>
    )
  }

  if (init.data.recovery_key && !acknowledged) {
    const groups = init.data.recovery_key.split('-')
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
          <CardContent className="space-y-4">
            <div className="grid grid-cols-4 gap-x-4 gap-y-2 rounded-md bg-muted p-4 text-center font-mono text-base tracking-wider select-all">
              {groups.map((g, i) => (
                <span key={i}>{g}</span>
              ))}
            </div>
            <label className="flex items-center gap-2 text-sm">
              <Checkbox checked={checked} onCheckedChange={(v) => setChecked(v === true)} />
              {t('setup.recoveryConfirm')}
            </label>
            <Button disabled={!checked} onClick={() => setAcknowledged(true)}>
              {t('common.open')}
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
