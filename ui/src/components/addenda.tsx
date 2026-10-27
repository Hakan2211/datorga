import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { FileSignature } from 'lucide-react'
import type { ReportAddendum } from '@/bindings/ReportAddendum'
import { formatDate } from '@/components/bits'
import { RichField } from '@/components/rich-field'
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
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { api, errorMessage, q } from '@/lib/api'

/** Nachträge to a signed Sitzungsnotiz (§ 630f BGB): earlier ones read-only, a new one signed on submit. */
export function Addenda({
  reportId,
  addenda,
  noteSigner,
}: {
  reportId: string
  addenda: ReportAddendum[]
  /** Falls back to whoever signed the note when no therapist name is set. */
  noteSigner: string | null
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const therapist = useQuery(q.setting<string>('therapist_name'))
  const [signer, setSigner] = useState<string | null>(null)
  const signAs = signer ?? (therapist.data || noteSigner || '')
  const [body, setBody] = useState('')

  const add = useMutation({
    mutationFn: () => api.addAddendum(reportId, body, signAs),
    onSuccess: () => {
      setBody('')
      void qc.invalidateQueries({ queryKey: ['report', reportId] })
      toast.success(t('addendum.signed'))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <section className="space-y-4 border-t pt-6">
      <h2 className="font-serif text-[17px] leading-snug font-semibold">{t('addendum.title')}</h2>
      {addenda.map((a) => (
        <div key={a.id} className="space-y-2">
          <h3 className="text-sm font-medium">
            {t('addendum.heading', { date: formatDate(a.created_at, true), signer: a.signed_by })}
          </h3>
          <RichField value={a.body} onChange={() => {}} readOnly />
        </div>
      ))}
      <div className="space-y-3 rounded-md border bg-card p-3">
        <Label className="text-sm">{t('addendum.add')}</Label>
        <p className="text-xs text-muted-foreground">{t('addendum.hint')}</p>
        <RichField value={body} onChange={setBody} placeholder={t('addendum.placeholder')} />
        <div className="flex items-end gap-2">
          <div className="flex-1 space-y-1.5">
            <Label htmlFor="addendum-signer" className="text-xs">
              {t('report.signAs')}
            </Label>
            <Input id="addendum-signer" value={signAs} onChange={(e) => setSigner(e.target.value)} />
          </div>
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button disabled={!body || !signAs.trim() || add.isPending}>
                <FileSignature /> {t('addendum.sign')}
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>{t('addendum.confirmTitle')}</AlertDialogTitle>
                <AlertDialogDescription>{t('addendum.confirmBody')}</AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
                <AlertDialogAction onClick={() => add.mutate()}>{t('addendum.sign')}</AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </div>
      </div>
    </section>
  )
}
