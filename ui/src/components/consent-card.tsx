import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { save } from '@tauri-apps/plugin-dialog'
import { Mic, MicOff, Printer, ShieldCheck } from 'lucide-react'
import type { Consent } from '@/bindings/Consent'
import { formatDate } from '@/components/bits'
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
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { api, errorMessage, q } from '@/lib/api'

/** Recording consent for one patient: current state, grant/revoke, history. */
export function ConsentCard({ patientId }: { patientId: string }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const consents = useQuery(q.consents(patientId))
  const active = consents.data?.find((c) => c.kind === 'recording' && !c.revoked_at)

  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: ['consents', patientId] })
    void qc.invalidateQueries({ queryKey: ['patient', patientId] })
    void qc.invalidateQueries({ queryKey: ['patients'] })
  }
  const revoke = useMutation({
    mutationFn: (id: string) => api.revokeConsent(id),
    onSuccess: invalidate,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const printForm = useMutation({
    mutationFn: async () => {
      // No patient name in the file name: the file may end up in a downloads folder.
      const path = await save({ defaultPath: 'Einwilligung-Tonaufnahme.pdf', filters: [{ name: 'PDF', extensions: ['pdf'] }] })
      if (!path) return false
      await api.exportConsentForm(patientId, 'recording', path)
      return true
    },
    onSuccess: (done) => done && toast.success(t('consent.formSaved')),
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0">
        <CardTitle className="flex items-center gap-3 text-base">
          {t('consent.title')}
          {active ? (
            <Badge variant="secondary" className="bg-success/15 font-normal text-success">
              <Mic /> {t('consent.active')}
            </Badge>
          ) : (
            <Badge variant="secondary" className="font-normal">
              <MicOff /> {t('consent.none')}
            </Badge>
          )}
        </CardTitle>
        {active ? (
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button size="sm" variant="outline" className="text-destructive">
                {t('consent.revoke')}
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>{t('consent.revoke')}</AlertDialogTitle>
                <AlertDialogDescription>{t('consent.revokeConfirm')}</AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
                <AlertDialogAction
                  className="bg-destructive hover:bg-destructive/90"
                  onClick={() => revoke.mutate(active.id)}
                >
                  {t('consent.revoke')}
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        ) : (
          <GrantConsentDialog patientId={patientId} onGranted={invalidate} />
        )}
      </CardHeader>
      <CardContent className="space-y-3">
        <p className="text-sm text-muted-foreground">{active ? t('consent.activeHint') : t('consent.noneHint')}</p>
        <Button size="sm" variant="outline" onClick={() => printForm.mutate()} disabled={printForm.isPending}>
          <Printer /> {t('consent.printForm')}
        </Button>
        {!!consents.data?.length && <ConsentHistory consents={consents.data} />}
      </CardContent>
    </Card>
  )
}

function ConsentHistory({ consents }: { consents: Consent[] }) {
  const { t } = useTranslation()
  return (
    <div>
      <h3 className="mb-1 text-xs font-medium text-muted-foreground">{t('consent.history')}</h3>
      <ul className="divide-y text-sm">
        {consents.map((c) => (
          <li key={c.id} className="flex flex-wrap items-center gap-x-3 py-1.5">
            <span>
              {t('consent.grantedAt')} {formatDate(c.granted_at, true)}
            </span>
            {c.revoked_at && (
              <span className="text-destructive">
                {t('consent.revokedAt')} {formatDate(c.revoked_at, true)}
              </span>
            )}
            {c.note && <span className="text-muted-foreground">{c.note}</span>}
            <span className="ml-auto font-mono text-xs text-muted-foreground" title={c.text_hash}>
              {t('consent.version')} {c.text_version}
            </span>
          </li>
        ))}
      </ul>
    </div>
  )
}

function GrantConsentDialog({ patientId, onGranted }: { patientId: string; onGranted: () => void }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [confirmed, setConfirmed] = useState(false)
  const [note, setNote] = useState('')
  const text = useQuery({ ...q.consentText('recording'), enabled: open })
  const grant = useMutation({
    mutationFn: () => api.grantConsent(patientId, 'recording', note),
    onSuccess: () => {
      onGranted()
      setOpen(false)
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  const onOpenChange = (o: boolean) => {
    setOpen(o)
    if (o) {
      setConfirmed(false)
      setNote('')
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogTrigger asChild>
        <Button size="sm">
          <ShieldCheck /> {t('consent.grant')}
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t('consent.grantTitle')}</DialogTitle>
        </DialogHeader>
        <p className="text-sm text-muted-foreground">{t('consent.paperHint')}</p>
        <p className="text-sm text-muted-foreground">{t('consent.shown')}</p>
        <div className="max-h-80 space-y-3 overflow-y-auto rounded-md border bg-muted/30 p-4 text-sm">
          {text.data?.text.split('\n\n').map((para, i) => <p key={i}>{para}</p>)}
        </div>
        {text.data && (
          <p className="font-mono text-xs text-muted-foreground">
            {t('consent.version')} {text.data.version}
          </p>
        )}
        <div className="space-y-1.5">
          <Label htmlFor="consent-note">{t('consent.note')}</Label>
          <Input id="consent-note" value={note} onChange={(e) => setNote(e.target.value)} />
        </div>
        <Label className="flex items-start gap-2 font-normal">
          <Checkbox checked={confirmed} onCheckedChange={(v) => setConfirmed(v === true)} className="mt-0.5" />
          {t('consent.confirm')}
        </Label>
        <DialogFooter>
          <Button onClick={() => grant.mutate()} disabled={!confirmed || !text.data || grant.isPending}>
            {t('consent.grant')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
