/// <reference types="vite/client" />
import type { ReactNode } from 'react'
import { ClientOnly, HeadContent, Outlet, Scripts, createRootRouteWithContext } from '@tanstack/react-router'
import { QueryClientProvider, type QueryClient } from '@tanstack/react-query'
import appCss from '@/styles.css?url'
import '@/lib/i18n'
import { AppShell } from '@/components/app-shell'
import { UnlockGate } from '@/components/unlock-gate'
import { Toaster } from '@/components/ui/sonner'
import { TooltipProvider } from '@/components/ui/tooltip'
import { THEME_BOOT_SCRIPT } from '@/lib/theme'

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  head: () => ({
    meta: [
      { charSet: 'utf-8' },
      { name: 'viewport', content: 'width=device-width, initial-scale=1' },
      { title: 'Theralo' },
    ],
    links: [{ rel: 'stylesheet', href: appCss }],
  }),
  shellComponent: RootDocument,
  component: RootComponent,
})

function RootDocument({ children }: { children: ReactNode }) {
  return (
    // The boot script sets `dark` on <html> before React hydrates.
    <html lang="de" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_BOOT_SCRIPT }} />
        <HeadContent />
      </head>
      <body>
        {children}
        <Scripts />
      </body>
    </html>
  )
}

function RootComponent() {
  const { queryClient } = Route.useRouteContext()
  return (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        {/* Tauri IPC exists only in the client; the prerendered SPA shell stays empty. */}
        <ClientOnly fallback={null}>
          <UnlockGate>
            <AppShell>
              <Outlet />
            </AppShell>
          </UnlockGate>
        </ClientOnly>
        <Toaster position="bottom-right" />
      </TooltipProvider>
    </QueryClientProvider>
  )
}
