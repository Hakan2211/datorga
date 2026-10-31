import { useState } from 'react'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Building2, CircleHelp, Compass, Monitor, Moon, Palette, Sun, Workflow } from 'lucide-react'
import { DemoEnterButton } from '@/components/demo'
import { UpdateCard } from '@/components/update'
import { useTour } from '@/components/tour'
import { SectionCard } from '@/components/bits'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { setTheme, THEMES, useTheme, type Theme } from '@/lib/theme'
import { api, errorMessage, q } from '@/lib/api'
import { HELP } from '@/lib/help'

export const Route = createFileRoute('/settings/allgemein')({ component: General })

function General() {
  const { t } = useTranslation()
  return (
    <>
      <SectionCard icon={Building2} title={t('settings.practiceTitle')} className="max-w-2xl" contentClassName="space-y-5">
        <SettingField settingKey="therapist_name" label={t('settings.therapist')} />
        <SettingField settingKey="practice_header" label={t('settings.practice')} multiline />
      </SectionCard>
      <AppearanceCard />
      <AutopilotCard />
      <UpdateCard />
      <HelpCard />
    </>
  )
}

function HelpCard() {
  const { t } = useTranslation()
  const tour = useTour()
  return (
    <SectionCard icon={CircleHelp} title={t('help.settingsTitle')} className="max-w-2xl" contentClassName="space-y-3 text-sm">
      <p className="text-muted-foreground">{t('help.settingsBody')}</p>
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" onClick={tour.start} data-testid="settings-tour-start">
          <Compass /> {t('tour.start')}
        </Button>
        <DemoEnterButton />
        <Button variant="ghost" asChild>
          <Link to="/hilfe">{t('help.open')}</Link>
        </Button>
      </div>
    </SectionCard>
  )
}

/** On by default (user decision); off = every step after the session is a button again. */
function AutopilotCard() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const on = useQuery(q.autopilot())
  const set = useMutation({
    mutationFn: (v: boolean) => api.setAutopilot(v),
    onSuccess: () => void qc.invalidateQueries({ queryKey: q.autopilot().queryKey }),
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <SectionCard
      icon={Workflow}
      title={t('autopilot.settingTitle')}
      help={HELP.autopilot}
      tour="autopilot"
      className="max-w-2xl"
      contentClassName="space-y-3 text-sm"
    >
      <div className="flex items-center gap-3">
        <Switch
          id="autopilot"
          checked={on.data ?? true}
          onCheckedChange={(v) => set.mutate(v)}
          disabled={on.isLoading || set.isPending}
          data-testid="autopilot-switch"
        />
        <Label htmlFor="autopilot">{t('autopilot.settingLabel')}</Label>
      </div>
      <p className="text-[13px] text-muted-foreground">{t('autopilot.settingHint')}</p>
    </SectionCard>
  )
}

const themeIcon = { hell: Sun, dunkel: Moon, system: Monitor } as const

function AppearanceCard() {
  const { t } = useTranslation()
  const { theme } = useTheme()
  return (
    <SectionCard icon={Palette} title={t('settings.appearance.title')} className="max-w-2xl" contentClassName="space-y-2">
      <Label>{t('settings.appearance.theme')}</Label>
      <ToggleGroup
        type="single"
        variant="outline"
        value={theme}
        onValueChange={(v) => v && setTheme(v as Theme)}
        aria-label={t('settings.appearance.theme')}
      >
        {THEMES.map((th) => {
          const Icon = themeIcon[th]
          return (
            <ToggleGroupItem key={th} value={th} className="px-4">
              <Icon /> {t(`settings.appearance.${th}`)}
            </ToggleGroupItem>
          )
        })}
      </ToggleGroup>
      <p className="text-xs text-muted-foreground">{t('settings.appearance.hint')}</p>
    </SectionCard>
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
