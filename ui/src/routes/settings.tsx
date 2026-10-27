import { useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Lock, LockOpen } from 'lucide-react'
import { Page, PageHeader } from '@/components/app-shell'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage, q } from '@/lib/api'

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
        <Card className="max-w-2xl">
          <CardHeader>
            <CardTitle className="text-base">{t('settings.security')}</CardTitle>
          </CardHeader>
          <CardContent className="flex items-center gap-2 text-sm">
            {init.data?.encrypted ? (
              <>
                <Lock className="size-4 text-success" /> {t('privacy.encrypted')} (SQLCipher, AES-256)
              </>
            ) : (
              <>
                <LockOpen className="size-4 text-warning" /> {t('privacy.notEncrypted')}
              </>
            )}
          </CardContent>
        </Card>
      </Page>
    </>
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
