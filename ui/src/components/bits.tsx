import type { ComponentType, ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Check } from 'lucide-react'
import type { ReportStatus } from '@/bindings/ReportStatus'
import { Badge } from '@/components/ui/badge'
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { cn } from '@/lib/utils'

/** Tones from the mockups: Geplant = sand, Notiz offen = warning, Signiert = success. */
export type PillTone = 'neutral' | 'sand' | 'warning' | 'success' | 'primary' | 'danger'

const pillTone: Record<PillTone, string> = {
  neutral: 'bg-secondary text-secondary-foreground',
  sand: 'bg-sand text-sand-foreground',
  warning: 'bg-warning/15 text-warning-foreground',
  success: 'bg-success/12 text-success',
  primary: 'bg-accent text-accent-foreground',
  danger: 'bg-destructive/10 text-destructive',
}

export function StatusPill({ tone = 'neutral', className, children }: { tone?: PillTone; className?: string; children: ReactNode }) {
  return (
    <span
      className={cn(
        'inline-flex h-6 shrink-0 items-center gap-1.5 rounded-md px-2.5 text-xs font-medium whitespace-nowrap',
        pillTone[tone],
        className,
      )}
    >
      {children}
    </span>
  )
}

const statusTone: Record<ReportStatus, PillTone> = {
  draft: 'neutral',
  in_review: 'warning',
  signed: 'success',
  exported: 'primary',
}

export function StatusBadge({ status }: { status: ReportStatus }) {
  const { t } = useTranslation()
  return <StatusPill tone={statusTone[status]}>{t(`enums.status.${status}`)}</StatusPill>
}

type Icon = ComponentType<{ className?: string }>

/** A quiet placeholder for an empty list or panel, with at most one next action. */
export function EmptyState({
  icon: Icon,
  title,
  description,
  action,
  className,
}: {
  icon?: Icon
  title: ReactNode
  description?: ReactNode
  action?: ReactNode
  className?: string
}) {
  return (
    <div className={cn('flex flex-col items-center gap-2 rounded-lg border border-dashed px-6 py-10 text-center', className)}>
      {Icon && (
        <div className="mb-1 flex size-10 items-center justify-center rounded-full bg-accent text-accent-foreground">
          <Icon className="size-5" />
        </div>
      )}
      <div className="text-sm font-medium">{title}</div>
      {description && <p className="max-w-sm text-sm text-muted-foreground">{description}</p>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  )
}

/** The mockups' card: icon + title, optional description and header actions. */
export function SectionCard({
  icon: Icon,
  title,
  description,
  actions,
  className,
  contentClassName,
  children,
}: {
  icon?: Icon
  title: ReactNode
  description?: ReactNode
  actions?: ReactNode
  className?: string
  contentClassName?: string
  children: ReactNode
}) {
  return (
    <Card className={cn('gap-4', className)}>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base font-semibold">
          {Icon && <Icon className="size-4 text-muted-foreground" />}
          {title}
        </CardTitle>
        {description && <CardDescription>{description}</CardDescription>}
        {actions && <CardAction className="flex items-center gap-2">{actions}</CardAction>}
      </CardHeader>
      <CardContent className={contentClassName}>{children}</CardContent>
    </Card>
  )
}

/** Bottom bar that stays visible while the page scrolls: status on the left, the one primary action on the right. */
export function StickyActionBar({ children, info, className }: { children: ReactNode; info?: ReactNode; className?: string }) {
  return (
    <div
      className={cn(
        'sticky bottom-0 z-10 flex items-center justify-between gap-4 border-t bg-background/90 px-8 py-3 backdrop-blur-sm',
        className,
      )}
    >
      <div className="flex min-w-0 items-center gap-4 text-sm text-muted-foreground">{info}</div>
      <div className="flex shrink-0 items-center gap-2">{children}</div>
    </div>
  )
}

export type StepState = 'done' | 'current' | 'todo'
export type Step<K extends string> = { key: K; label: ReactNode; state: StepState }

/**
 * The workflow steps from the mockups: numbered circles on a line, done steps checked. `viewing`
 * is the step shown below; any step can be opened, the current one is just where work continues.
 */
export function Stepper<K extends string>({
  steps,
  viewing,
  onSelect,
  className,
}: {
  steps: Step<K>[]
  viewing?: K
  onSelect: (key: K) => void
  className?: string
}) {
  return (
    <ol className={cn('flex items-start', className)} data-testid="stepper">
      {steps.map((s, i) => (
        <li key={s.key} className="relative flex flex-1 flex-col items-center">
          {i > 0 && (
            <span
              aria-hidden
              className={cn(
                'absolute top-4 right-1/2 left-[-50%] mx-5 h-px',
                steps[i - 1].state === 'done' ? 'bg-primary' : 'bg-border',
              )}
            />
          )}
          <button
            type="button"
            onClick={() => onSelect(s.key)}
            aria-current={s.state === 'current' ? 'step' : undefined}
            data-step={s.key}
            data-state={s.state}
            className="group relative flex flex-col items-center gap-2 rounded-md px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
          >
            <span
              className={cn(
                'flex size-8 items-center justify-center rounded-full border text-[13px] font-medium transition-colors',
                s.state === 'done' && 'border-primary bg-primary text-primary-foreground',
                s.state === 'current' && 'border-primary bg-card text-primary ring-4 ring-primary/15',
                s.state === 'todo' && 'bg-card text-muted-foreground group-hover:border-foreground/30',
              )}
            >
              {s.state === 'done' ? <Check className="size-4" /> : i + 1}
            </span>
            <span
              className={cn(
                'whitespace-nowrap underline-offset-[6px]',
                s.state === 'todo' ? 'text-muted-foreground' : 'text-foreground',
                s.state === 'current' && 'font-medium',
                viewing === s.key && 'underline decoration-primary decoration-2',
              )}
            >
              {s.label}
            </span>
          </button>
        </li>
      ))}
    </ol>
  )
}

/** The first step not done is current; everything after it is still to do. */
export function stepStates<K extends string>(steps: { key: K; done: boolean }[], current?: K): Record<K, StepState> {
  const at = current ?? steps.find((s) => !s.done)?.key
  const out = {} as Record<K, StepState>
  for (const s of steps) out[s.key] = s.key === at ? 'current' : s.done ? 'done' : 'todo'
  return out
}

/** The note got a Nachtrag after its last export: the file lacks it. */
export function ExportOutdatedBadge({ show }: { show: boolean }) {
  const { t } = useTranslation()
  if (!show) return null
  return (
    <Badge variant="outline" className="ml-2 border-warning font-normal text-warning-foreground" title={t('note.exportOutdatedHint')}>
      {t('note.exportOutdated')}
    </Badge>
  )
}

export function formatDate(iso: string | null | undefined, withTime = false): string {
  if (!iso) return '—'
  const d = new Date(iso.length === 10 ? `${iso}T00:00:00` : iso)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleString('de-DE', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    ...(withTime ? { hour: '2-digit', minute: '2-digit' } : {}),
  })
}
