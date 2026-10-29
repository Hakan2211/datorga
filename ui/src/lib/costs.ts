// AI costs and balance (slice 33): amounts, the 30-day series and the low-balance rule.
import type { DaySpend } from '@/bindings/DaySpend'
import type { BalanceView } from '@/lib/api'

export const WARN_SETTING = 'balance_warn_usd'
export const DEFAULT_WARN_USD = 2

/** Balances always in dollars with cents: "8,42 $". */
export function dollars(usd: number): string {
  return `${usd.toLocaleString('de-DE', { minimumFractionDigits: 2, maximumFractionDigits: 2 })} $`
}

/** The UTC day `YYYY-MM-DD` of `now`, minus `back` days. */
function utcDay(now: number, back: number): string {
  return new Date(now - back * 86_400_000).toISOString().slice(0, 10)
}

/** 30 points, oldest first, ending today (UTC, like the audit log); days without requests are 0. */
export function last30(days: DaySpend[], now = Date.now()): { day: string; usd: number; requests: number }[] {
  const byDay = new Map(days.map((d) => [d.day, d]))
  return Array.from({ length: 30 }, (_, i) => {
    const day = utcDay(now, 29 - i)
    const d = byDay.get(day)
    return { day, usd: d?.usd ?? 0, requests: d?.requests ?? 0 }
  })
}

/**
 * What is left to spend: the account balance (management key), else the key's own limit.
 * `null` when neither is known (no management key, unlimited key).
 */
export function remaining(b: BalanceView | undefined): { usd: number; kind: 'account' | 'limit' } | null {
  if (!b) return null
  if (b.credits) return { usd: b.credits.remaining, kind: 'account' }
  if (b.key.limit_remaining != null) return { usd: b.key.limit_remaining, kind: 'limit' }
  return null
}

/** Warn below the threshold; 0 (or less) turns the warning off. */
export function lowBalance(b: BalanceView | undefined, threshold: number | null | undefined): number | null {
  const t = threshold ?? DEFAULT_WARN_USD
  const r = remaining(b)
  return r && t > 0 && r.usd < t ? r.usd : null
}
