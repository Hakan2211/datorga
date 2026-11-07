// Schweigepflichtentbindung (slice 46): recorded from paper, per address book entry or kind of
// recipient. A letter's checks warn when its recipient has none.
import { useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { save } from '@tauri-apps/plugin-dialog'
import { Printer, ShieldCheck } from 'lucide-react'
import type { Consent } from '@/bindings/Consent'
import type { RecipientKind } from '@/bindings/RecipientKind'
import { SectionCard, formatDate } from '@/components/bits'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { DatePicker } from '@/components/ui/date-picker'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { api, errorMessage, q } from '@/lib/api'
import { RECIPIENT_KINDS, isoDay } from '@/lib/letters'

/** Who a new release is for: one book entry (if the letter has one) or a kind of recipient. */
type Target = { recipientId: string; name: string } | { kind: RecipientKind }

export function useReleaseRefresh(patientId: string, reportId?: string) {
  const qc = useQueryClient()
  return () => {
    void qc.invalidateQueries({ queryKey: ['consents', patientId] })
    if (reportId) void qc.invalidateQueries({ queryKey: ['checklist', reportId] })
  }
}

export function ReleaseDialog({
  patientId,
  recipientId,
  recipientName,
  kind,
  reportId,
  trigger,
}: {
  patientId: string
  /** The letter's address book entry, if it has one. */
  recipientId?: string | null
  recipientName?: string
  kind?: RecipientKind | null
  /** The letter whose checks change. */
  reportId?: string
  trigger: ReactNode
}) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const own: Target | null = recipientId ? { recipientId, name: recipientName ?? '' } : null
  const [target, setTarget] = useState<'own' | RecipientKind>(own ? 'own' : (kind ?? 'rentenversicherung'))
  const [day, setDay] = useState(isoDay())
  const [note, setNote] = useState('')
  const refresh = useReleaseRefresh(patientId, reportId)
  const grant = useMutation({
    mutationFn: () =>
      api.grantRelease(patientId, {
        recipient_id: target === 'own' ? (recipientId ?? null) : null,
        scope: target === 'own' ? null : target,
        granted_on: day || null,
        note: note || null,
      }),
    onSuccess: () => {
      toast.success(t('release.saved'))
      refresh()
      setOpen(false)
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const printForm = useMutation({
    mutationFn: async () => {
      const path = await save({ defaultPath: t('release.file'), filters: [{ name: 'PDF', extensions: ['pdf'] }] })
      if (!path) return false
      await api.exportConsentForm(patientId, 'release_of_information', path)
      return true
    },
    onSuccess: (done) => done && toast.success(t('consent.formSaved')),
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o)
        if (o) {
          setTarget(own ? 'own' : (kind ?? 'rentenversicherung'))
          setDay(isoDay())
          setNote('')
        }
      }}
    >
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent className="sm:max-w-md" data-testid="release-dialog">
        <DialogHeader>
          <DialogTitle>{t('release.dialogTitle')}</DialogTitle>
          <DialogDescription>{t('release.dialogText')}</DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label className="text-xs">{t('briefe.recipient')}</Label>
            <Select value={target} onValueChange={(v) => setTarget(v as 'own' | RecipientKind)}>
              <SelectTrigger className="w-full" data-testid="release-target">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {own && <SelectItem value="own">{t('release.forRecipient', { name: own.name })}</SelectItem>}
                {RECIPIENT_KINDS.map((k) => (
                  <SelectItem key={k} value={k}>
                    {t('release.forKind', { kind: t(`enums.recipient_kind.${k}`) })}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1.5">
            <Label className="text-xs" htmlFor="release-day">
              {t('release.grantedOn')}
            </Label>
            <DatePicker id="release-day" value={day} max={isoDay()} onChange={(d) => setDay(d ?? '')} />
          </div>
          <Input placeholder={t('release.note')} value={note} onChange={(e) => setNote(e.target.value)} />
          <Button variant="link" size="sm" className="h-auto p-0 text-xs" onClick={() => printForm.mutate()} disabled={printForm.isPending}>
            <Printer className="size-3.5" /> {t('consent.printForm')}
          </Button>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)}>
            {t('common.cancel')}
          </Button>
          <Button onClick={() => grant.mutate()} disabled={grant.isPending} data-testid="release-save">
            <ShieldCheck /> {t('release.add')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** Patient page: the Schweigepflichtentbindungen on record, with revoke and "Erfassen". */
export function ReleaseCard({ patientId }: { patientId: string }) {
  const { t } = useTranslation()
  const consents = useQuery(q.consents(patientId))
  const recipients = useQuery(q.recipients())
  const refresh = useReleaseRefresh(patientId)
  const revoke = useMutation({
    mutationFn: (id: string) => api.revokeConsent(id),
    onSuccess: refresh,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const releases = consents.data?.filter((c) => c.kind === 'release_of_information') ?? []
  const label = (c: Consent) =>
    c.recipient_id
      ? (recipients.data?.find((r) => r.id === c.recipient_id)?.name ?? t('release.deletedRecipient'))
      : t('release.kindAll', { kind: c.scope ? t(`enums.recipient_kind.${c.scope}`) : '' })

  return (
    <SectionCard
      title={t('release.title')}
      actions={
        <ReleaseDialog
          patientId={patientId}
          trigger={
            <Button size="sm" variant="outline" data-testid="release-add">
              <ShieldCheck /> {t('release.add')}
            </Button>
          }
        />
      }
      contentClassName="space-y-2"
    >
      <p className="text-sm text-muted-foreground">{t('release.subtitle')}</p>
      {releases.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t('release.none')}</p>
      ) : (
        <ul className="divide-y text-sm" data-testid="release-list">
          {releases.map((c) => (
            <li key={c.id} className="flex flex-wrap items-center gap-x-3 gap-y-1 py-1.5">
              <span className={c.revoked_at ? 'text-muted-foreground line-through' : 'font-medium'}>{label(c)}</span>
              <span className="text-muted-foreground">
                {t('consent.grantedAt')} {formatDate(c.granted_at)}
              </span>
              {c.revoked_at && (
                <span className="text-destructive">
                  {t('consent.revokedAt')} {formatDate(c.revoked_at)}
                </span>
              )}
              {c.note && <span className="text-muted-foreground">{c.note}</span>}
              {!c.revoked_at && (
                <AlertDialog>
                  <AlertDialogTrigger asChild>
                    <Button size="sm" variant="ghost" className="ml-auto h-7 px-2 text-xs text-destructive">
                      {t('consent.revoke')}
                    </Button>
                  </AlertDialogTrigger>
                  <AlertDialogContent>
                    <AlertDialogHeader>
                      <AlertDialogTitle>{t('consent.revoke')}</AlertDialogTitle>
                      <AlertDialogDescription>{t('release.revokeConfirm')}</AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                      <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
                      <AlertDialogAction variant="destructive" onClick={() => revoke.mutate(c.id)}>
                        {t('consent.revoke')}
                      </AlertDialogAction>
                    </AlertDialogFooter>
                  </AlertDialogContent>
                </AlertDialog>
              )}
            </li>
          ))}
        </ul>
      )}
    </SectionCard>
  )
}
