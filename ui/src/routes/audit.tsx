import { createFileRoute, redirect } from '@tanstack/react-router'

/** The Protokoll lives under Einstellungen now; old links land there. */
export const Route = createFileRoute('/audit')({
  beforeLoad: () => {
    throw redirect({ to: '/settings/protokoll', replace: true })
  },
})
