import { useState } from 'react'
import { Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Wallet } from 'lucide-react'
import { ExternalButton } from '@/components/key-guide'
import { Button } from '@/components/ui/button'
import { q } from '@/lib/api'
import { dollars, lowBalance, WARN_SETTING } from '@/lib/costs'

/**
 * Banner above every page while the OpenRouter balance (or the key's limit) is below the
 * threshold from Settings. Only known with a management key or a key limit. "Später" hides it
 * until the next start.
 */
export function BalanceReminder() {
  const { t } = useTranslation()
  const [dismissed, setDismissed] = useState(false)
  const llm = useQuery(q.llm())
  const openrouter = llm.data?.providers.find((p) => p.id === 'openrouter')
  const balance = useQuery(q.llmBalance(!!openrouter?.has_key))
  const warn = useQuery(q.setting<number>(WARN_SETTING))
  if (dismissed || !openrouter?.has_key || warn.isPending) return null
  const low = lowBalance(balance.data, warn.data)
  if (low == null) return null

  return (
    <div role="status" className="flex flex-wrap items-center gap-3 border-b bg-warning/15 px-8 py-2.5 text-sm" data-testid="balance-reminder">
      <Wallet className="size-4 shrink-0" />
      <span>{t('costs.low', { usd: dollars(low) })}</span>
      <span className="ml-auto flex items-center gap-3">
        {openrouter.credits_url && <ExternalButton url={openrouter.credits_url}>{t('costs.topUp')}</ExternalButton>}
        <Button size="sm" variant="ghost" asChild>
          <Link to="/settings" hash="costs">
            {t('costs.details')}
          </Link>
        </Button>
        <Button size="sm" variant="ghost" onClick={() => setDismissed(true)}>
          {t('backup.reminderLater')}
        </Button>
      </span>
    </div>
  )
}
