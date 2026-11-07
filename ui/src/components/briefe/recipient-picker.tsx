import { useState, type ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Building2, Star } from 'lucide-react'
import type { Recipient } from '@/bindings/Recipient'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { q } from '@/lib/api'

/** Address book search: the patient's remembered recipients first, then the whole book. */
export function RecipientPicker({
  patientId,
  trigger,
  onPick,
}: {
  patientId?: string
  trigger: ReactNode
  onPick: (r: Recipient) => void
}) {
  const [open, setOpen] = useState(false)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>{trigger}</PopoverTrigger>
      <PopoverContent className="w-96 p-0" align="start">
        {open && (
          <RecipientCommand
            patientId={patientId}
            onPick={(r) => {
              setOpen(false)
              onPick(r)
            }}
          />
        )}
      </PopoverContent>
    </Popover>
  )
}

/** The searchable list itself (also inline in the "Neuer Brief" dialog). */
export function RecipientCommand({ patientId, onPick }: { patientId?: string; onPick: (r: Recipient) => void }) {
  const { t } = useTranslation()
  const all = useQuery(q.recipients())
  const linked = useQuery({ ...q.patientRecipients(patientId ?? ''), enabled: !!patientId })
  const linkedIds = new Set(linked.data?.map((r) => r.id))
  const rest = all.data?.filter((r) => !linkedIds.has(r.id)) ?? []
  const row = (r: Recipient, starred: boolean) => (
    <CommandItem
      key={r.id}
      value={`${r.name} ${r.department} ${r.lines.join(' ')} ${r.id}`}
      onSelect={() => onPick(r)}
      data-testid="recipient-option"
    >
      {starred ? <Star className="text-primary" /> : <Building2 />}
      <span className="flex min-w-0 flex-col leading-tight">
        <span className="truncate">{r.name}</span>
        <span className="truncate text-xs text-muted-foreground">
          {[r.role ?? t(`enums.recipient_kind.${r.kind}`), r.department, r.lines.join(', ')].filter(Boolean).join(' · ')}
        </span>
      </span>
    </CommandItem>
  )
  return (
    <Command>
      <CommandInput placeholder={t('briefe.recipientSearch')} />
      <CommandList>
        <CommandEmpty>{all.data?.length === 0 ? t('briefe.noRecipients') : t('palette.empty')}</CommandEmpty>
        {!!linked.data?.length && <CommandGroup heading={t('briefe.recipientsOfPatient')}>{linked.data.map((r) => row(r, true))}</CommandGroup>}
        {rest.length > 0 && <CommandGroup heading={t('briefe.recipientsAll')}>{rest.map((r) => row(r, false))}</CommandGroup>}
      </CommandList>
    </Command>
  )
}
