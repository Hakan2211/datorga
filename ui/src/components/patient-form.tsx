import { useState, type ReactNode } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import type { Patient } from '@/bindings/Patient'
import type { PatientInput } from '@/bindings/PatientInput'
import { api, errorMessage } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'

const empty: PatientInput = {
  external_ref: null,
  first_name: '',
  last_name: '',
  birth_date: null,
  chiffre: null,
  pronouns: null,
}

/** Suggests a Chiffre: first letter of the surname + birth date as DDMMYY. Verify the convention with the practice. */
export function suggestChiffre(lastName: string, birthDate: string | null): string {
  const m = birthDate?.match(/^(\d{4})-(\d{2})-(\d{2})$/)
  if (!lastName.trim() || !m) return ''
  return `${lastName.trim()[0]!.toUpperCase()}${m[3]}${m[2]}${m[1]!.slice(2)}`
}

export function PatientFormDialog({
  patient,
  trigger,
  onSaved,
}: {
  patient?: Patient
  trigger: ReactNode
  onSaved?: (p: Patient) => void
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState<PatientInput>(patient ? { ...patient } : empty)

  const save = useMutation({
    mutationFn: () => {
      const input = { ...form, chiffre: form.chiffre || suggestChiffre(form.last_name, form.birth_date) || null }
      return patient ? api.updatePatient(patient.id, input) : api.createPatient(input)
    },
    onSuccess: (p) => {
      void qc.invalidateQueries({ queryKey: ['patients'] })
      void qc.invalidateQueries({ queryKey: ['patient', p.id] })
      toast.success(t('common.saved'))
      setOpen(false)
      if (!patient) setForm(empty)
      onSaved?.(p)
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  const field = (key: keyof PatientInput) => ({
    value: form[key] ?? '',
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => setForm({ ...form, [key]: e.target.value || null }),
  })

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{patient ? t('common.edit') : t('patients.new')}</DialogTitle>
        </DialogHeader>
        <form
          className="grid grid-cols-2 gap-4"
          onSubmit={(e) => {
            e.preventDefault()
            save.mutate()
          }}
        >
          <Field label={t('patients.firstName')}>
            <Input required autoFocus {...field('first_name')} value={form.first_name} />
          </Field>
          <Field label={t('patients.lastName')}>
            <Input required {...field('last_name')} value={form.last_name} />
          </Field>
          <Field label={t('patients.birthDate')}>
            <Input type="date" {...field('birth_date')} />
          </Field>
          <Field label={t('patients.pronouns')}>
            <Input {...field('pronouns')} />
          </Field>
          <Field label={t('patients.chiffre')} hint={t('patients.chiffreHint')}>
            <Input
              className="font-mono"
              placeholder={suggestChiffre(form.last_name, form.birth_date)}
              {...field('chiffre')}
            />
          </Field>
          <Field label={t('patients.externalRef')}>
            <Input {...field('external_ref')} />
          </Field>
          <DialogFooter className="col-span-2">
            <Button type="button" variant="ghost" onClick={() => setOpen(false)}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" disabled={save.isPending}>
              {patient ? t('common.save') : t('common.create')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      {children}
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  )
}
