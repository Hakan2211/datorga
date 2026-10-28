import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { AlertTriangle, Trash2 } from 'lucide-react'
import type { DiarizeConfig } from '@/bindings/DiarizeConfig'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import { api, errorMessage } from '@/lib/api'

/** Settings → Sprechertrennung: on/off, learning her voice, and her voice profile (slice 27). */
export function SpeakerCard() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const overview = useQuery({ queryKey: ['diarize'], queryFn: api.diarize })
  const refresh = () => void qc.invalidateQueries({ queryKey: ['diarize'] })
  const save = useMutation({
    mutationFn: (config: DiarizeConfig) => api.setDiarizeConfig(config),
    onSuccess: refresh,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const remove = useMutation({
    mutationFn: api.deleteVoiceProfile,
    onSuccess: () => {
      refresh()
      toast.success(t('settings.stt.speakers.deleted'))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const data = overview.data
  if (!data) return null
  const { config, profile } = data
  const prints = profile.dictations + profile.sessions

  return (
    <Card className="max-w-2xl" data-testid="speaker-card">
      <CardHeader>
        <CardTitle className="text-base">{t('settings.stt.speakers.title')}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4 text-sm">
        <p className="text-muted-foreground">{t('settings.stt.speakers.intro')}</p>
        {!data.engine && (
          <p className="flex items-center gap-2 text-destructive">
            <AlertTriangle className="size-4" /> {t('settings.stt.speakers.missing')}
          </p>
        )}
        <label className="flex items-center gap-2">
          <Checkbox
            checked={config.enabled}
            disabled={!data.engine || save.isPending}
            onCheckedChange={(v) => save.mutate({ ...config, enabled: v === true })}
          />
          {t('settings.stt.speakers.enabled')}
        </label>
        <div className="space-y-1">
          <label className="flex items-center gap-2">
            <Checkbox
              checked={config.learn}
              disabled={!data.engine || save.isPending}
              onCheckedChange={(v) => save.mutate({ ...config, learn: v === true })}
            />
            {t('settings.stt.speakers.learn')}
          </label>
          <p className="pl-6 text-xs text-muted-foreground">{t('settings.stt.speakers.learnHint')}</p>
        </div>
        <div className="flex flex-wrap items-center gap-3 rounded-md border p-3">
          <div className="min-w-0 flex-1">
            <p className="font-medium">{t('settings.stt.speakers.profile')}</p>
            <p className="text-muted-foreground" data-testid="voice-profile">
              {prints === 0
                ? t('settings.stt.speakers.profileNone')
                : t('settings.stt.speakers.profileSome', {
                    dictations: profile.dictations,
                    sessions: profile.sessions,
                    minutes: Math.max(1, Math.round(profile.seconds / 60)),
                  })}
            </p>
          </div>
          {prints > 0 && (
            <Button size="sm" variant="outline" disabled={remove.isPending} onClick={() => remove.mutate()}>
              <Trash2 /> {t('settings.stt.speakers.deleteProfile')}
            </Button>
          )}
        </div>
        {data.engine && <p className="text-xs text-muted-foreground">{t('settings.stt.engine')}: {data.engine}</p>}
      </CardContent>
    </Card>
  )
}
