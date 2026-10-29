import { Link } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { CircleHelp } from 'lucide-react'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { helpArticle, type HelpSlug } from '@/lib/help'
import { cn } from '@/lib/utils'

/** The small "?" next to a card title: opens the matching Hilfe article. */
export function HelpLink({ slug, className }: { slug: HelpSlug; className?: string }) {
  const { t } = useTranslation()
  const title = helpArticle(slug)?.title ?? ''
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Link
          to="/hilfe/$slug"
          params={{ slug }}
          aria-label={t('help.linkTo', { title })}
          data-testid={`help-link-${slug}`}
          className={cn('inline-flex size-6 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-foreground/5 hover:text-primary', className)}
        >
          <CircleHelp className="size-4" />
        </Link>
      </TooltipTrigger>
      <TooltipContent>{t('help.linkTo', { title })}</TooltipContent>
    </Tooltip>
  )
}
