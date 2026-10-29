import { describe, expect, it } from 'vitest'
import type { BalanceView } from './api'
import { dollars, last30, lowBalance, remaining } from './costs'

const key = { usage: 3, usage_daily: 0, usage_monthly: 1.37, limit: null, limit_remaining: null, limit_reset: null, is_free_tier: false }
const view = (patch: Partial<BalanceView> = {}): BalanceView => ({
  key,
  has_mgmt_key: false,
  credits: null,
  credits_error: null,
  mgmt_key_url: 'https://openrouter.ai/settings/management-keys',
  ...patch,
})

describe('dollars', () => {
  it('formats German with two decimals', () => {
    expect(dollars(8.42)).toBe('8,42 $')
    expect(dollars(1234.5)).toBe('1.234,50 $')
  })
})

describe('last30', () => {
  it('fills 30 UTC days ending today, oldest first', () => {
    const now = Date.parse('2026-11-02T23:30:00Z')
    const s = last30([{ day: '2026-11-02', requests: 2, usd: 0.1 }, { day: '2026-10-04', requests: 1, usd: 0.05 }, { day: '2026-10-01', requests: 9, usd: 9 }], now)
    expect(s).toHaveLength(30)
    expect(s[0]).toEqual({ day: '2026-10-04', usd: 0.05, requests: 1 })
    expect(s[29]).toEqual({ day: '2026-11-02', usd: 0.1, requests: 2 })
    expect(s[1].usd).toBe(0)
  })
})

describe('remaining / lowBalance', () => {
  it('prefers the account balance over the key limit', () => {
    expect(remaining(undefined)).toBeNull()
    expect(remaining(view())).toBeNull()
    expect(remaining(view({ key: { ...key, limit: 5, limit_remaining: 4 } }))).toEqual({ usd: 4, kind: 'limit' })
    const both = view({ key: { ...key, limit: 5, limit_remaining: 4 }, credits: { total_credits: 10, total_usage: 8.5, remaining: 1.5 } })
    expect(remaining(both)).toEqual({ usd: 1.5, kind: 'account' })
  })

  it('warns below the threshold (default 2 $), never when unknown or switched off', () => {
    const low = view({ credits: { total_credits: 10, total_usage: 8.5, remaining: 1.5 } })
    expect(lowBalance(low, undefined)).toBe(1.5)
    expect(lowBalance(low, 1)).toBeNull()
    expect(lowBalance(low, 0)).toBeNull()
    expect(lowBalance(view(), 2)).toBeNull()
  })
})
