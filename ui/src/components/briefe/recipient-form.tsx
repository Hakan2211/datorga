import { useState, type ReactNode } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import type { Recipient } from '@/bindings/Recipient'
import type { RecipientInput } from '@/bindings/RecipientInput'
import type { RecipientKind } from '@/bindings/RecipientKind'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage } from '@/lib/api'
import { RECIPIENT_KINDS, linesFromText } from '@/lib/letters'

/** New or edit an address book entry. */
export function RecipientFormDialog({ recipient, trigger }: { recipient?: Recipient; trigger: ReactNode }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [open, setOpen] = useState(false)
  const init = () => ({
    kind: recipient?.kind ?? ('sonstige' as RecipientKind),
    name: recipient?.name ?? '',
    department: recipient?.department ?? '',
    lines: recipient?.lines.join('\n') ?? '',
    salutation: recipient?.salutation ?? '',
  })
  const [form, setForm] = useState(init)
  const save = useMutation({
    mutationFn: () => {
      const input: RecipientInput = {
        kind: form.kind,
        name: form.name,
        department: form.department,
        lines: linesFromText(form.lines),
        salutation: form.salutation.trim() || null,
        default_template: recipient?.default_template ?? null,
      }
      return recipient ? api.updateRecipient(recipient.id, input) : api.createRecipient(input)
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['recipients'] })
      toast.success(t('common.saved'))
      setOpen(false)
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o)
        if (o) setForm(init())
      }}
    >
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{recipient ? t('briefe.recipients.edit') : t('briefe.recipients.new')}</DialogTitle>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault()
            save.mutate()
          }}
        >
          <div className="space-y-1.5">
            <Label>{t('briefe.kind')}</Label>
            <Select value={form.kind} onValueChange={(v) => setForm({ ...form, kind: v as RecipientKind })}>
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {RECIPIENT_KINDS.map((k) => (
                  <SelectItem key={k} value={k}>
                    {t(`enums.recipient_kind.${k}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="rcp-name">{t('briefe.name')}</Label>
            <Input id="rcp-name" required value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} data-testid="recipient-name" />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="rcp-dept">{t('briefe.department')}</Label>
            <Input id="rcp-dept" value={form.department} onChange={(e) => setForm({ ...form, department: e.target.value })} />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="rcp-lines">{t('briefe.addressLines')}</Label>
            <Textarea id="rcp-lines" rows={3} value={form.lines} onChange={(e) => setForm({ ...form, lines: e.target.value })} />
            <p className="text-xs text-muted-foreground">{t('briefe.addressHint')}</p>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="rcp-sal">{t('briefe.salutation')}</Label>
            <Input id="rcp-sal" value={form.salutation} onChange={(e) => setForm({ ...form, salutation: e.target.value })} />
            <p className="text-xs text-muted-foreground">{t('briefe.recipients.salutationHint')}</p>
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" disabled={save.isPending || !form.name.trim()}>
              {t('common.save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
