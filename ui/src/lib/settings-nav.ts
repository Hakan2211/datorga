import { DatabaseBackup, Mic, ScrollText, ShieldCheck, SlidersHorizontal, Sparkles, Trash2, type LucideIcon } from 'lucide-react'

/** The Einstellungen subpages, in nav order. Shared by the settings nav and the Strg+K palette. */
export const SETTINGS_PAGES = [
  { key: 'allgemein', to: '/settings/allgemein', icon: SlidersHorizontal },
  { key: 'ki', to: '/settings/ki', icon: Sparkles },
  { key: 'sprache', to: '/settings/sprache', icon: Mic },
  { key: 'sicherung', to: '/settings/sicherung', icon: DatabaseBackup },
  { key: 'papierkorb', to: '/settings/papierkorb', icon: Trash2 },
  { key: 'sicherheit', to: '/settings/sicherheit', icon: ShieldCheck },
  { key: 'protokoll', to: '/settings/protokoll', icon: ScrollText },
] as const satisfies readonly { key: string; to: string; icon: LucideIcon }[]
