import { createFileRoute } from '@tanstack/react-router'
import { HelpPage } from '@/components/help'
import { HELP } from '@/lib/help'

export const Route = createFileRoute('/hilfe/')({ component: () => <HelpPage slug={HELP.start} /> })
