import { useRef, useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { Editor } from '@tiptap/react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import type { TextBlock } from '@/bindings/TextBlock'
import type { TextBlockInput } from '@/bindings/TextBlockInput'
import { RichField } from '@/components/rich-field'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { api, errorMessage, q } from '@/lib/api'
import { isBlank } from '@/lib/report'
import { PLACEHOLDERS, letterTemplates } from '@/lib/letters'

const NO_SECTION = '__none__'

/**
 * New or edit a Textbaustein. `initial` prefills a new one (from the letter: the selected text,
 * the letter's type and the section it came from). Editing a built-in block makes it her own.
 */
export function TextBlockFormDialog({
  block,
  initial,
  trigger,
  open: controlled,
  onOpenChange,
  onSaved,
  note,
}: {
  block?: TextBlock
  /** A line under the title (e.g. that names were replaced by placeholders). */
  note?: string
  initial?: Partial<TextBlockInput>
  trigger?: ReactNode
  open?: boolean
  onOpenChange?: (open: boolean) => void
  onSaved?: (b: TextBlock) => void
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [own, setOwn] = useState(false)
  const open = controlled ?? own
  const setOpen = (o: boolean) => {
    setOwn(o)
    onOpenChange?.(o)
  }
  const templates = useQuery(q.templates())
  const types = letterTemplates(templates.data ?? [], null, ['bericht', 'brief']).map((x) => x.template)
  const init = (): TextBlockInput => ({
    title: block?.title ?? initial?.title ?? '',
    body: block?.body ?? initial?.body ?? '',
    template_keys: block?.template_keys ?? initial?.template_keys ?? [],
    section_key: block?.section_key ?? initial?.section_key ?? null,
  })
  const [form, setForm] = useState(init)
  // The body editor is made once per opening; its content then lives in `form.body`.
  const [round, setRound] = useState(0)
  const editor = useRef<Editor | null>(null)

  // A section can be named when exactly one type is chosen.
  const only = form.template_keys.length === 1 ? types.find((x) => x.key === form.template_keys[0]) : undefined
  const sections = only?.sections.filter((s) => s.field === 'prose') ?? []

  const save = useMutation({
    mutationFn: () => {
      const input: TextBlockInput = { ...form, section_key: only ? form.section_key : null }
      return block ? api.updateTextblock(block.id, input) : api.createTextblock(input)
    },
    onSuccess: (b) => {
      void qc.invalidateQueries({ queryKey: ['textblocks'] })
      toast.success(t('common.saved'))
      setOpen(false)
      onSaved?.(b)
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  const toggleType = (key: string, on: boolean) =>
    setForm((f) => ({ ...f, template_keys: on ? [...f.template_keys, key] : f.template_keys.filter((k) => k !== key) }))

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o)
        if (o) {
          setForm(init())
          setRound((r) => r + 1)
        }
      }}
    >
      {trigger && <DialogTrigger asChild>{trigger}</DialogTrigger>}
      <DialogContent className="sm:max-w-2xl" data-testid="textblock-form">
        <DialogHeader>
          <DialogTitle>{block ? t('textblocks.edit') : t('textblocks.new')}</DialogTitle>
          {block?.origin === 'builtin' && <DialogDescription>{t('textblocks.builtinEditNote')}</DialogDescription>}
          {note && <DialogDescription data-testid="textblock-note">{note}</DialogDescription>}
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="tb-title">{t('textblocks.titleLabel')}</Label>
            <Input
              id="tb-title"
              value={form.title}
              onChange={(e) => setForm({ ...form, title: e.target.value })}
              placeholder={t('textblocks.titlePlaceholder')}
              data-testid="textblock-title"
            />
          </div>
          <div className="space-y-1.5">
            <Label>{t('textblocks.body')}</Label>
            <div data-testid="textblock-body">
              <RichField
                key={round}
                value={form.body}
                onChange={(body) => setForm((f) => ({ ...f, body }))}
                onEditor={(e) => (editor.current = e)}
              />
            </div>
            <div className="flex flex-wrap items-center gap-1 pt-1">
              <span className="mr-1 text-xs text-muted-foreground">{t('textblocks.placeholders')}</span>
              {PLACEHOLDERS.map((p) => (
                <button
                  key={p}
                  type="button"
                  className="rounded border bg-muted/50 px-1.5 py-0.5 font-mono text-[11px] hover:bg-muted"
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => editor.current?.chain().focus().insertContent(p).run()}
                >
                  {p}
                </button>
              ))}
            </div>
            <p className="text-xs text-muted-foreground">{t('textblocks.placeholdersHint')}</p>
          </div>
          <div className="space-y-1.5">
            <Label>{t('textblocks.types')}</Label>
            <p className="text-xs text-muted-foreground">{t('textblocks.typesHint')}</p>
            <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 pt-1">
              {types.map((x) => (
                <label key={x.key} className="flex items-center gap-2 text-sm">
                  <Checkbox checked={form.template_keys.includes(x.key)} onCheckedChange={(c) => toggleType(x.key, c === true)} />
                  {x.title}
                </label>
              ))}
            </div>
          </div>
          {only && sections.length > 1 && (
            <div className="space-y-1.5">
              <Label>{t('textblocks.section')}</Label>
              <Select
                value={form.section_key ?? NO_SECTION}
                onValueChange={(v) => setForm({ ...form, section_key: v === NO_SECTION ? null : v })}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NO_SECTION}>{t('textblocks.anySection')}</SelectItem>
                  {sections.map((s) => (
                    <SelectItem key={s.key} value={s.key}>
                      {s.heading || t('textblocks.intro_section')}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)}>
            {t('common.cancel')}
          </Button>
          <Button
            onClick={() => save.mutate()}
            disabled={save.isPending || !form.title.trim() || isBlank(form.body)}
            data-testid="textblock-save"
          >
            {t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
