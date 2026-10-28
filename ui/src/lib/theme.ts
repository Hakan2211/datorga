import { useSyncExternalStore } from 'react'

export type Theme = 'hell' | 'dunkel' | 'system'
export const THEMES: Theme[] = ['hell', 'dunkel', 'system']

// A device preference, not practice data: it must work before unlock, so it lives in
// localStorage like the mic choice, not in the encrypted settings table.
const KEY = 'thera.theme'
const DEFAULT: Theme = 'hell'

/** Runs inline in <head> before first paint, so a dark start doesn't flash light. */
export const THEME_BOOT_SCRIPT = `try{var t=localStorage.getItem('${KEY}')||'${DEFAULT}';if(t==='dunkel'||(t==='system'&&matchMedia('(prefers-color-scheme: dark)').matches))document.documentElement.classList.add('dark')}catch(e){}`

const listeners = new Set<() => void>()
const media = typeof window === 'undefined' ? null : window.matchMedia('(prefers-color-scheme: dark)')

function read(): Theme {
  const t = localStorage.getItem(KEY)
  return t === 'hell' || t === 'dunkel' || t === 'system' ? t : DEFAULT
}

function resolve(t: Theme): 'light' | 'dark' {
  if (t === 'system') return media?.matches ? 'dark' : 'light'
  return t === 'dunkel' ? 'dark' : 'light'
}

function apply() {
  document.documentElement.classList.toggle('dark', resolve(read()) === 'dark')
  listeners.forEach((l) => l())
}

media?.addEventListener('change', apply)

export function setTheme(t: Theme) {
  localStorage.setItem(KEY, t)
  apply()
}

function subscribe(l: () => void) {
  listeners.add(l)
  return () => listeners.delete(l)
}

/** The chosen theme and what it resolves to right now (`system` follows Windows). */
export function useTheme(): { theme: Theme; resolved: 'light' | 'dark' } {
  const theme = useSyncExternalStore(subscribe, read, () => DEFAULT)
  const resolved = useSyncExternalStore(subscribe, () => resolve(read()), () => 'light' as const)
  return { theme, resolved }
}
