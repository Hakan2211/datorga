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

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  head: () => ({
    meta: [
      { charSet: 'utf-8' },
      { name: 'viewport', content: 'width=device-width, initial-scale=1' },
      { title: 'thera' },
    ],
    links: [{ rel: 'stylesheet', href: appCss }],
  }),
  shellComponent: RootDocument,
  component: RootComponent,
})

function RootDocument({ children }: { children: ReactNode }) {
  return (
    <html lang="de">
      <head>
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
