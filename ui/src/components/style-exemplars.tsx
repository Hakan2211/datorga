// Her style exemplars (slice 49): old letters and Berichte that show the AI how she writes.
// Adding one: text (pasted, or a file read locally with the Windows text recognition) →
// redaction review (every mark replaced by a placeholder unless she unmarks it; she marks what was
// missed) → the redacted text, trimmed to the letter, saved for a document type. Only that text
// is kept; it goes along with drafts of the type as "Stilbeispiel", never as a source.
import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { ClipboardPaste, EyeOff, Feather, NotebookPen, Pencil, Plus } from 'lucide-react'
import type { EntityKind } from '@/bindings/EntityKind'
import type { StyleExemplar } from '@/bindings/StyleExemplar'
import type { StyleReview } from '@/bindings/StyleReview'
import type { Template } from '@/bindings/Template'
import { SectionCard, formatDate } from '@/components/bits'
import { DeleteButton } from '@/components/delete-button'
import { DocumentButton } from '@/components/document-button'
import { ProfileLine, StyleNotesDialog } from '@/components/style-profile'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage, q } from '@/lib/api'
import { placeholderCount, reviewParts, styleTypes, toggleKeep } from '@/lib/documents'
import { ALL_TYPES, mappedLabel } from '@/lib/style-profile'
import { cn } from '@/lib/utils'

type Step = 'text' | 'redact' | 'save'

/** What a new exemplar starts from (a signed Bericht or letter of hers). */
export type StyleStart = { text: string; template_key?: string; title?: string }

const KINDS: EntityKind[] = ['person', 'ort', 'arbeitgeber', 'datum']

/** New exemplar (from `start` or from scratch) or edit `exemplar` (its stored, redacted text). */
export function StyleExemplarDialog({
  exemplar,
  start,
  trigger,
  open: controlled,
  onOpenChange,
}: {
  exemplar?: StyleExemplar
  start?: StyleStart
  trigger?: ReactNode
  open?: boolean
  onOpenChange?: (open: boolean) => void
}) {
  const [own, setOwn] = useState(false)
  const open = controlled ?? own
  const setOpen = (o: boolean) => {
    setOwn(o)
    onOpenChange?.(o)
  }
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      {trigger && <DialogTrigger asChild>{trigger}</DialogTrigger>}
      {open && (
        <DialogContent className="max-h-[92vh] grid-rows-[auto_minmax(0,1fr)_auto] sm:max-w-3xl" data-testid="style-dialog">
          <StyleForm exemplar={exemplar} start={start} onDone={() => setOpen(false)} />
        </DialogContent>
      )}
    </Dialog>
  )
}

function StyleForm({ exemplar, start, onDone }: { exemplar?: StyleExemplar; start?: StyleStart; onDone: () => void }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const templates = useQuery(q.templates())
  const list = useQuery(q.styleExemplars())
  const types = styleTypes(templates.data ?? [])
  const [step, setStep] = useState<Step>(exemplar ? 'save' : start?.text ? 'redact' : 'text')
  const [source, setSource] = useState(start?.text ?? '')
  const [sha, setSha] = useState<string | null>(null)
  const [extra, setExtra] = useState<[EntityKind, string][]>([])
  const [keep, setKeep] = useState<string[]>([])
  const [reviewed, setReviewed] = useState<StyleReview | null>(null)
  const [text, setText] = useState(exemplar?.text ?? '')
  const [templateKey, setTemplateKey] = useState(exemplar?.template_key ?? start?.template_key ?? '')
  const [title, setTitle] = useState(exemplar?.title ?? start?.title ?? '')
  const [leftover, setLeftover] = useState<string[] | null>(null)

  const duplicate = sha ? list.data?.find((e) => e.source_sha256 === sha && e.template_key === templateKey) : undefined

  const save = useMutation({
    mutationFn: () => {
      const input = { template_key: templateKey, title, text, source_sha256: sha }
      return exemplar ? api.updateStyleExemplar(exemplar.id, input, exemplar.active) : api.createStyleExemplar(input)
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['style'] })
      void qc.invalidateQueries({ queryKey: ['draftSources'] })
      void qc.invalidateQueries({ queryKey: ['letter'] })
      toast.success(t('style.saved'))
      onDone()
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  // Before saving: is anything still recognized as identifying (beyond what she kept)?
  const check = useMutation({
    mutationFn: () => api.styleReview(text, [], keep),
    onSuccess: (r) => {
      const left = [...new Set(r.marks.filter((m) => !keep.some((k) => k.toLowerCase() === m.text.toLowerCase())).map((m) => m.text))]
      if (left.length) setLeftover(left)
      else save.mutate()
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  useEffect(() => setLeftover(null), [text])

  return (
    <>
      <DialogHeader>
        <DialogTitle className="flex items-center gap-2">
          <Feather className="size-5 text-primary" /> {exemplar ? t('style.editTitle') : t('style.dialogTitle')}
        </DialogTitle>
        {!exemplar && (
          <DialogDescription className="flex gap-3 text-xs">
            {(['text', 'redact', 'save'] as Step[]).map((s, i) => (
              <span key={s} className={cn(step === s ? 'font-medium text-foreground' : 'text-muted-foreground')}>
                {i + 1}. {t(`style.step${s === 'text' ? 'Text' : s === 'redact' ? 'Redact' : 'Save'}`)}
              </span>
            ))}
          </DialogDescription>
        )}
      </DialogHeader>

      <div className="min-h-0 space-y-3 overflow-y-auto pr-1 text-sm">
        {step === 'text' && (
          <>
            <div className="flex items-center justify-between gap-2">
              <Label htmlFor="style-source">{t('style.textLabel')}</Label>
              <DocumentButton
                purpose="style"
                onRead={(i) => {
                  setSource(i.text)
                  setSha(i.sha256)
                  if (!title) setTitle(i.file_name.replace(/\.[^.]+$/, ''))
                }}
                label={t('style.upload')}
                testId="style-file"
              />
            </div>
            <Textarea
              id="style-source"
              rows={16}
              value={source}
              onChange={(e) => {
                setSource(e.target.value)
                setSha(null)
              }}
              placeholder={t('style.textPlaceholder')}
              className="text-[13px]"
              data-testid="style-source"
            />
          </>
        )}

        {step === 'redact' && (
          <RedactReview source={source} extra={extra} setExtra={setExtra} keep={keep} setKeep={setKeep} onReview={setReviewed} />
        )}

        {step === 'save' && (
          <>
            {!exemplar && <p className="text-[13px] text-muted-foreground">{t('style.saveIntro')}</p>}
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label>{t('style.type')}</Label>
                <Select value={templateKey} onValueChange={setTemplateKey}>
                  <SelectTrigger data-testid="style-type">
                    <SelectValue placeholder={t('style.typePlaceholder')} />
                  </SelectTrigger>
                  <SelectContent>
                    {types.map((x: Template) => (
                      <SelectItem key={x.key} value={x.key}>
                        {x.title}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="style-title">{t('style.titleLabel')}</Label>
                <Input id="style-title" value={title} onChange={(e) => setTitle(e.target.value)} placeholder={t('style.titlePlaceholder')} data-testid="style-title" />
              </div>
            </div>
            <Textarea rows={16} value={text} onChange={(e) => setText(e.target.value)} className="text-[13px]" data-testid="style-text" />
            <p className="text-[11px] text-muted-foreground">
              {t('style.placeholders', { count: placeholderCount(text) })} · {t('style.chars', { count: text.length })}
            </p>
            {duplicate && <p className="text-[13px] text-warning-foreground">{t('style.duplicate', { title: duplicate.title })}</p>}
            {leftover && (
              <p className="rounded-md bg-warning/15 px-3 py-2 text-[13px]" data-testid="style-leftover">
                {t('style.stillMarked', { count: leftover.length, list: leftover.slice(0, 6).join(', ') })}
              </p>
            )}
          </>
        )}
      </div>

      <DialogFooter>
        {step === 'redact' && (
          <Button variant="outline" className="mr-auto" onClick={() => setStep('text')}>
            {t('style.back')}
          </Button>
        )}
        {step === 'save' && !exemplar && (
          <Button variant="outline" className="mr-auto" onClick={() => setStep('redact')}>
            {t('style.back')}
          </Button>
        )}
        <Button variant="outline" onClick={onDone}>
          {t('common.cancel')}
        </Button>
        {step === 'text' && (
          <Button disabled={!source.trim()} onClick={() => setStep('redact')} data-testid="style-next">
            {t('style.next')}
          </Button>
        )}
        {step === 'redact' && (
          <Button
            disabled={!reviewed}
            onClick={() => {
              setText(reviewed!.redacted)
              setStep('save')
            }}
            data-testid="style-next"
          >
            {t('style.next')}
          </Button>
        )}
        {step === 'save' && (
          <Button
            disabled={!templateKey || !title.trim() || !text.trim() || save.isPending || check.isPending || !!duplicate}
            onClick={() => (leftover ? save.mutate() : check.mutate())}
            data-testid="style-save"
          >
            {leftover ? t('style.saveAnyway') : t('common.save')}
          </Button>
        )}
      </DialogFooter>
    </>
  )
}

/**
 * The redaction review of an uploaded text (slice 49, also the template builder's): every mark
 * is replaced by a placeholder unless she clicks it to keep it; she adds what was missed.
 * `onReview` gets the current result (`null` while it is computed).
 */
export function RedactReview({
  source,
  extra,
  setExtra,
  keep,
  setKeep,
  onReview,
}: {
  source: string
  extra: [EntityKind, string][]
  setExtra: (f: (x: [EntityKind, string][]) => [EntityKind, string][]) => void
  keep: string[]
  setKeep: (f: (k: string[]) => string[]) => void
  onReview: (r: StyleReview | null) => void
}) {
  const { t } = useTranslation()
  const [term, setTerm] = useState('')
  const [kind, setKind] = useState<EntityKind>('person')
  const review = useQuery({
    queryKey: ['styleReview', source, extra, keep],
    queryFn: () => api.styleReview(source, extra, keep),
    enabled: !!source.trim(),
    placeholderData: keepPreviousData,
    gcTime: 0,
  })
  useEffect(() => onReview(review.data && !review.isFetching ? review.data : null), [review.data, review.isFetching, onReview])
  const parts = useMemo(() => (review.data ? reviewParts(source, review.data.marks, keep) : []), [review.data, source, keep])
  const marked = parts.filter((p) => p.mark && !p.kept).length
  const kept = parts.filter((p) => p.mark && p.kept).length
  const addTerm = () => {
    const v = term.trim()
    if (!v) return
    setExtra((x) => [...x, [kind, v]])
    setKeep((k) => k.filter((x) => x.toLowerCase() !== v.toLowerCase()))
    setTerm('')
  }
  return (
    <>
      <p className="text-[13px] text-muted-foreground">{t('style.redactIntro')}</p>
      <p className="flex flex-wrap gap-2 text-[13px]" data-testid="style-counts">
        <Badge variant="secondary">{t('style.marks', { count: marked })}</Badge>
        {kept > 0 && <Badge variant="outline">{t('style.kept', { count: kept })}</Badge>}
      </p>
      <div
        className="max-h-[45vh] overflow-y-auto rounded-md border bg-muted/30 p-3 text-[13px] leading-relaxed whitespace-pre-wrap"
        onMouseUp={() => {
          const sel = document.getSelection()?.toString().trim() ?? ''
          if (sel && sel.length < 80) setTerm(sel)
        }}
        data-testid="style-review"
      >
        {review.isError && <p className="text-destructive">{errorMessage(review.error)}</p>}
        {parts.map((p, i) =>
          p.mark ? (
            <button
              key={i}
              type="button"
              title={t(`style.kind.${p.mark.kind}`)}
              onClick={() => setKeep((k) => toggleKeep(k, p.mark!.text))}
              className={cn(
                'rounded px-0.5',
                p.kept ? 'bg-transparent underline decoration-dotted' : 'bg-primary/15 text-primary line-through decoration-primary/40',
              )}
              data-testid="style-mark"
            >
              {p.text}
            </button>
          ) : (
            <span key={i}>{p.text}</span>
          ),
        )}
      </div>
      <div className="space-y-1.5">
        <Label className="text-xs">{t('style.addTerm')}</Label>
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault()
            addTerm()
          }}
        >
          <Select value={kind} onValueChange={(v) => setKind(v as EntityKind)}>
            <SelectTrigger className="w-36">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {KINDS.map((k) => (
                <SelectItem key={k} value={k}>
                  {t(`style.kind.${k}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Input value={term} onChange={(e) => setTerm(e.target.value)} placeholder={t('style.addTermPlaceholder')} className="flex-1" data-testid="style-term" />
          <Button type="submit" variant="outline" disabled={!term.trim()}>
            <EyeOff /> {t('style.addTermButton')}
          </Button>
        </form>
        <p className="text-[11px] text-muted-foreground">{t('style.selectHint')}</p>
      </div>
    </>
  )
}

/** "Ihr Schreibstil" on the Vorlagen page: her exemplars per type, add, pause, edit, delete. */
export function StyleExemplarsCard({ add = false }: { add?: boolean } = {}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const list = useQuery(q.styleExemplars())
  const templates = useQuery(q.templates())
  const profiles = useQuery(q.styleProfiles())
  const profile = (key: string) => profiles.data?.find((p) => p.template_key === key)
  const title = (key: string) => templates.data?.find((x) => x.key === key)?.title ?? key
  const [adding, setAdding] = useState(add)
  useEffect(() => {
    if (add) setAdding(true)
  }, [add])
  const [notesFor, setNotesFor] = useState<string | null>(null)
  const [editing, setEditing] = useState<StyleExemplar | null>(null)
  const toggle = useMutation({
    mutationFn: (e: StyleExemplar) =>
      api.updateStyleExemplar(e.id, { template_key: e.template_key, title: e.title, text: e.text, source_sha256: e.source_sha256 }, !e.active),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['style'] })
      void qc.invalidateQueries({ queryKey: ['draftSources'] })
      void qc.invalidateQueries({ queryKey: ['letter'] })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })

  // Grouped by type, in the order of the type list; a type with only notes shows too.
  const groups = useMemo(() => {
    const by = new Map<string, StyleExemplar[]>()
    for (const e of list.data ?? []) by.set(e.template_key, [...(by.get(e.template_key) ?? []), e])
    for (const p of profiles.data ?? []) if (p.template_key !== ALL_TYPES && p.notes && !by.has(p.template_key)) by.set(p.template_key, [])
    const order = styleTypes(templates.data ?? []).map((x) => x.key)
    return [...by.entries()].sort((a, b) => order.indexOf(a[0]) - order.indexOf(b[0]))
  }, [list.data, templates.data, profiles.data])
  const allNotes = profile(ALL_TYPES)?.notes ?? ''

  return (
    <SectionCard
      icon={Feather}
      title={t('style.title')}
      description={t('style.intro')}
      actions={
        <>
          <Button size="sm" variant="outline" onClick={() => setNotesFor(ALL_TYPES)} data-testid="style-notes-open">
            <NotebookPen /> {t('styleProfile.button')}
          </Button>
          <Button size="sm" onClick={() => setAdding(true)} data-testid="style-add">
            <Plus /> {t('style.add')}
          </Button>
        </>
      }
      contentClassName="space-y-4 text-sm"
    >
      {list.data?.length === 0 && (
        <div className="flex flex-col items-start gap-3 rounded-md border border-dashed p-4">
          <p className="text-muted-foreground">{t('style.empty')}</p>
          <div className="flex gap-2">
            <Button variant="outline" size="sm" onClick={() => setAdding(true)}>
              <ClipboardPaste /> {t('style.paste')}
            </Button>
          </div>
        </div>
      )}
      {allNotes && (
        <p className="flex items-center gap-2 text-[13px]" data-testid="style-notes-all">
          <span className="min-w-0 flex-1 truncate">
            <span className="text-muted-foreground">{t('styleProfile.allTypes')}: </span>
            {allNotes}
          </span>
          <Button variant="ghost" size="sm" onClick={() => setNotesFor(ALL_TYPES)}>
            <Pencil /> {t('style.edit')}
          </Button>
        </p>
      )}
      {groups.map(([key, items]) => (
        <div key={key} className="space-y-1.5" data-testid="style-group">
          <div className="flex items-end justify-between gap-2">
            <div className="min-w-0">
              <p className="text-xs font-medium text-muted-foreground">
                {title(key)} · {t('style.count', { count: items.length })}
              </p>
              <ProfileLine computed={profile(key)?.computed} notes={profile(key)?.notes ?? ''} />
            </div>
            <Button variant="ghost" size="sm" className="h-7 shrink-0 text-xs" onClick={() => setNotesFor(key)} data-testid="style-profile-open">
              <NotebookPen className="size-3.5" /> {t('styleProfile.open')}
            </Button>
          </div>
          {items.length > 0 && <ul className="divide-y rounded-md border">
            {items.map((e) => (
              <li key={e.id} className="flex items-center gap-3 px-3 py-2" data-testid="style-item">
                <div className="min-w-0 flex-1">
                  <p className="truncate font-medium">{e.title}</p>
                  <p className="text-xs text-muted-foreground">
                    {[formatDate(e.created_at), t('style.chars', { count: e.text.length }), t('style.placeholders', { count: placeholderCount(e.text) }), mappedLabel(profile(key)?.mapped, e.id, t)]
                      .filter(Boolean)
                      .join(' · ')}
                  </p>
                </div>
                <label className="flex items-center gap-2 text-xs text-muted-foreground">
                  <Switch checked={e.active} onCheckedChange={() => toggle.mutate(e)} data-testid="style-active" />
                  {e.active ? t('style.active') : t('style.paused')}
                </label>
                <Button variant="ghost" size="icon" aria-label={t('style.edit')} title={t('style.edit')} onClick={() => setEditing(e)}>
                  <Pencil />
                </Button>
                <DeleteButton
                  title={t('style.deleteTitle')}
                  description={t('style.deleteBody', { title: e.title })}
                  action={() => api.deleteStyleExemplar(e.id)}
                  testId="style-delete"
                />
              </li>
            ))}
          </ul>}
        </div>
      ))}
      {!!list.data?.length && <p className="text-[11px] text-muted-foreground">{t('style.used')}</p>}
      <StyleExemplarDialog open={adding} onOpenChange={setAdding} />
      {editing && <StyleExemplarDialog exemplar={editing} open onOpenChange={(o) => !o && setEditing(null)} />}
      {notesFor && <StyleNotesDialog templateKey={notesFor} onClose={() => setNotesFor(null)} />}
    </SectionCard>
  )
}

/** "Als Stilbeispiel übernehmen" for a signed Bericht or letter: its text into the review. */
export function StyleFromReportButton({ reportId }: { reportId: string }) {
  const { t } = useTranslation()
  const [start, setStart] = useState<StyleStart | null>(null)
  const load = useMutation({
    mutationFn: () => api.styleSource(reportId),
    onSuccess: (s) => setStart(s),
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <>
      <Button variant="outline" size="sm" title={t('style.fromReportHint')} disabled={load.isPending} onClick={() => load.mutate()} data-testid="style-from-report">
        <Feather /> {t('style.fromReport')}
      </Button>
      {start && <StyleExemplarDialog start={start} open onOpenChange={(o) => !o && setStart(null)} />}
    </>
  )
}
