import { useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { open } from '@tauri-apps/plugin-dialog'
import { AlertTriangle } from 'lucide-react'
import { ModelRow } from '@/components/model-row'
import { SpeakerCard } from '@/components/speaker-settings'
import { VoiceCard } from '@/components/voice-settings'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { api, errorMessage, q, type SttOverview } from '@/lib/api'
import type { SttConfig } from '@/bindings/SttConfig'
import type { SttMode } from '@/bindings/SttMode'

export const Route = createFileRoute('/settings/sprache')({ component: Speech })

function Speech() {
  return (
    <>
      <SttCard />
      <SpeakerCard />
      <VoiceCard />
    </>
  )
}

const STT_MODES: SttMode[] = ['local', 'cloud', 'fallback']

/** Local vs. cloud for own dictations. Session recordings always stay local (backend rule). */
function SttEngineChoice({ data, onSaved }: { data: SttOverview; onSaved: () => void }) {
  const { t } = useTranslation()
  const config = data.config
  const [customModel, setCustomModel] = useState(config.cloud_model ?? '')
  const save = useMutation({
    mutationFn: (c: SttConfig) => api.setSttConfig(c),
    onSuccess: onSaved,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const set = (patch: Partial<SttConfig>) => save.mutate({ ...config, ...patch })
  const cloud = config.mode !== 'local'
  const openrouter = config.cloud_provider === 'openrouter'
  const model = config.cloud_model ?? (openrouter ? data.cloud_models[0]?.id : '')

  return (
    <section className="space-y-3 rounded-md border p-3" data-testid="stt-engine">
      <div className="space-y-1.5">
        <Label>{t('settings.stt.where')}</Label>
        <Select value={config.mode} onValueChange={(v) => set({ mode: v as SttMode })}>
          <SelectTrigger className="w-80">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {STT_MODES.map((m) => (
              <SelectItem key={m} value={m}>
                {t(`settings.stt.mode.${m}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-[13px] text-muted-foreground">{t(`settings.stt.modeHint.${config.mode}`)}</p>
      </div>
      {cloud && (
        <>
          <p className="flex items-start gap-2 rounded-md bg-warning/15 px-3 py-2 text-[13px]">
            <AlertTriangle className="mt-0.5 size-4 shrink-0" /> {t('settings.stt.cloudWarning')}
          </p>
          <div className="flex flex-wrap gap-3">
            <div className="space-y-1.5">
              <Label>{t('settings.stt.provider')}</Label>
              <Select
                value={config.cloud_provider}
                onValueChange={(v) => set({ cloud_provider: v as SttConfig['cloud_provider'], cloud_model: null })}
              >
                <SelectTrigger className="w-64">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {(['openrouter', 'custom'] as const).map((p) => (
                    <SelectItem key={p} value={p}>
                      {t(`settings.stt.providers.${p}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="min-w-64 flex-1 space-y-1.5">
              <Label>{t('settings.stt.model')}</Label>
              {openrouter ? (
                <Select value={model ?? ''} onValueChange={(v) => set({ cloud_model: v })}>
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {data.cloud_models.map((m) => (
                      <SelectItem key={m.id} value={m.id}>
                        {m.label}
                        <span className="text-xs text-muted-foreground">
                          {' '}
                          ·{' '}
                          {m.usd_per_hour == null
                            ? t('settings.stt.priceUnknown')
                            : t('settings.stt.perHour', { price: m.usd_per_hour.toFixed(m.usd_per_hour < 0.1 ? 3 : 2) })}
                        </span>
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : (
                <Input
                  value={customModel}
                  placeholder={t('settings.stt.modelPlaceholder')}
                  onChange={(e) => setCustomModel(e.target.value)}
                  onBlur={() => customModel.trim() !== (config.cloud_model ?? '') && set({ cloud_model: customModel.trim() || null })}
                />
              )}
            </div>
          </div>
          {openrouter && (
            <p className="text-xs text-muted-foreground">
              {t('settings.stt.zdr', { via: data.cloud_models.find((m) => m.id === model)?.zdr_via ?? '–' })}
            </p>
          )}
          {data.cloud_problem && (
            <p className="flex items-center gap-2 text-destructive">
              <AlertTriangle className="size-4" /> {data.cloud_problem}
            </p>
          )}
          {config.mode === 'fallback' && (
            <p className="text-[13px] text-muted-foreground">
              {data.local_ready ? t('settings.stt.fallbackLocal') : t('settings.stt.fallbackCloud')}
            </p>
          )}
        </>
      )}
    </section>
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
        {data && <SttEngineChoice data={data} onSaved={refresh} />}
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
