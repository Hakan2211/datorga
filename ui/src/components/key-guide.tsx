// Key guide (slice 33): step by step from "no account" to a tested key, per cloud provider.
// Opens from the key fields and the costs card (later also from Hilfe).
import { useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { openUrl } from '@tauri-apps/plugin-opener'
import { CheckCircle2, ExternalLink, KeyRound, Loader2, PlugZap, ShieldCheck } from 'lucide-react'
import type { ProviderId } from '@/bindings/ProviderId'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { api, errorMessage, q } from '@/lib/api'
import { cn } from '@/lib/utils'

export type GuideProvider = Extract<ProviderId, 'openrouter' | 'requesty' | 'openai'>

/** A link-styled button that opens `url` in the browser. */
export function ExternalButton({ url, children, className }: { url: string; children: ReactNode; className?: string }) {
  return (
    <button type="button" className={cn('inline-flex items-center gap-1 text-primary underline-offset-2 hover:underline', className)} onClick={() => void openUrl(url)}>
      {children} <ExternalLink className="size-3" />
    </button>
  )
}

/** "Schritt-für-Schritt-Anleitung" link that opens the guide. */
export function KeyGuideButton({ provider, className }: { provider: GuideProvider; className?: string }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  return (
    <>
      <button type="button" className={cn('text-xs underline', className)} onClick={() => setOpen(true)} data-testid={`key-guide-${provider}`}>
        {t('keyGuide.open')}
      </button>
      <KeyGuide provider={provider} open={open} onOpenChange={setOpen} />
    </>
  )
}

export function KeyGuide({ provider, open, onOpenChange }: { provider: GuideProvider; open: boolean; onOpenChange: (open: boolean) => void }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const llm = useQuery(q.llm())
  const info = llm.data?.providers.find((p) => p.id === provider)
  const [key, setKey] = useState('')
  const [checked, setChecked] = useState<{ free_tier: boolean } | null>(null)

  const store = useMutation({
    mutationFn: () => api.setLlmKey(provider, key),
    onSuccess: () => {
      setKey('')
      setChecked(null)
      void qc.invalidateQueries({ queryKey: ['llm'] })
      void qc.invalidateQueries({ queryKey: ['balance'] })
      toast.success(t('settings.llm.keySaved'))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const check = useMutation({
    mutationFn: () => api.checkLlmKey(provider),
    onMutate: () => setChecked(null),
    onSuccess: setChecked,
  })

  if (!info) return null
  const hasKey = info.has_key
  const steps: { title: ReactNode; body: ReactNode; done?: boolean }[] = [
    {
      title: info.signup_url ? <ExternalButton url={info.signup_url}>{t('keyGuide.signup')}</ExternalButton> : t('keyGuide.signup'),
      body: t(`keyGuide.${provider}.signup`),
    },
    {
      title: info.credits_url ? <ExternalButton url={info.credits_url}>{t('keyGuide.credits')}</ExternalButton> : t('keyGuide.credits'),
      body: t(`keyGuide.${provider}.credits`),
    },
    {
      title: info.key_url ? <ExternalButton url={info.key_url}>{t('keyGuide.create')}</ExternalButton> : t('keyGuide.create'),
      body: t(`keyGuide.${provider}.create`),
    },
    {
      title: t('keyGuide.paste'),
      done: hasKey,
      body: (
        <div className="space-y-2">
          <p>{hasKey ? t('keyGuide.pasteStored') : t('keyGuide.pasteHint')}</p>
          <form
            className="flex gap-2"
            onSubmit={(e) => {
              e.preventDefault()
              if (key.trim()) store.mutate()
            }}
          >
            <Input
              type="password"
              autoComplete="off"
              aria-label={t('settings.llm.key')}
              value={key}
              placeholder={hasKey ? '••••••••' : 'sk-…'}
              onChange={(e) => setKey(e.target.value)}
              data-testid="key-guide-input"
            />
            <Button type="submit" variant="outline" disabled={!key.trim() || store.isPending}>
              <KeyRound /> {t('common.save')}
            </Button>
          </form>
        </div>
      ),
    },
    {
      title: t('keyGuide.test'),
      done: !!checked,
      body: (
        <div className="space-y-2">
          <p>{t('keyGuide.testHint')}</p>
          <Button size="sm" disabled={!hasKey || check.isPending} onClick={() => check.mutate()} data-testid="key-guide-test">
            {check.isPending ? <Loader2 className="animate-spin" /> : <PlugZap />} {t('settings.llm.test')}
          </Button>
          {checked && (
            <p role="status" className="flex gap-2 rounded-md border border-success/40 bg-success/10 p-2.5 text-foreground">
              <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-success" />
              <span>
                <span className="font-medium">{t('keyGuide.testOk')}</span>
                <br />
                {checked.free_tier ? t('keyGuide.freeTier') : t(`keyGuide.${provider}.ready`)}
              </span>
            </p>
          )}
          {check.isError && (
            <p role="alert" className="rounded-md border border-destructive/40 bg-destructive/10 p-2.5 text-foreground">
              {errorMessage(check.error)}
            </p>
          )}
        </div>
      ),
    },
  ]

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl" data-testid="key-guide">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <KeyRound className="size-4" /> {t('keyGuide.title', { provider: t(`keyGuide.${provider}.name`) })}
          </DialogTitle>
          <DialogDescription>{t('keyGuide.intro')}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-6 sm:grid-cols-[1fr_15rem]">
          <ol className="space-y-0">
            {steps.map((s, i) => (
              <li key={i} className="relative flex gap-3 pb-5 last:pb-0">
                {i < steps.length - 1 && <span aria-hidden className="absolute top-8 bottom-0 left-[0.9375rem] w-px bg-border" />}
                <span
                  className={cn(
                    'z-10 flex size-8 shrink-0 items-center justify-center rounded-full border text-sm font-medium',
                    s.done ? 'border-primary bg-primary text-primary-foreground' : 'bg-card',
                  )}
                >
                  {s.done ? <CheckCircle2 className="size-4" /> : i + 1}
                </span>
                <div className="min-w-0 flex-1 space-y-1 pt-1 text-sm">
                  <p className="font-medium">{s.title}</p>
                  <div className="text-[13px] text-muted-foreground">{s.body}</div>
                </div>
              </li>
            ))}
          </ol>
          <aside className="h-fit space-y-2 rounded-md border border-warning/40 bg-warning/10 p-4 text-[13px]">
            <p className="flex items-center gap-2 font-medium">
              <ShieldCheck className="size-4 text-primary" /> {t('keyGuide.privacy')}
            </p>
            <p>{t(`keyGuide.${provider}.privacy`)}</p>
          </aside>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('common.close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
