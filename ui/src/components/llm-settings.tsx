// Settings → "KI-Entwürfe": provider, API key (OS keychain), endpoint, model, connection test.
import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { openUrl } from '@tauri-apps/plugin-opener'
import { AlertTriangle, CheckCircle2, ExternalLink, KeyRound, PlugZap, RefreshCw, ShieldCheck } from 'lucide-react'
import type { LlmConfig } from '@/bindings/LlmConfig'
import type { ProviderId } from '@/bindings/ProviderId'
import type { RemoteModel } from '@/bindings/RemoteModel'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { api, errorMessage, q, type LlmOverview } from '@/lib/api'

const OFF = 'off'

export function LlmCard() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const llm = useQuery(q.llm())
  const save = useMutation({
    mutationFn: (config: LlmConfig) => api.setLlmConfig(config),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['llm'] }),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const data = llm.data
  const active = data?.config.active ?? null

  return (
    <Card className="max-w-2xl">
      <CardHeader>
        <CardTitle className="text-base">{t('settings.llm.title')}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4 text-sm">
        <p className="flex gap-2 text-muted-foreground">
          <ShieldCheck className="mt-0.5 size-4 shrink-0 text-success" /> {t('settings.llm.intro')}
        </p>
        {data && (
          <div className="space-y-1.5">
            <Label>{t('settings.llm.provider')}</Label>
            <Select
              value={active ?? OFF}
              onValueChange={(v) => save.mutate({ ...data.config, active: v === OFF ? null : (v as ProviderId) })}
            >
              <SelectTrigger className="w-80">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={OFF}>{t('settings.llm.off')}</SelectItem>
                {data.providers.map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    {p.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}
        {data && active && <ProviderForm key={active} data={data} id={active} onSave={(c) => save.mutate(c)} />}
      </CardContent>
    </Card>
  )
}

function ProviderForm({ data, id, onSave }: { data: LlmOverview; id: ProviderId; onSave: (c: LlmConfig) => void }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const info = data.providers.find((p) => p.id === id)!
  const current = data.config.providers[id] ?? { model: null, base_url: null }
  const [key, setKey] = useState('')
  const [baseUrl, setBaseUrl] = useState(current.base_url ?? '')
  const [model, setModel] = useState(current.model ?? '')
  const [models, setModels] = useState<RemoteModel[]>([])
  const refresh = () => void qc.invalidateQueries({ queryKey: ['llm'] })

  const update = (patch: Partial<typeof current>) =>
    onSave({ ...data.config, providers: { ...data.config.providers, [id]: { ...current, ...patch } } })

  const storeKey = useMutation({
    mutationFn: (k: string | null) => api.setLlmKey(id, k),
    onSuccess: (_, k) => {
      setKey('')
      refresh()
      toast.success(k ? t('settings.llm.keySaved') : t('settings.llm.keyRemoved'))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const load = useMutation({
    mutationFn: () => api.llmModels(id),
    onSuccess: (list) => {
      setModels(list)
      toast.success(t('settings.llm.modelsLoaded', { count: list.length }))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const test = useMutation({
    mutationFn: () => api.testLlm(id),
    onSuccess: (r) => toast.success(t('settings.llm.testOk', { model: r.model, ms: r.latency_ms })),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const ready = (!info.needs_key || info.has_key) && !!current.model

  return (
    <div className="space-y-4">
      {info.us_intermediary && (
        <p className="flex gap-2 rounded-md border border-warning/40 bg-warning/10 p-3">
          <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" />
          <span>{t(`settings.llm.warning.${id}`)}</span>
        </p>
      )}

      <div className="space-y-1.5">
        <Label htmlFor="llm-key" className="flex items-center gap-2">
          <KeyRound className="size-4" /> {t('settings.llm.key')}
          {info.has_key && (
            <Badge variant="secondary" className="bg-success/15 font-normal text-success">
              <CheckCircle2 className="size-3" /> {t('settings.llm.keyStored')}
            </Badge>
          )}
        </Label>
        <div className="flex gap-2">
          <Input
            id="llm-key"
            type="password"
            autoComplete="off"
            value={key}
            placeholder={info.has_key ? '••••••••' : 'sk-…'}
            onChange={(e) => setKey(e.target.value)}
          />
          <Button variant="outline" disabled={!key.trim() || storeKey.isPending} onClick={() => storeKey.mutate(key)}>
            {t('common.save')}
          </Button>
          {info.has_key && (
            <Button variant="ghost" disabled={storeKey.isPending} onClick={() => storeKey.mutate(null)}>
              {t('settings.llm.keyRemove')}
            </Button>
          )}
        </div>
        <p className="text-xs text-muted-foreground">
          {t('settings.llm.keyHint')}
          {info.key_url && (
            <>
              {' '}
              <button type="button" className="inline-flex items-center gap-1 underline" onClick={() => void openUrl(info.key_url!)}>
                {t('settings.llm.keyCreate')} <ExternalLink className="size-3" />
              </button>
            </>
          )}
        </p>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="llm-base">{t('settings.llm.baseUrl')}</Label>
        <div className="flex gap-2">
          <Input
            id="llm-base"
            value={baseUrl}
            placeholder={info.default_base_url || 'https://…/v1'}
            onChange={(e) => setBaseUrl(e.target.value)}
          />
          <Button
            variant="outline"
            disabled={baseUrl === (current.base_url ?? '')}
            onClick={() => update({ base_url: baseUrl.trim() || null })}
          >
            {t('common.save')}
          </Button>
        </div>
        {info.default_base_url && <p className="text-xs text-muted-foreground">{t('settings.llm.baseUrlHint')}</p>}
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="llm-model">{t('settings.llm.model')}</Label>
        <div className="flex gap-2">
          <Input
            id="llm-model"
            list="llm-models"
            value={model}
            placeholder={t('settings.llm.modelPlaceholder')}
            onChange={(e) => setModel(e.target.value)}
          />
          <datalist id="llm-models">
            {models.map((m) => (
              <option key={m.id} value={m.id}>
                {m.name ?? m.id}
                {m.context_length ? ` · ${Math.round(m.context_length / 1000)}k` : ''}
              </option>
            ))}
          </datalist>
          <Button variant="outline" disabled={model.trim() === (current.model ?? '')} onClick={() => update({ model: model.trim() || null })}>
            {t('common.save')}
          </Button>
          <Button
            variant="ghost"
            disabled={load.isPending || (info.needs_key && !info.has_key)}
            onClick={() => load.mutate()}
            aria-label={t('settings.llm.loadModels')}
            title={t('settings.llm.loadModels')}
          >
            <RefreshCw className={load.isPending ? 'animate-spin' : ''} />
          </Button>
        </div>
      </div>

      <Button variant="outline" disabled={!ready || test.isPending} onClick={() => test.mutate()}>
        <PlugZap /> {test.isPending ? t('settings.llm.testing') : t('settings.llm.test')}
      </Button>
    </div>
  )
}
