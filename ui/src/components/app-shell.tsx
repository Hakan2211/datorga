import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ComponentType, type ReactNode } from 'react'
import { Link, useMatch } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import {
  CalendarDays,
  FileText,
  Lock,
  LockOpen,
  Mic,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  Search,
  Settings,
  Sparkles,
  Sun,
  Users,
} from 'lucide-react'
import { api, q } from '@/lib/api'
import { formatDuration } from '@/lib/capture'
import { AssistantPanel } from '@/components/assistant'
import { BackupReminder } from '@/components/backup-reminder'
import { BalanceReminder } from '@/components/balance-reminder'
import { CommandPalette } from '@/components/command-palette'
import { useAutopilotEvents } from '@/components/autopilot'
import { Button } from '@/components/ui/button'
import { Kbd, KbdGroup } from '@/components/ui/kbd'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

type Icon = ComponentType<{ className?: string }>

// Module registry: each module contributes a nav entry. Vorlagen stays hidden until the module
// exists; the Protokoll lives under Einstellungen → Sicherheit (its route stays).
const modules: { to?: string; label: string; icon: Icon; exact?: boolean }[] = [
  { to: '/', label: 'nav.dashboard', icon: Sun, exact: true },
  { to: '/calendar', label: 'nav.calendar', icon: CalendarDays },
  { to: '/patients', label: 'nav.patients', icon: Users },
  { to: '/sessions', label: 'nav.sessions', icon: Mic },
  { to: '/reports', label: 'nav.reports', icon: FileText },
  { to: '/assistant', label: 'nav.assistant', icon: Sparkles },
]

/** Marks the side panel as the assistant's (its title is rendered by the shell). */
const ASSISTANT_PANEL = 'assistant'

// A device preference like the theme: it must work before unlock, so not in the settings table.
const COLLAPSED_KEY = 'thera.sidebar'

type Panel = { title: ReactNode; content: ReactNode }
type PanelApi = { panel: Panel | null; show: (panel: Panel) => void; hide: () => void }
const PanelContext = createContext<PanelApi>({ panel: null, show: () => {}, hide: () => {} })

/** The right-side panel slot (the assistant chat, slice 25). */
export function useSidePanel() {
  return useContext(PanelContext)
}

export function AppShell({ children }: { children: ReactNode }) {
  const { t } = useTranslation()
  const init = useQuery({ queryKey: ['init'], queryFn: api.init, staleTime: Infinity })
  const encrypted = init.data?.encrypted
  // Notes share the Bericht editor route but belong to a session, so the nav marks "Sitzungen".
  const reportId = useMatch({ from: '/reports/$reportId', shouldThrow: false })?.params.reportId
  const openReport = useQuery({ ...q.report(reportId ?? ''), staleTime: Infinity, enabled: !!reportId })
  const inNote = !!reportId && openReport.data?.report.kind === 'sitzungsnotiz'
  useAutopilotEvents()

  const [collapsed, setCollapsed] = useState(() => localStorage.getItem(COLLAPSED_KEY) === 'collapsed')
  const toggle = () =>
    setCollapsed((c) => {
      localStorage.setItem(COLLAPSED_KEY, c ? 'expanded' : 'collapsed')
      return !c
    })

  const [panel, setPanel] = useState<Panel | null>(null)
  const hide = useCallback(() => setPanel(null), [])
  const panelApi = useMemo(() => ({ panel, show: setPanel, hide }), [panel, hide])

  const [palette, setPalette] = useState(false)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        setPalette((o) => !o)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  return (
    <PanelContext.Provider value={panelApi}>
      <div className="flex h-screen overflow-hidden">
        <aside
          data-collapsed={collapsed}
          className={cn(
            'flex shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground transition-[width] duration-150',
            collapsed ? 'w-[4.25rem]' : 'w-60',
          )}
        >
          <div className={cn('flex items-center pt-4 pb-3', collapsed ? 'justify-center px-2' : 'justify-between pr-2 pl-5')}>
            {!collapsed && (
              <div className="min-w-0">
                <div className="font-serif text-xl leading-tight text-primary">{t('app.name')}</div>
                <div className="truncate text-xs text-muted-foreground">{t('app.tagline')}</div>
              </div>
            )}
            <Button
              variant="ghost"
              size="icon"
              className="size-8 text-muted-foreground"
              onClick={toggle}
              aria-label={t(collapsed ? 'shell.expand' : 'shell.collapse')}
              title={t(collapsed ? 'shell.expand' : 'shell.collapse')}
            >
              {collapsed ? <PanelLeftOpen /> : <PanelLeftClose />}
            </Button>
          </div>
          <nav className="flex-1 space-y-0.5 px-3 pt-2">
            {modules.map((m) =>
              m.to ? (
                <NavLink
                  key={m.label}
                  to={m.to}
                  exact={m.exact}
                  collapsed={collapsed}
                  active={inNote ? m.to === '/sessions' : undefined}
                  icon={<m.icon className="size-4 shrink-0" />}
                >
                  {t(m.label)}
                </NavLink>
              ) : (
                <NavTip key={m.label} show={collapsed} label={`${t(m.label)} · ${t('shell.soon')}`}>
                  <div
                    aria-disabled
                    className={cn(
                      'flex items-center gap-2.5 rounded-md py-2 text-sm text-muted-foreground/55',
                      collapsed ? 'justify-center px-0' : 'px-3',
                    )}
                    title={collapsed ? undefined : t('shell.soon')}
                  >
                    <m.icon className="size-4 shrink-0" />
                    {!collapsed && t(m.label)}
                  </div>
                </NavTip>
              ),
            )}
          </nav>
          <div className="space-y-0.5 border-t px-3 py-3">
            <NavLink to="/settings" collapsed={collapsed} icon={<Settings className="size-4 shrink-0" />}>
              {t('nav.settings')}
            </NavLink>
            <NavTip show={collapsed} label={encrypted ? t('privacy.encrypted') : t('privacy.notEncrypted')}>
              <div
                className={cn(
                  'mt-2 flex items-center gap-2 rounded-md py-2 text-xs',
                  collapsed ? 'justify-center' : 'px-3',
                  encrypted ? 'text-success' : 'bg-warning/15 text-foreground',
                )}
              >
                {encrypted ? <Lock className="size-3.5" /> : <LockOpen className="size-3.5" />}
                {!collapsed && (encrypted ? t('privacy.encrypted') : t('privacy.notEncrypted'))}
              </div>
            </NavTip>
          </div>
        </aside>
        <div className="flex min-w-0 flex-1 flex-col">
          <header className="flex h-14 shrink-0 items-center gap-3 border-b px-6">
            <button
              type="button"
              onClick={() => setPalette(true)}
              data-testid="palette-open"
              className="flex h-9 w-80 items-center gap-2 rounded-md border bg-card px-3 text-sm text-muted-foreground shadow-xs transition-colors hover:bg-foreground/5"
            >
              <Search className="size-4" />
              <span className="flex-1 text-left">{t('shell.search')}</span>
              <KbdGroup>
                <Kbd>Strg</Kbd>
                <Kbd>K</Kbd>
              </KbdGroup>
            </button>
            <div className="ml-auto flex items-center gap-2">
              <RecordingPill ready={init.data?.status === 'ready'} />
              <Button
                variant={panel?.title === ASSISTANT_PANEL ? 'secondary' : 'ghost'}
                size="sm"
                onClick={() => (panel?.title === ASSISTANT_PANEL ? hide() : setPanel({ title: ASSISTANT_PANEL, content: <AssistantPanel /> }))}
                data-testid="assistant-toggle"
              >
                <Sparkles /> {t('nav.assistant')}
              </Button>
            </div>
          </header>
          <div className="flex min-h-0 flex-1">
            <main className="min-w-0 flex-1 overflow-y-auto">
              <BackupReminder />
              <BalanceReminder />
              {children}
            </main>
            {panel && (
              <aside className="flex w-[26rem] shrink-0 flex-col border-l bg-card" data-testid="side-panel">
                <div className="flex h-12 items-center justify-between border-b pr-2 pl-4 text-sm font-medium">
                  {panel.title === ASSISTANT_PANEL ? (
                    <span className="flex items-center gap-2">
                      <Sparkles className="size-4 text-primary" /> {t('assistant.title')}
                    </span>
                  ) : (
                    panel.title
                  )}
                  <Button variant="ghost" size="icon" className="size-8" onClick={hide} aria-label={t('shell.closePanel')}>
                    <PanelRightClose />
                  </Button>
                </div>
                <div className="min-h-0 flex-1 overflow-y-auto">{panel.content}</div>
              </aside>
            )}
          </div>
        </div>
      </div>
      <CommandPalette open={palette} onOpenChange={setPalette} />
    </PanelContext.Provider>
  )
}

/** Shown on every page while a recording runs; leads back to its session. */
function RecordingPill({ ready }: { ready: boolean }) {
  const { t } = useTranslation()
  // Slow poll here; the session page polls faster for its meter (react-query uses the shortest).
  const recording = useQuery({ ...q.recording(), refetchInterval: 1000, enabled: ready })
  const r = recording.data
  if (!r) return null
  return (
    <Link
      to="/sessions/$sessionId"
      params={{ sessionId: r.session_id }}
      data-testid="recording-pill"
      className="flex h-8 items-center gap-2 rounded-full bg-destructive/10 px-3 text-sm font-medium text-destructive hover:bg-destructive/15"
    >
      <span className="size-2 animate-pulse rounded-full bg-destructive" />
      {t(`capture.kind.${r.kind}`)}
      <span className="font-mono tabular-nums">{formatDuration(r.duration_ms)}</span>
    </Link>
  )
}

function NavTip({ show, label, children }: { show: boolean; label: ReactNode; children: ReactNode }) {
  if (!show) return children
  return (
    <Tooltip>
      <TooltipTrigger asChild>{children}</TooltipTrigger>
      <TooltipContent side="right">{label}</TooltipContent>
    </Tooltip>
  )
}

const ACTIVE = 'bg-card font-medium text-foreground shadow-xs hover:bg-card'

/** `active` overrides the router's own match when set. */
function NavLink({
  to,
  exact,
  active,
  collapsed = false,
  icon,
  children,
}: {
  to: string
  exact?: boolean
  active?: boolean
  collapsed?: boolean
  icon: ReactNode
  children: ReactNode
}) {
  return (
    <NavTip show={collapsed} label={children}>
      <Link
        to={to}
        activeOptions={{ exact: !!exact }}
        aria-label={collapsed && typeof children === 'string' ? children : undefined}
        className={cn(
          'flex items-center gap-2.5 rounded-md py-2 text-sm transition-colors hover:bg-foreground/5',
          collapsed ? 'justify-center px-0' : 'px-3',
          active && ACTIVE,
        )}
        activeProps={{ className: active === undefined ? ACTIVE : '' }}
      >
        {icon}
        {!collapsed && children}
      </Link>
    </NavTip>
  )
}

export function PageHeader({
  title,
  subtitle,
  actions,
  leading,
}: {
  title: ReactNode
  subtitle?: ReactNode
  actions?: ReactNode
  /** Shown left of the title, e.g. a patient's portrait. */
  leading?: ReactNode
}) {
  return (
    <div className="flex items-end justify-between gap-4 border-b px-8 pt-7 pb-5">
      <div className="flex items-center gap-4">
        {leading}
        <div>
          <h1 className="font-serif text-[1.65rem] leading-tight">{title}</h1>
          {subtitle && <div className="mt-1 text-sm text-muted-foreground">{subtitle}</div>}
        </div>
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  )
}

export function Page({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn('space-y-6 px-8 py-6', className)}>{children}</div>
}
