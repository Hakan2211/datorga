// Settings → "KI-Kosten & Guthaben" (slice 33): balance per provider, spend per task/model from
// the audit log, a 30-day sparkline and the low-balance threshold.
import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { AlertTriangle, KeyRound, Loader2, RefreshCw, Wallet } from 'lucide-react'
import type { SpendReport } from '@/bindings/SpendReport'
import type { DaySpend } from '@/bindings/DaySpend'
import { SectionCard } from '@/components/bits'
import { ExternalButton, KeyGuide, type GuideProvider } from '@/components/key-guide'
import { money } from '@/components/model-picker'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { api, errorMessage, q, type LlmOverview } from '@/lib/api'
import { DEFAULT_WARN_USD, dollars, last30, remaining, WARN_SETTING } from '@/lib/costs'
import { HELP } from '@/lib/help'

const CLOUD: GuideProvider[] = ['openrouter', 'requesty', 'openai']

/** Spend: "—" when nothing was billed (local model, provider without cost reports). */
const spent = (usd: number) => (usd > 0 ? money(usd) : '—')

export function CostsCard() {
  const { t } = useTranslation()
  const llm = useQuery(q.llm())
  const [all, setAll] = useState(false)
  const costs = useQuery(q.llmCosts(all))
  const withKey = CLOUD.filter((id) => llm.data?.providers.find((p) => p.id === id)?.has_key)
  const without = CLOUD.filter((id) => !withKey.includes(id))
  // Kept here, not in the list below: storing the key moves the provider out of that list.
  const [guide, setGuide] = useState<GuideProvider | null>(null)
  const guideLinks = (
    <span className="flex flex-wrap gap-x-4 gap-y-1">
      {without.map((id) => (
        <button key={id} type="button" className="text-primary underline" onClick={() => setGuide(id)} data-testid={`key-guide-${id}`}>
          {t('keyGuide.title', { provider: t(`keyGuide.${id}.name`) })}
        </button>
      ))}
    </span>
  )

  return (
    <SectionCard
      help={HELP.ai}
      icon={Wallet}
      title={t('costs.title')}
      description={t('costs.intro')}
      className="max-w-2xl"
      contentClassName="space-y-5 text-sm"
    >
      {llm.data && withKey.length === 0 && (
        <div className="space-y-2 rounded-md border border-dashed p-3" data-testid="costs-no-provider">
          <p>{t('costs.noProvider')}</p>
          {guideLinks}
        </div>
      )}
      {llm.data && withKey.length > 0 && (
        <div className="space-y-2">
          <div className="grid gap-3 sm:grid-cols-2">
            {withKey.map((id) =>
              id === 'openrouter' ? <OpenrouterTile key={id} data={llm.data} /> : <LinkTile key={id} id={id} data={llm.data} />,
            )}
          </div>
          {without.length > 0 && (
            <p className="flex flex-wrap gap-x-2 text-xs text-muted-foreground" data-testid="costs-more-providers">
              {t('costs.moreProviders')} {guideLinks}
            </p>
          )}
        </div>
      )}
      {guide && <KeyGuide provider={guide} open onOpenChange={(open) => !open && setGuide(null)} />}

      {costs.data && (
        <>
          <p data-testid="costs-totals">
            {t('costs.totals', {
              month: spent(costs.data.month.usd),
              requests: costs.data.month.requests,
              total: spent(costs.data.total.usd),
            })}
          </p>
          {costs.data.total.requests === 0 ? (
            <p className="text-muted-foreground">{t('costs.none')}</p>
          ) : (
            <>
              <ToggleGroup
                type="single"
                variant="outline"
                size="sm"
                value={all ? 'all' : 'month'}
                onValueChange={(v) => v && setAll(v === 'all')}
                aria-label={t('costs.period')}
              >
                <ToggleGroupItem value="month">{t('costs.month')}</ToggleGroupItem>
                <ToggleGroupItem value="all">{t('costs.all')}</ToggleGroupItem>
              </ToggleGroup>
              <Breakdown report={costs.data.breakdown} />
              <Sparkline days={costs.data.days} />
              <p className="text-xs text-muted-foreground">{t('costs.footnote')}</p>
            </>
          )}
        </>
      )}
      {withKey.includes('openrouter') && <WarnThreshold />}
    </SectionCard>
  )
}

function OpenrouterTile({ data }: { data: LlmOverview }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const balance = useQuery(q.llmBalance(true))
  const [mgmt, setMgmt] = useState('')
  const saveMgmt = useMutation({
    mutationFn: (k: string | null) => api.setMgmtKey(k),
    onSuccess: (_, k) => {
      setMgmt('')
      void qc.invalidateQueries({ queryKey: ['balance'] })
      toast.success(k ? t('costs.mgmt.saved') : t('costs.mgmt.removed'))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const b = balance.data
  const left = remaining(b)
  const creditsUrl = data.providers.find((p) => p.id === 'openrouter')?.credits_url

  return (
    <section className="space-y-2 rounded-md border p-3" data-testid="balance-openrouter">
      <div className="flex items-start justify-between gap-2">
        <p className="font-medium">OpenRouter</p>
        <Button
          size="icon"
          variant="ghost"
          className="size-7"
          aria-label={t('costs.refresh')}
          title={t('costs.refresh')}
          disabled={balance.isFetching}
          onClick={() => void balance.refetch()}
        >
          <RefreshCw className={balance.isFetching ? 'animate-spin' : ''} />
        </Button>
      </div>
      {balance.isPending && <Loader2 className="size-4 animate-spin text-muted-foreground" />}
      {balance.isError && (
        <p role="alert" className="text-destructive">
          {errorMessage(balance.error)}
        </p>
      )}
      {b && (
        <>
          {left ? (
            <p data-testid="balance-amount">
              <span className="text-xs text-muted-foreground">{t(left.kind === 'account' ? 'costs.balance' : 'costs.limitLeft')}</span>
              <br />
              <span className="text-xl font-semibold text-primary">{dollars(left.usd)}</span>
              {left.kind === 'limit' && b.key.limit != null && (
                <span className="text-xs text-muted-foreground"> {t('costs.ofLimit', { limit: dollars(b.key.limit) })}</span>
              )}
            </p>
          ) : (
            <p className="text-muted-foreground" data-testid="balance-browser-only">
              {t('costs.balanceBrowser')}
            </p>
          )}
          <p className="text-xs text-muted-foreground">{t('costs.keyMonth', { usd: dollars(b.key.usage_monthly) })}</p>
          {b.key.is_free_tier && <p className="text-xs text-warning">{t('keyGuide.freeTier')}</p>}
          {b.credits_error && (
            <p role="alert" className="text-xs text-destructive">
              {t('costs.mgmt.failed', { error: b.credits_error })}
            </p>
          )}
        </>
      )}
      {creditsUrl && (
        <ExternalButton url={creditsUrl} className="font-medium">
          {t('costs.topUp')}
        </ExternalButton>
      )}
      {b && (
        <details className="text-xs" open={!!b.credits_error}>
          <summary className="cursor-pointer text-muted-foreground">
            {b.has_mgmt_key ? t('costs.mgmt.stored') : t('costs.mgmt.show')}
          </summary>
          <div className="mt-2 space-y-2">
            <p className="text-muted-foreground">
              {t('costs.mgmt.hint')}{' '}
              <ExternalButton url={b.mgmt_key_url}>{t('costs.mgmt.create')}</ExternalButton>
            </p>
            <Label htmlFor="mgmt-key" className="flex items-center gap-1.5 text-xs">
              <KeyRound className="size-3.5" /> {t('costs.mgmt.label')}
            </Label>
            <div className="flex gap-2">
              <Input
                id="mgmt-key"
                type="password"
                autoComplete="off"
                className="h-8"
                value={mgmt}
                placeholder={b.has_mgmt_key ? '••••••••' : 'sk-or-v1-…'}
                onChange={(e) => setMgmt(e.target.value)}
              />
              <Button size="sm" variant="outline" disabled={!mgmt.trim() || saveMgmt.isPending} onClick={() => saveMgmt.mutate(mgmt)}>
                {t('common.save')}
              </Button>
              {b.has_mgmt_key && (
                <Button size="sm" variant="ghost" disabled={saveMgmt.isPending} onClick={() => saveMgmt.mutate(null)}>
                  {t('settings.llm.keyRemove')}
                </Button>
              )}
            </div>
          </div>
        </details>
      )}
    </section>
  )
}

/** Requesty and OpenAI: the balance needs an admin key there, so only links. */
function LinkTile({ id, data }: { id: GuideProvider; data: LlmOverview }) {
  const { t } = useTranslation()
  const info = data.providers.find((p) => p.id === id)!
  return (
    <section className="space-y-2 rounded-md border p-3" data-testid={`balance-${id}`}>
      <p className="font-medium">{t(`costs.tile.${id}`)}</p>
      <p className="text-muted-foreground">{t('costs.browserOnly')}</p>
      {info.credits_url && (
        <ExternalButton url={info.credits_url} className="font-medium">
          {t('costs.topUpBrowser')}
        </ExternalButton>
      )}
    </section>
  )
}

function Breakdown({ report }: { report: SpendReport }) {
  const { t } = useTranslation()
  const catalog = useQuery(q.llmCatalog())
  const label = (id: string) => catalog.data?.models.find((m) => m.id === id)?.label ?? id
  // Bars by cost; when nothing was billed (local only) by number of requests.
  const byCost = report.tasks.some((r) => r.usd > 0)
  const value = (r: { usd: number; requests: number }) => (byCost ? r.usd : r.requests)
  const max = Math.max(...report.tasks.map(value), 0)

  return (
    <div className="space-y-5">
      <section className="space-y-2" data-testid="costs-by-task">
        <h3 className="text-sm font-medium">{t('costs.byTask')}</h3>
        <ul className="space-y-2">
          {report.tasks.map((r) => (
            <li
              key={r.task}
              className="grid grid-cols-[9rem_1fr_4.5rem] items-center gap-2"
              title={t('costs.requests', { count: r.requests })}
            >
              <span className="truncate">{t(`costs.task.${r.task}`)}</span>
              <span className="h-2 rounded-full bg-muted">
                <span
                  className="block h-2 rounded-full bg-primary/80"
                  style={{ width: `${max > 0 ? Math.max(3, (value(r) / max) * 100) : 0}%` }}
                />
              </span>
              <span className="text-right tabular-nums">{spent(r.usd)}</span>
            </li>
          ))}
        </ul>
      </section>
      <section className="space-y-2" data-testid="costs-by-model">
        <h3 className="text-sm font-medium">{t('costs.byModel')}</h3>
        <table className="w-full text-[13px]">
          <thead className="text-xs text-muted-foreground">
            <tr>
              <th className="pb-1 text-left font-normal">{t('costs.model')}</th>
              <th className="pb-1 text-right font-normal">{t('costs.requestsHead')}</th>
              <th className="pb-1 text-right font-normal">{t('costs.cost')}</th>
            </tr>
          </thead>
          <tbody>
            {report.models.map((m) => (
              <tr key={`${m.provider}/${m.model}`} className="border-t">
                <td className="py-1 pr-2" title={`${m.model}${m.provider ? ` · ${m.provider}` : ''}`}>
                  {label(m.model)}
                  {m.provider === 'local' && <span className="text-muted-foreground"> · {t('costs.local')}</span>}
                </td>
                <td className="py-1 text-right tabular-nums">{m.requests}</td>
                <td className="py-1 text-right tabular-nums">{spent(m.usd)}</td>
              </tr>
            ))}
            <tr className="border-t font-medium">
              <td className="py-1">{t('costs.sum')}</td>
              <td className="py-1 text-right tabular-nums">{report.total.requests}</td>
              <td className="py-1 text-right tabular-nums">{spent(report.total.usd)}</td>
            </tr>
          </tbody>
        </table>
      </section>
    </div>
  )
}

const W = 600
const H = 80
const PAD = 6

function Sparkline({ days }: { days: DaySpend[] }) {
  const { t } = useTranslation()
  const series = last30(days)
  const max = Math.max(...series.map((d) => d.usd), 0)
  const step = (W - 2 * PAD) / (series.length - 1)
  const x = (i: number) => PAD + i * step
  const y = (usd: number) => (max > 0 ? H - PAD - (usd / max) * (H - 2 * PAD) : H - PAD)
  const points = series.map((d, i) => `${x(i)},${y(d.usd)}`).join(' ')
  const dayLabel = (day: string) => new Date(`${day}T12:00:00Z`).toLocaleDateString('de-DE', { day: 'numeric', month: 'numeric' })
  const [hover, setHover] = useState<number | null>(null)
  const h = hover != null ? series[hover] : null

  return (
    <section className="space-y-1" data-testid="costs-sparkline">
      <div className="flex items-baseline justify-between">
        <h3 className="text-sm font-medium">{t('costs.trend')}</h3>
        <span className="text-xs text-muted-foreground tabular-nums">
          {h ? t('costs.trendPoint', { day: dayLabel(h.day), cost: spent(h.usd), count: h.requests }) : t('costs.trendMax', { cost: spent(max) })}
        </span>
      </div>
      <svg viewBox={`0 0 ${W} ${H}`} className="h-20 w-full" preserveAspectRatio="none" role="img" aria-label={t('costs.trend')}>
        <line x1={PAD} x2={W - PAD} y1={H - PAD} y2={H - PAD} className="stroke-border" strokeWidth={1} vectorEffect="non-scaling-stroke" />
        <polyline points={points} fill="none" className="stroke-primary" strokeWidth={2} strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
        {h && hover != null && (
          <line x1={x(hover)} x2={x(hover)} y1={PAD} y2={H - PAD} className="stroke-muted-foreground/50" strokeWidth={1} vectorEffect="non-scaling-stroke" />
        )}
        {series.map((d, i) => (
          <rect
            key={d.day}
            x={x(i) - step / 2}
            y={0}
            width={step}
            height={H}
            fill="transparent"
            onMouseEnter={() => setHover(i)}
            onMouseLeave={() => setHover(null)}
          >
            <title>{t('costs.trendPoint', { day: dayLabel(d.day), cost: spent(d.usd), count: d.requests })}</title>
          </rect>
        ))}
      </svg>
      <div className="flex justify-between text-xs text-muted-foreground">
        <span>{t('costs.ago30')}</span>
        <span>{t('costs.today')}</span>
      </div>
    </section>
  )
}

function WarnThreshold() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const warn = useQuery(q.setting<number>(WARN_SETTING))
  const [edit, setEdit] = useState<string | null>(null)
  const save = useMutation({
    mutationFn: (v: number) => api.setSetting(WARN_SETTING, v),
    onSuccess: () => {
      setEdit(null)
      void qc.invalidateQueries({ queryKey: ['setting', WARN_SETTING] })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  if (warn.isPending) return null
  const value = warn.data ?? DEFAULT_WARN_USD
  const parsed = edit == null ? NaN : Number(edit.replace(',', '.'))

  return (
    <div className="flex flex-wrap items-center gap-3 rounded-md border border-warning/40 bg-warning/10 p-3" data-testid="warn-threshold">
      <AlertTriangle className="size-4 shrink-0 text-warning" />
      {edit == null ? (
        <>
          <span className="flex-1">{value > 0 ? t('costs.warnAt', { usd: dollars(value) }) : t('costs.warnOff')}</span>
          <Button size="sm" variant="outline" onClick={() => setEdit(String(value).replace('.', ','))}>
            {t('common.edit')}
          </Button>
        </>
      ) : (
        <form
          className="flex flex-1 items-center gap-2"
          onSubmit={(e) => {
            e.preventDefault()
            if (Number.isFinite(parsed) && parsed >= 0) save.mutate(Math.round(parsed * 100) / 100)
          }}
        >
          <Label htmlFor="warn-usd">{t('costs.warnLabel')}</Label>
          <Input id="warn-usd" inputMode="decimal" className="h-8 w-24" value={edit} onChange={(e) => setEdit(e.target.value)} autoFocus />
          <span>$</span>
          <Button size="sm" type="submit" disabled={!Number.isFinite(parsed) || parsed < 0 || save.isPending}>
            {t('common.save')}
          </Button>
          <Button size="sm" variant="ghost" type="button" onClick={() => setEdit(null)}>
            {t('common.cancel')}
          </Button>
          <span className="text-xs text-muted-foreground">{t('costs.warnZero')}</span>
        </form>
      )}
    </div>
  )
}
