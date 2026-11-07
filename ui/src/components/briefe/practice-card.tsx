import { useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { Building2, Eye, ImagePlus, Loader2, X } from 'lucide-react'
import type { Practice } from '@/bindings/Practice'
import type { PracticeAsset } from '@/bindings/PracticeAsset'
import { SectionCard } from '@/components/bits'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage, q } from '@/lib/api'
import { EMPTY_PRACTICE } from '@/lib/letters'

/**
 * Settings → Allgemein: the practice data for the letterhead, the letter footer and the line
 * at the top of a Bericht; logo and optional signature image; a preview of page one.
 */
export function PracticeCard() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const stored = useQuery(q.practice())
  const storedName = useQuery(q.setting<string>('therapist_name'))
  const [edit, setEdit] = useState<Practice | null>(null)
  const [name, setName] = useState<string | null>(null)
  const p = edit ?? stored.data ?? EMPTY_PRACTICE
  const therapist = name ?? storedName.data ?? ''
  const [previewOn, setPreviewOn] = useState(false)
  const [previewRev, setPreviewRev] = useState(0)
  const preview = useQuery({
    queryKey: ['practice', 'preview', previewRev],
    queryFn: () => api.practicePreview(1.4),
    enabled: previewOn,
    staleTime: Infinity,
  })
  const set = (patch: Partial<Practice>) => setEdit({ ...p, ...patch })
  const dirty = edit !== null || name !== null

  const save = useMutation({
    mutationFn: async () => {
      if (edit) await api.setPractice(edit)
      if (name !== null) await api.setSetting('therapist_name', name.trim())
    },
    onSuccess: () => {
      setEdit(null)
      setName(null)
      void qc.invalidateQueries({ queryKey: ['practice'] })
      void qc.invalidateQueries({ queryKey: ['setting', 'therapist_name'] })
      setPreviewRev((r) => r + 1)
      toast.success(t('common.saved'))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  const text = (key: keyof Practice, label: string, hint?: string) => (
    <div className="space-y-1">
      <Label htmlFor={`practice-${key}`} className="text-xs">
        {label}
      </Label>
      <Input id={`practice-${key}`} value={String(p[key] ?? '')} onChange={(e) => set({ [key]: e.target.value })} data-testid={`practice-${key}`} />
      {hint && <p className="text-[11px] text-muted-foreground">{hint}</p>}
    </div>
  )

  return (
    <SectionCard
      icon={Building2}
      title={t('settings.practiceTitle')}
      description={t('settings.practiceHint')}
      className="max-w-2xl"
      contentClassName="space-y-6"
    >
      <Group title={t('settings.practiceGroups.letterhead')}>
        {text('name', t('settings.practiceFields.name'))}
        {text('subtitle', t('settings.practiceFields.subtitle'), t('settings.practiceFields.subtitleHint'))}
        <AssetField kind="logo" label={t('settings.logo')} hint={t('settings.logoHint')} pick={t('settings.logoPick')} onChange={() => setPreviewRev((r) => r + 1)} />
      </Group>
      <Group title={t('settings.practiceGroups.address')}>
        <div className="col-span-2">{text('street', t('settings.practiceFields.street'))}</div>
        <div className="col-span-2 grid grid-cols-[7rem_1fr] gap-3">
          {text('postal_code', t('settings.practiceFields.postalCode'))}
          {text('city', t('settings.practiceFields.city'))}
        </div>
        {text('place', t('settings.practiceFields.place'), t('settings.practiceFields.placeHint'))}
      </Group>
      <Group title={t('settings.practiceGroups.contact')}>
        {text('phone', t('settings.practiceFields.phone'))}
        {text('fax', t('settings.practiceFields.fax'))}
        {text('email', t('settings.practiceFields.email'))}
        {text('web', t('settings.practiceFields.web'))}
      </Group>
      <Group title={t('settings.practiceGroups.bank')}>
        {text('bank', t('settings.practiceFields.bank'))}
        {text('iban', t('settings.practiceFields.iban'))}
        {text('bic', t('settings.practiceFields.bic'))}
        {text('tax_no', t('settings.practiceFields.taxNo'))}
      </Group>
      <Group title={t('settings.practiceGroups.signature')}>
        <div className="col-span-2 space-y-1">
          <Label htmlFor="therapist_name" className="text-xs">
            {t('settings.therapist')}
          </Label>
          <Input id="therapist_name" value={therapist} onChange={(e) => setName(e.target.value)} data-testid="practice-therapist" />
        </div>
        <div className="col-span-2 space-y-1">
          <Label htmlFor="practice-titles" className="text-xs">
            {t('settings.practiceFields.titleLines')}
          </Label>
          <Textarea id="practice-titles" rows={2} value={p.title_lines} onChange={(e) => set({ title_lines: e.target.value })} />
          <p className="text-[11px] text-muted-foreground">{t('settings.practiceFields.titleLinesHint')}</p>
        </div>
        <div className="col-span-2 space-y-2">
          <AssetField
            kind="signature"
            label={t('settings.signatureImage')}
            hint={t('settings.signatureHint')}
            pick={t('settings.signaturePick')}
            onChange={() => setPreviewRev((r) => r + 1)}
          />
          <div className="flex items-center gap-2">
            <Switch id="signature-on" checked={p.signature_image} onCheckedChange={(v) => set({ signature_image: v })} />
            <Label htmlFor="signature-on" className="text-sm font-normal">
              {t('settings.signatureOn')}
            </Label>
          </div>
        </div>
      </Group>
      <div className="flex items-center gap-2">
        <Button onClick={() => save.mutate()} disabled={!dirty || save.isPending} data-testid="practice-save">
          {t('common.save')}
        </Button>
        <Button variant="outline" onClick={() => setPreviewOn((v) => !v)} data-testid="practice-preview">
          <Eye /> {t('settings.practicePreview')}
        </Button>
      </div>
      {previewOn && (
        <div className="rounded-md border bg-muted/40 p-3">
          {preview.data?.[0] ? (
            <img src={preview.data[0]} alt={t('settings.practicePreview')} className="mx-auto w-full max-w-xl shadow-sm" data-testid="practice-preview-page" />
          ) : preview.isError ? (
            <p className="text-sm text-destructive">{errorMessage(preview.error)}</p>
          ) : (
            <Loader2 className="mx-auto size-5 animate-spin text-muted-foreground" />
          )}
        </div>
      )}
    </SectionCard>
  )
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="space-y-2">
      <h3 className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">{title}</h3>
      <div className="grid grid-cols-2 gap-3">{children}</div>
    </section>
  )
}

/** Logo or signature: picked file is stored at once (normalized PNG), shown small. */
function AssetField({ kind, label, hint, pick, onChange }: { kind: PracticeAsset; label: string; hint: string; pick: string; onChange: () => void }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const image = useQuery(q.practiceAsset(kind))
  const done = () => {
    void qc.invalidateQueries({ queryKey: ['practice', 'asset', kind] })
    onChange()
  }
  const set = useMutation({
    mutationFn: async () => {
      const path = await openDialog({ multiple: false, filters: [{ name: t('settings.imageFilter'), extensions: ['png', 'jpg', 'jpeg', 'webp'] }] })
      if (typeof path !== 'string') return
      await api.setPracticeAsset(kind, path)
    },
    onSuccess: done,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const remove = useMutation({ mutationFn: () => api.removePracticeAsset(kind), onSuccess: done, onError: (e) => toast.error(errorMessage(e)) })
  return (
    <div className="col-span-2 space-y-1">
      <Label className="text-xs">{label}</Label>
      <div className="flex items-center gap-3">
        <div className="flex h-14 w-28 items-center justify-center rounded border bg-white p-1">
          {image.data && <img src={image.data} alt="" className="max-h-full max-w-full object-contain" />}
        </div>
        <Button type="button" variant="outline" size="sm" onClick={() => set.mutate()} disabled={set.isPending} data-testid={`practice-${kind}-pick`}>
          <ImagePlus /> {pick}
        </Button>
        {image.data && (
          <Button type="button" variant="ghost" size="sm" onClick={() => remove.mutate()} disabled={remove.isPending}>
            <X /> {t('settings.logoRemove')}
          </Button>
        )}
      </div>
      <p className="text-[11px] text-muted-foreground">{hint}</p>
    </div>
  )
}
