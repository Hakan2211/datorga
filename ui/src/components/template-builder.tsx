// Template builder (slice 51): her own form or letter → her own Vorlage.
// Text (file, scan or pasted) → Schwärzen (the same review as style exemplars: names, dates and
// numbers become placeholders) → the outline the app reads from it, optionally refined by one AI
// request (hints and questions per section) → the template editor. Nothing is saved before she
// saves it in the editor, and the original file is not kept.
import { useState } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { FileStack, Sparkles } from 'lucide-react'
import type { EntityKind } from '@/bindings/EntityKind'
import type { StyleReview } from '@/bindings/StyleReview'
import type { Template } from '@/bindings/Template'
import { DocumentButton } from '@/components/document-button'
import { RedactionDialog } from '@/components/fact-draft'
import { RedactReview } from '@/components/style-exemplars'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage, q } from '@/lib/api'
import { putDraft } from '@/lib/templates'
import { cn } from '@/lib/utils'

type Step = 'text' | 'redact' | 'outline'

export function TemplateBuilderDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {open && (
        <DialogContent className="max-h-[92vh] grid-rows-[auto_minmax(0,1fr)_auto] sm:max-w-3xl" data-testid="builder-dialog">
          <Builder onDone={() => onOpenChange(false)} />
        </DialogContent>
      )}
    </Dialog>
  )
}

function Builder({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const llm = useQuery(q.llm())
  const [step, setStep] = useState<Step>('text')
  const [title, setTitle] = useState('')
  const [source, setSource] = useState('')
  const [extra, setExtra] = useState<[EntityKind, string][]>([])
  const [keep, setKeep] = useState<string[]>([])
  const [reviewed, setReviewed] = useState<StyleReview | null>(null)
  const [text, setText] = useState('')
  const [ai, setAi] = useState<Template | null>(null)
  const [asking, setAsking] = useState(false)

  const outline = useQuery({
    queryKey: ['templateOutline', text, title],
    queryFn: () => api.templateOutline(text, title),
    enabled: step === 'outline' && !!text.trim(),
    gcTime: 0,
    retry: false,
  })
  const draft = ai ?? outline.data
  const open = () => {
    if (!draft) return
    const id = putDraft(draft)
    onDone()
    void navigate({ to: '/templates/edit', search: { draft: id } })
  }

  return (
    <>
      <DialogHeader>
        <DialogTitle className="flex items-center gap-2">
          <FileStack className="size-5 text-primary" /> {t('builder.title')}
        </DialogTitle>
        <DialogDescription className="flex gap-3 text-xs">
          {(['text', 'redact', 'outline'] as Step[]).map((s, i) => (
            <span key={s} className={cn(step === s ? 'font-medium text-foreground' : 'text-muted-foreground')}>
              {i + 1}. {t(`builder.step.${s}`)}
            </span>
          ))}
        </DialogDescription>
      </DialogHeader>

      <div className="min-h-0 space-y-3 overflow-y-auto pr-1 text-sm">
        {step === 'text' && (
          <>
            <p className="text-[13px] text-muted-foreground">{t('builder.intro')}</p>
            <div className="space-y-1.5">
              <Label htmlFor="builder-title">{t('builder.name')}</Label>
              <Input id="builder-title" value={title} onChange={(e) => setTitle(e.target.value)} placeholder={t('builder.namePlaceholder')} data-testid="builder-title" />
            </div>
            <div className="flex items-center justify-between gap-2">
              <Label htmlFor="builder-source">{t('builder.textLabel')}</Label>
              <DocumentButton
                purpose="template"
                onRead={(i) => {
                  setSource(i.text)
                  if (!title) setTitle(i.file_name.replace(/\.[^.]+$/, ''))
                }}
                label={t('builder.upload')}
                testId="builder-file"
              />
            </div>
            <Textarea
              id="builder-source"
              rows={14}
              value={source}
              onChange={(e) => setSource(e.target.value)}
              placeholder={t('builder.textPlaceholder')}
              className="text-[13px]"
              data-testid="builder-source"
            />
          </>
        )}

        {step === 'redact' && <RedactReview source={source} extra={extra} setExtra={setExtra} keep={keep} setKeep={setKeep} onReview={setReviewed} />}

        {step === 'outline' && (
          <>
            {outline.isError && <p className="text-destructive">{errorMessage(outline.error)}</p>}
            {draft && <OutlineSummary t={draft} ai={!!ai} />}
            {draft && (
              <div className="flex flex-wrap items-center gap-3 rounded-md border bg-muted/30 p-3 text-[13px]">
                <Sparkles className="size-4 shrink-0 text-primary" />
                <p className="min-w-0 flex-1 text-muted-foreground">{llm.data?.config.active ? t('builder.aiHint') : t('builder.aiNone')}</p>
                <Button size="sm" variant="outline" disabled={!llm.data?.config.active} onClick={() => setAsking(true)} data-testid="builder-ai">
                  <Sparkles /> {ai ? t('builder.aiAgain') : t('builder.ai')}
                </Button>
              </div>
            )}
          </>
        )}
      </div>

      <DialogFooter>
        {step !== 'text' && (
          <Button variant="outline" className="mr-auto" onClick={() => setStep(step === 'outline' ? 'redact' : 'text')}>
            {t('style.back')}
          </Button>
        )}
        <Button variant="outline" onClick={onDone}>
          {t('common.cancel')}
        </Button>
        {step === 'text' && (
          <Button disabled={!source.trim()} onClick={() => setStep('redact')} data-testid="builder-next">
            {t('style.next')}
          </Button>
        )}
        {step === 'redact' && (
          <Button
            disabled={!reviewed}
            onClick={() => {
              setText(reviewed!.redacted)
              setAi(null)
              setStep('outline')
            }}
            data-testid="builder-next"
          >
            {t('style.next')}
          </Button>
        )}
        {step === 'outline' && (
          <Button disabled={!draft} onClick={open} data-testid="builder-open">
            {t('builder.open')}
          </Button>
        )}
      </DialogFooter>

      {asking && (
        <RedactionDialog
          reportId="builder"
          previewKey={['builderPreview', text, title]}
          loadPreview={() => api.builderPreview(text, title)}
          progressEvent="builder-progress"
          labels={{
            send: t('builder.send'),
            runLocal: t('builder.runLocal'),
            running: t('builder.running'),
            part: () => t('builder.running'),
            parts: () => '',
          }}
          onRun={async (digest, requestId) => {
            const r = await api.builderRun(text, title, digest, requestId)
            setAi(r)
            toast.success(t('builder.aiDone', { count: r.sections.length }))
          }}
          onClose={() => setAsking(false)}
        />
      )}
    </>
  )
}

/** What the app read: type, reader, Betreff, reference lines and the sections with their counts. */
function OutlineSummary({ t: tpl, ai }: { t: Template; ai: boolean }) {
  const { t } = useTranslation()
  return (
    <div className="space-y-3" data-testid="builder-outline">
      <p className="text-[13px] text-muted-foreground">{ai ? t('builder.outlineAi') : t('builder.outlineLocal')}</p>
      <p className="flex flex-wrap gap-2">
        <Badge variant="secondary">{t(`tplEditor.category.${tpl.category}`)}</Badge>
        {tpl.audience && <Badge variant="outline">{t(`tplEditor.audience.${tpl.audience}`)}</Badge>}
        {tpl.recipients.map((r) => (
          <Badge key={r} variant="outline">
            {t(`enums.recipient_kind.${r}`)}
          </Badge>
        ))}
      </p>
      {tpl.subject && (
        <p className="text-[13px]">
          <span className="text-muted-foreground">{t('tplEditor.subject')}: </span>
          {tpl.subject}
        </p>
      )}
      {tpl.refs.length > 0 && (
        <p className="text-[13px]">
          <span className="text-muted-foreground">{t('tplEditor.refs')}: </span>
          {tpl.refs.map((r) => r.label).join(', ')}
        </p>
      )}
      <ol className="divide-y rounded-md border">
        {tpl.sections.map((s) => (
          <li key={s.key} className="flex items-center gap-3 px-3 py-2" data-testid="builder-section">
            <span className="min-w-0 flex-1 truncate font-medium">{s.heading || t('letterAi.opening')}</span>
            {s.field === 'diagnoses' ? (
              <Badge variant="outline">{t('tplEditor.diagnosesField')}</Badge>
            ) : (
              <span className="shrink-0 text-xs text-muted-foreground">
                {t('builder.hints', { count: s.hints.length })} · {t('builder.questions', { count: s.asks.length })}
              </span>
            )}
          </li>
        ))}
      </ol>
    </div>
  )
}
