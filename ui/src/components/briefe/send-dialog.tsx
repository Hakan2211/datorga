// "Als versendet markieren": date and Weg of a signed letter.
import { useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import type { SentVia } from '@/bindings/SentVia'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { api, errorMessage } from '@/lib/api'
import { isoDay } from '@/lib/letters'

const SENT_VIA: SentVia[] = ['post', 'fax', 'email', 'kim', 'personal']

export function SendDialog({ reportId, onClose, onSent }: { reportId: string; onClose: () => void; onSent: () => void }) {
  const { t } = useTranslation()
  const [day, setDay] = useState(isoDay())
  const [via, setVia] = useState<SentVia>('post')
  const send = useMutation({
    mutationFn: () => api.markLetterSent(reportId, day, via),
    onSuccess: () => {
      onClose()
      onSent()
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t('briefe.sentTitle')}</DialogTitle>
        </DialogHeader>
        <div className="grid grid-cols-2 gap-3">
          <div className="space-y-1.5">
            <Label htmlFor="sent-at">{t('briefe.sentAt')}</Label>
            <Input id="sent-at" type="date" value={day} onChange={(e) => setDay(e.target.value)} />
          </div>
          <div className="space-y-1.5">
            <Label>{t('briefe.via')}</Label>
            <Select value={via} onValueChange={(v) => setVia(v as SentVia)}>
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {SENT_VIA.map((v) => (
                  <SelectItem key={v} value={v}>
                    {t(`enums.sent_via.${v}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button disabled={!day || send.isPending} onClick={() => send.mutate()} data-testid="letter-sent-confirm">
            {t('briefe.markSent')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
