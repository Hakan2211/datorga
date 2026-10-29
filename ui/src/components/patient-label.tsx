import { cn } from '@/lib/utils'

/** Patient name with the Chiffre small underneath (left out when there is none). Truncates in narrow containers. */
export function PatientLabel({ name, chiffre, className }: { name: string; chiffre: string | null; className?: string }) {
  return (
    <span className={cn('flex min-w-0 flex-col leading-tight', className)}>
      <span className="truncate text-sm font-medium">{name}</span>
      {chiffre && <span className="truncate font-mono text-xs text-muted-foreground">{chiffre}</span>}
    </span>
  )
}
