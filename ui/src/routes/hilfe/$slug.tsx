import { createFileRoute } from '@tanstack/react-router'
import { HelpPage } from '@/components/help'

export const Route = createFileRoute('/hilfe/$slug')({ component: HelpArticleRoute })

function HelpArticleRoute() {
  const { slug } = Route.useParams()
  return <HelpPage slug={slug} />
}
