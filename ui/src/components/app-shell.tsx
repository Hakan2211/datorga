import type { ReactNode } from 'react'
import { Link, useMatch } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import {
  CalendarDays,
  FileText,
  LayoutDashboard,
  Library,
  Lock,
  LockOpen,
  Mic,
  ReceiptText,
  ScrollText,
  Settings,
  Users,
} from 'lucide-react'
import { api, q } from '@/lib/api'
import { BackupReminder } from '@/components/backup-reminder'
import { cn } from '@/lib/utils'

// Module registry: each module contributes a nav entry. Later modules are listed but disabled.
const modules = [
  { to: '/', label: 'nav.dashboard', icon: LayoutDashboard, exact: true },
  { to: '/patients', label: 'nav.patients', icon: Users, exact: false },
  { to: '/sessions', label: 'nav.sessions', icon: Mic, exact: false },
  { to: '/reports', label: 'nav.reports', icon: FileText, exact: false },
  { to: '/templates', label: 'nav.templates', icon: Library, exact: false },
] as const

const later = [
  { label: 'nav.calendar', icon: CalendarDays },
  { label: 'nav.billing', icon: ReceiptText },
] as const

export function AppShell({ children }: { children: ReactNode }) {
  const { t } = useTranslation()
  const init = useQuery({ queryKey: ['init'], queryFn: api.init, staleTime: Infinity })
  const encrypted = init.data?.encrypted
  // Slow poll here; the session page polls faster for its meter (react-query uses the shortest).
  const recording = useQuery({ ...q.recording(), refetchInterval: 1000, enabled: init.data?.status === 'ready' })
  // Notes share the Bericht editor route but belong to a session, so the nav marks "Sitzungen".
  const reportId = useMatch({ from: '/reports/$reportId', shouldThrow: false })?.params.reportId
  const openReport = useQuery({ ...q.report(reportId ?? ''), staleTime: Infinity, enabled: !!reportId })
  const inNote = !!reportId && openReport.data?.report.kind === 'sitzungsnotiz'

  return (
    <div className="flex h-screen overflow-hidden">
      <aside className="flex w-60 shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground">
        <div className="px-5 pt-5 pb-4">
          <div className="text-lg font-semibold tracking-tight text-primary">{t('app.name')}</div>
          <div className="text-xs text-muted-foreground">{t('app.tagline')}</div>
        </div>
        {recording.data && (
          <Link
            to="/sessions/$sessionId"
            params={{ sessionId: recording.data.session_id }}
            className="mx-3 mb-3 flex items-center gap-2 rounded-md bg-destructive/10 px-3 py-2 text-sm font-medium text-destructive"
          >
            <span className="size-2 animate-pulse rounded-full bg-destructive" />
            {t('capture.running')}
          </Link>
        )}
        <nav className="flex-1 space-y-0.5 px-3">
          {modules.map((m) => (
            <NavLink
              key={m.to}
              to={m.to}
              exact={m.exact}
              active={inNote ? m.to === '/sessions' : undefined}
              icon={<m.icon className="size-4" />}
            >
              {t(m.label)}
            </NavLink>
          ))}
          <div className="px-3 pt-5 pb-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
            {t('nav.later')}
          </div>
          {later.map((m) => (
            <div key={m.label} className="flex items-center gap-2.5 rounded-md px-3 py-2 text-sm text-muted-foreground/60">
              <m.icon className="size-4" />
              {t(m.label)}
            </div>
          ))}
        </nav>
        <div className="space-y-0.5 border-t px-3 py-3">
          <NavLink to="/audit" icon={<ScrollText className="size-4" />}>
            {t('nav.audit')}
          </NavLink>
          <NavLink to="/settings" icon={<Settings className="size-4" />}>
            {t('nav.settings')}
          </NavLink>
          <div
            className={cn(
              'mt-2 flex items-center gap-2 rounded-md px-3 py-2 text-xs',
              encrypted ? 'text-success' : 'bg-warning/15 text-foreground',
            )}
          >
            {encrypted ? <Lock className="size-3.5" /> : <LockOpen className="size-3.5" />}
            {encrypted ? t('privacy.encrypted') : t('privacy.notEncrypted')}
          </div>
        </div>
      </aside>
      <main className="flex-1 overflow-y-auto">
        <BackupReminder />
        {children}
      </main>
    </div>
  )
}

const ACTIVE = 'bg-card font-medium text-foreground shadow-xs hover:bg-card'

/** `active` overrides the router's own match when set. */
function NavLink({
  to,
  exact,
  active,
  icon,
  children,
}: {
  to: string
  exact?: boolean
  active?: boolean
  icon: ReactNode
  children: ReactNode
}) {
  return (
    <Link
      to={to}
      activeOptions={{ exact: !!exact }}
      className={cn(
        'flex items-center gap-2.5 rounded-md px-3 py-2 text-sm transition-colors hover:bg-foreground/5',
        active && ACTIVE,
      )}
      activeProps={{ className: active === undefined ? ACTIVE : '' }}
    >
      {icon}
      {children}
    </Link>
  )
}

export function PageHeader({ title, subtitle, actions }: { title: ReactNode; subtitle?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="flex items-end justify-between gap-4 border-b px-8 pt-7 pb-5">
      <div>
        <h1 className="font-serif text-[1.65rem] leading-tight">{title}</h1>
        {subtitle && <div className="mt-1 text-sm text-muted-foreground">{subtitle}</div>}
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  )
}

export function Page({ children }: { children: ReactNode }) {
  return <div className="space-y-6 px-8 py-6">{children}</div>
}
