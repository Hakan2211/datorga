import { useMemo, useState, type ReactNode } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Search, X } from 'lucide-react'
import type { Category } from '@/bindings/Category'
import type { Letter } from '@/bindings/Letter'
import type { Recipient } from '@/bindings/Recipient'
import { RecipientCommand } from '@/components/briefe/recipient-picker'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { api, errorMessage, q } from '@/lib/api'
import { letterTemplates } from '@/lib/letters'
import { cn } from '@/lib/utils'

/** "Neuer Brief": patient, recipient (optional), type → the letter page. */
export function NewLetterDialog({
  patientId,
  trigger,
  open: openProp,
  onOpenChange,
}: {
  /** Opened from a patient: no patient choice. */
  patientId?: string
  trigger?: ReactNode
  open?: boolean
  onOpenChange?: (open: boolean) => void
}) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const [openState, setOpenState] = useState(false)
  const open = openProp ?? openState
  const setOpen = (o: boolean) => {
    if (openProp === undefined) setOpenState(o)
    onOpenChange?.(o)
  }
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      {trigger && <DialogTrigger asChild>{trigger}</DialogTrigger>}
      <DialogContent className="sm:max-w-2xl" data-testid="new-letter-dialog">
        <DialogHeader>
          <DialogTitle className="font-serif text-xl font-normal">{t('briefe.new')}</DialogTitle>
          <DialogDescription>{t('briefe.newDescription')}</DialogDescription>
        </DialogHeader>
        {open && (
          <NewLetterForm
            patientId={patientId}
            onCancel={() => setOpen(false)}
            onCreated={(l) => {
              setOpen(false)
              void navigate({ to: '/briefe/$briefId', params: { briefId: l.report_id } })
            }}
          />
        )}
      </DialogContent>
    </Dialog>
  )
}

/**
 * Patient, recipient (optional) and type, then "Anlegen". Inside a dialog: the "Neuer Brief"
 * dialog, and "Weitere Berichte" in "Neuer Bericht" (`categories: ['bericht']`).
 */
export function NewLetterForm({
  patientId: fixedPatient,
  categories,
  createLabel,
  onCancel,
  onCreated,
}: {
  patientId?: string
  categories?: Category[]
  createLabel?: string
  onCancel: () => void
  onCreated: (letter: Letter) => void
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [patientId, setPatientId] = useState<string | null>(fixedPatient ?? null)
  const [query, setQuery] = useState('')
  const [recipient, setRecipient] = useState<Recipient | null>(null)
  const [pickRecipient, setPickRecipient] = useState(false)
  const [templateKey, setTemplateKey] = useState<string | null>(null)
  const patients = useQuery({ ...q.patients(), enabled: !fixedPatient })
  const templates = useQuery(q.templates())
  const types = useMemo(() => letterTemplates(templates.data ?? [], recipient?.kind ?? null, categories), [templates.data, recipient, categories])
  const chosenType = templateKey ?? types[0]?.template.key ?? null

  const create = useMutation({
    mutationFn: () =>
      api.createLetter({ patient_id: patientId!, template_key: chosenType!, episode_id: null, recipient_id: recipient?.id ?? null }),
    onSuccess: (l) => {
      void qc.invalidateQueries({ queryKey: ['letters'] })
      void qc.invalidateQueries({ queryKey: ['reports'] })
      void qc.invalidateQueries({ queryKey: ['timeline', l.patient_id] })
      onCreated(l)
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  const needle = query.trim().toLowerCase()
  const shown = (patients.data ?? []).filter((p) => !needle || `${p.first_name} ${p.last_name} ${p.chiffre ?? ''}`.toLowerCase().includes(needle))
  const picked = patients.data?.find((p) => p.id === patientId)

  return (
    <>
      {!fixedPatient && (
        <section className="space-y-2">
          <Label>{t('briefe.patient')}</Label>
          {picked ? (
            <div className="flex items-center justify-between rounded-md border px-3 py-2 text-sm">
              <span className="font-medium">
                {picked.first_name} {picked.last_name}
              </span>
              <Button variant="ghost" size="sm" onClick={() => setPatientId(null)}>
                <X /> {t('common.edit')}
              </Button>
            </div>
          ) : (
            <>
              <div className="relative">
                <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input autoFocus value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t('briefe.pickPatient')} className="pl-9" />
              </div>
              <ul className="max-h-48 divide-y overflow-y-auto rounded-md border">
                {patients.data?.length === 0 && <li className="px-3 py-4 text-sm text-muted-foreground">{t('briefe.noPatients')}</li>}
                {shown.map((p) => (
                  <li key={p.id}>
                    <button
                      type="button"
                      className="flex w-full items-center justify-between px-3 py-2 text-left text-sm hover:bg-foreground/5"
                      onClick={() => setPatientId(p.id)}
                      data-testid="new-letter-patient"
                    >
                      <span>
                        {p.last_name}, {p.first_name}
                      </span>
                      {p.chiffre && <span className="font-mono text-xs text-muted-foreground">{p.chiffre}</span>}
                    </button>
                  </li>
                ))}
              </ul>
            </>
          )}
        </section>
      )}

      <section className="space-y-2">
        <Label>{t('briefe.recipient')}</Label>
        {recipient ? (
          <div className="flex items-center justify-between rounded-md border px-3 py-2 text-sm">
            <span className="min-w-0">
              <span className="font-medium">{recipient.name}</span>
              <span className="ml-2 text-xs text-muted-foreground">{t(`enums.recipient_kind.${recipient.kind}`)}</span>
            </span>
            <Button variant="ghost" size="sm" onClick={() => setRecipient(null)}>
              <X /> {t('common.edit')}
            </Button>
          </div>
        ) : pickRecipient ? (
          <div className="max-h-56 overflow-hidden rounded-md border">
            <RecipientCommand
              patientId={patientId ?? undefined}
              onPick={(r) => {
                setRecipient(r)
                setTemplateKey(null)
                setPickRecipient(false)
              }}
            />
          </div>
        ) : (
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="outline" size="sm" onClick={() => setPickRecipient(true)} data-testid="new-letter-pick-recipient">
              {t('briefe.fromBook')}
            </Button>
            <span className="text-xs text-muted-foreground">{t('briefe.recipientLater')}</span>
          </div>
        )}
      </section>

      <section className="space-y-2">
        <Label>{t('briefe.type')}</Label>
        <div className="grid gap-2 sm:grid-cols-2">
          {types.map(({ template, recommended }) => (
            <button
              key={template.key}
              type="button"
              onClick={() => setTemplateKey(template.key)}
              aria-pressed={chosenType === template.key}
              className={cn(
                'flex flex-col items-start gap-1 rounded-md border px-3 py-2.5 text-left transition-colors hover:bg-foreground/5',
                chosenType === template.key && 'border-primary bg-primary/5 ring-1 ring-primary/30',
              )}
              data-testid={`letter-type-${template.key}`}
            >
              <span className="flex items-center gap-2 text-sm font-medium">
                {template.title}
                {recommended && <Badge variant="secondary">{t('briefe.recommended')}</Badge>}
              </span>
              {template.description && <span className="text-xs text-muted-foreground">{template.description}</span>}
            </button>
          ))}
        </div>
      </section>

      <DialogFooter>
        <Button variant="outline" onClick={onCancel}>
          {t('common.cancel')}
        </Button>
        <Button disabled={!patientId || !chosenType || create.isPending} onClick={() => create.mutate()} data-testid="new-letter-create">
          {createLabel ?? t('briefe.create')}
        </Button>
      </DialogFooter>
    </>
  )
}
