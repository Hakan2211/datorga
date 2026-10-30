import { createFileRoute, Link, Outlet } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { Page, PageHeader } from '@/components/app-shell'
import { SETTINGS_PAGES } from '@/lib/settings-nav'

export const Route = createFileRoute('/settings')({ component: SettingsLayout })

/** Einstellungen: a sub-nav on the left (on top in narrow windows), one route per group. */
function SettingsLayout() {
  const { t } = useTranslation()
  return (
    <>
      <PageHeader title={t('settings.title')} />
      <Page className="flex flex-col gap-6 space-y-0 lg:flex-row lg:items-start">
        <nav
          className="-mx-1 flex shrink-0 gap-1 overflow-x-auto px-1 pb-1 [scrollbar-width:none] lg:sticky lg:top-6 lg:mx-0 lg:w-52 lg:flex-col lg:overflow-visible lg:px-0"
          aria-label={t('settings.title')}
        >
          {SETTINGS_PAGES.map((p) => (
            <Link
              key={p.key}
              to={p.to}
              className="flex shrink-0 items-center gap-2.5 rounded-md px-3 py-2 text-sm whitespace-nowrap text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              activeProps={{ className: 'bg-card font-medium text-foreground shadow-xs', 'aria-current': 'page' }}
              data-testid={`settings-nav-${p.key}`}
            >
              <p.icon className="size-4 shrink-0" /> {t(`settings.pages.${p.key}`)}
            </Link>
          ))}
        </nav>
        <div className="min-w-0 flex-1 space-y-6">
          <Outlet />
        </div>
      </Page>
    </>
  )
}
