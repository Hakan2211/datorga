import { cloneElement, useId, useState, type ReactElement, type ReactNode } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { ImagePlus, X } from 'lucide-react'
import type { Gender } from '@/bindings/Gender'
import type { Patient } from '@/bindings/Patient'
import type { PatientInput } from '@/bindings/PatientInput'
import { api, errorMessage } from '@/lib/api'
import { PatientAvatar } from '@/components/patient-avatar'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

const GENDERS: Gender[] = ['w', 'm', 'd']
const NO_GENDER = 'none'

/** Photo change, applied after the patient itself is saved. */
type PhotoChange = { kind: 'keep' } | { kind: 'set'; path: string } | { kind: 'remove' }

const empty: PatientInput = {
  external_ref: null,
  first_name: '',
  last_name: '',
  birth_date: null,
  chiffre: null,
  pronouns: null,
  gender: null,
}

/** Suggests a Chiffre: first letter of the surname + birth date as DDMMYY. Verify the convention with the practice. */
export function suggestChiffre(lastName: string, birthDate: string | null): string {
  const m = birthDate?.match(/^(\d{4})-(\d{2})-(\d{2})$/)
  if (!lastName.trim() || !m) return ''
  return `${lastName.trim()[0]!.toUpperCase()}${m[3]}${m[2]}${m[1]!.slice(2)}`
}

export function PatientFormDialog({
  patient,
  hasPhoto = false,
  trigger,
  onSaved,
}: {
  patient?: Patient
  hasPhoto?: boolean
  trigger: ReactNode
  onSaved?: (p: Patient) => void
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState<PatientInput>(patient ? { ...patient } : empty)
  const [photo, setPhoto] = useState<PhotoChange>({ kind: 'keep' })

  const save = useMutation({
    mutationFn: async () => {
      const input = { ...form, chiffre: form.chiffre || suggestChiffre(form.last_name, form.birth_date) || null }
      const saved = patient ? await api.updatePatient(patient.id, input) : await api.createPatient(input)
      // A bad image must not lose the rest of the form: the patient is saved, the photo error is shown.
      try {
        if (photo.kind === 'set') await api.setPatientPhoto(saved.id, photo.path)
        if (photo.kind === 'remove') await api.removePatientPhoto(saved.id)
      } catch (e) {
        toast.error(errorMessage(e))
      }
      return saved
    },
    onSuccess: (p) => {
      void qc.invalidateQueries({ queryKey: ['patients'] })
      void qc.invalidateQueries({ queryKey: ['patient', p.id] })
      toast.success(t('common.saved'))
      setOpen(false)
      setPhoto({ kind: 'keep' })
      if (!patient) setForm(empty)
      onSaved?.(p)
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  const pickPhoto = async () => {
    const path = await openDialog({
      multiple: false,
      filters: [{ name: t('patients.photoFilter'), extensions: ['jpg', 'jpeg', 'png', 'webp'] }],
    })
    if (typeof path === 'string') setPhoto({ kind: 'set', path })
  }
  const showsPhoto = photo.kind === 'set' || (photo.kind === 'keep' && hasPhoto)

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
          <Field label={t('patients.gender')}>
            <GenderSelect value={form.gender} onChange={(gender) => setForm({ ...form, gender })} />
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
          <div className="col-span-2 space-y-1.5">
            <Label>{t('patients.photo')}</Label>
            <div className="flex items-center gap-3">
              <PatientAvatar
                patientId={patient?.id ?? ''}
                hasPhoto={!!patient && hasPhoto && photo.kind === 'keep'}
                firstName={form.first_name}
                lastName={form.last_name}
                className="size-12 text-base"
              />
              <div className="min-w-0 flex-1">
                {photo.kind === 'set' && <p className="truncate text-sm">{photo.path.split(/[\\/]/).pop()}</p>}
                <p className="text-xs text-muted-foreground">{t('patients.photoHint')}</p>
              </div>
              <Button type="button" variant="outline" size="sm" onClick={() => void pickPhoto()} data-testid="patient-photo-pick">
                <ImagePlus /> {showsPhoto ? t('patients.photoChange') : t('patients.photoPick')}
              </Button>
              {showsPhoto && (
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  aria-label={t('patients.photoRemove')}
                  onClick={() => setPhoto(hasPhoto ? { kind: 'remove' } : { kind: 'keep' })}
                >
                  <X />
                </Button>
              )}
            </div>
          </div>
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

/** Label linked to its single input (or select trigger) by id, so screen readers and `getByLabel` find it. */
function GenderSelect({ id, value, onChange }: { id?: string; value: Gender | null; onChange: (g: Gender | null) => void }) {
  const { t } = useTranslation()
  return (
    <Select value={value ?? NO_GENDER} onValueChange={(v) => onChange(v === NO_GENDER ? null : (v as Gender))}>
      <SelectTrigger id={id} className="w-full">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={NO_GENDER}>{t('patients.genderNone')}</SelectItem>
        {GENDERS.map((g) => (
          <SelectItem key={g} value={g}>
            {t(`enums.gender.${g}`)}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactElement<{ id?: string }> }) {
  const id = useId()
  return (
    <div className="space-y-1.5">
      <Label htmlFor={id}>{label}</Label>
      {cloneElement(children, { id })}
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  )
}
