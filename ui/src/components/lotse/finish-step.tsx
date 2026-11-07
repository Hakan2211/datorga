import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { AlertTriangle, CheckCircle2, Download, FileSignature, Send } from 'lucide-react'
import { formatDate } from '@/components/bits'
import { SendDialog } from '@/components/briefe/send-dialog'
import { useReportExport } from '@/components/report-export'
import { ReportPreview } from '@/components/report-preview'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { api, errorMessage, q, type ReportView } from '@/lib/api'
import { germanDay } from '@/lib/letters'

/**
 * Lotse step 5: the Bericht as it will be sent, the signature, and the export. Signing needs
 * every blocking check of step 4 passed; exporting before signing is possible, marked "unsigniert".
 */
export function FinishStep({
  view,
  content,
  flush,
  busy,
  onCheck,
}: {
  view: ReportView
  /** Changes whenever the text changes (re-renders the preview). */
  content: string
  flush: () => Promise<void>
  /** Edits are still being saved. */
  busy: boolean
  onCheck: () => void
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const { report } = view
  const signed = report.status === 'signed' || report.status === 'exported'
  const checklist = useQuery(q.checklist(report.id))
  const therapist = useQuery(q.setting<string>('therapist_name'))
  const [signer, setSigner] = useState<string | null>(null)
  const signAs = signer ?? therapist.data ?? ''
  const exportAs = useReportExport(view)
  const blocking = checklist.data?.filter((c) => !c.ok && c.severity === 'block') ?? []
  // A letter-type Bericht signs as a letter (a typed-in recipient goes into the address book) and
  // is marked as sent afterwards.
  const isLetter = report.kind === 'brief'
  const letter = useQuery({ ...q.letter(report.id), enabled: isLetter })
  const offerSave = !!letter.data && !letter.data.recipient_id && !!letter.data.recipient.name.trim()
  const [saveRecipient, setSaveRecipient] = useState(true)
  const [sending, setSending] = useState(false)
  const afterLetter = () => {
    for (const key of [['letter', report.id], ['letters'], ['recipients'], ['timeline', view.patient.id], ['reports']]) {
      void qc.invalidateQueries({ queryKey: key })
    }
  }

  const sign = useMutation({
    mutationFn: async () => {
      await flush()
      if (isLetter) await api.signLetter(report.id, signAs.trim(), offerSave && saveRecipient)
      else await api.sign(report.id, signAs.trim())
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['report', report.id] })
      void qc.invalidateQueries({ queryKey: ['reports'] })
      if (isLetter) afterLetter()
      toast.success(t('enums.status.signed'))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const unsend = useMutation({
    mutationFn: () => api.markLetterSent(report.id, null, null),
    onSuccess: afterLetter,
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,7fr)_minmax(0,4fr)]" data-testid="lotse-finish">
      <div className="h-[calc(100vh-14rem)] min-h-96">
        <ReportPreview reportId={report.id} content={content} flush={flush} />
      </div>
      <div className="space-y-4 self-start">
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('lotse.finish.signTitle')}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            {signed ? (
              <p className="flex items-start gap-2 rounded-md bg-success/10 px-3 py-2.5 text-sm text-success" data-testid="finish-signed">
                <CheckCircle2 className="mt-0.5 size-4 shrink-0" />
                {t('lotse.finish.signed', { name: report.signed_by, date: formatDate(report.signed_at, true) })}
              </p>
            ) : (
              <>
                <p className="text-[13px] text-muted-foreground">{t('lotse.finish.signHint')}</p>
                <div className="space-y-1.5">
                  <Label htmlFor="signer" className="text-xs">
                    {t('report.signAs')}
                  </Label>
                  <Input id="signer" value={signAs} onChange={(e) => setSigner(e.target.value)} placeholder={t('lotse.finish.signerPlaceholder')} />
                </div>
                {offerSave && (
                  <label className="flex items-center gap-2 text-[13px]">
                    <Checkbox checked={saveRecipient} onCheckedChange={(v) => setSaveRecipient(v === true)} />
                    {t('briefe.saveRecipient')}
                  </label>
                )}
                {blocking.length > 0 && (
                  <p className="flex items-start gap-2 text-[13px] text-destructive" data-testid="finish-blocked">
                    <AlertTriangle className="mt-0.5 size-4 shrink-0" />
                    <span>
                      {t('lotse.finish.blocked', { count: blocking.length })}{' '}
                      <button type="button" className="underline underline-offset-2" onClick={onCheck}>
                        {t('lotse.finish.toCheck')}
                      </button>
                    </span>
                  </p>
                )}
                <Button
                  className="w-full"
                  disabled={busy || blocking.length > 0 || !signAs.trim() || sign.isPending}
                  onClick={() => sign.mutate()}
                  data-testid="finish-sign"
                >
                  <FileSignature /> {t('report.sign')}
                </Button>
              </>
            )}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('lotse.finish.exportTitle')}</CardTitle>
          </CardHeader>
          <CardContent className="grid gap-2">
            {!signed && (
              <p className="flex items-start gap-2 rounded-md bg-warning/12 px-3 py-2 text-[13px]" data-testid="finish-unsigned">
                <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning-foreground" />
                {t('lotse.finish.unsigned')}
              </p>
            )}
            {report.export_outdated && (
              <p className="flex items-start gap-2 text-sm text-warning-foreground">
                <AlertTriangle className="mt-0.5 size-4 shrink-0" />
                {t('note.exportOutdatedSince', { date: formatDate(report.exported_at, true) })}
              </p>
            )}
            {report.exported_at && !report.export_outdated && (
              <p className="text-[13px] text-muted-foreground">{t('lotse.finish.exportedAt', { date: formatDate(report.exported_at, true) })}</p>
            )}
            <Button variant={signed ? 'default' : 'outline'} onClick={() => exportAs.mutate('pdf')} disabled={exportAs.isPending} data-testid="finish-pdf">
              <Download /> {signed ? t('report.exportPdf') : t('lotse.finish.exportPdfUnsigned')}
            </Button>
            <Button variant="outline" onClick={() => exportAs.mutate('docx')} disabled={exportAs.isPending} data-testid="finish-docx">
              <Download /> {signed ? t('report.exportDocx') : t('lotse.finish.exportDocxUnsigned')}
            </Button>
          </CardContent>
        </Card>
        {isLetter && letter.data && (
          <Card>
            <CardHeader>
              <CardTitle className="text-base">{t('lotse.letter.sendTitle')}</CardTitle>
            </CardHeader>
            <CardContent className="space-y-2 text-sm">
              {letter.data.sent_at ? (
                <p className="flex flex-wrap items-center gap-x-2 text-[13px] text-muted-foreground" data-testid="finish-sent">
                  <Send className="size-3.5" />
                  {t('briefe.sentOn', {
                    date: germanDay(letter.data.sent_at),
                    via: letter.data.sent_via ? t(`enums.sent_via.${letter.data.sent_via}`) : '',
                  })}
                  <Button variant="link" size="sm" className="h-auto p-0 text-xs" onClick={() => unsend.mutate()}>
                    {t('briefe.unmarkSent')}
                  </Button>
                </p>
              ) : signed ? (
                <Button className="w-full" variant="outline" onClick={() => setSending(true)} data-testid="finish-mark-sent">
                  <Send /> {t('briefe.markSent')}
                </Button>
              ) : (
                <p className="text-[13px] text-muted-foreground">{t('lotse.letter.sendHint')}</p>
              )}
            </CardContent>
          </Card>
        )}
      </div>
      {sending && <SendDialog reportId={report.id} onClose={() => setSending(false)} onSent={afterLetter} />}
    </div>
  )
}
