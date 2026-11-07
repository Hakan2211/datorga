// Template editor (slice 51): her own letter or Bericht template. Name, type, reader and letter
// frame (Betreff, reference lines, closing sentence), then the sections with their hints and the
// Lotse's questions. Each save is a new version; letters written earlier keep theirs.
import { useEffect, useState, type ReactNode } from 'react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { ArrowDown, ArrowUp, Plus, Trash2, X } from 'lucide-react'
import type { Audience } from '@/bindings/Audience'
import type { Category } from '@/bindings/Category'
import type { PatientRef } from '@/bindings/PatientRef'
import type { RecipientKind } from '@/bindings/RecipientKind'
import type { SectionDef } from '@/bindings/SectionDef'
import type { Template } from '@/bindings/Template'
import { Page, PageHeader } from '@/components/app-shell'
import { EmptyState, FilterChip } from '@/components/bits'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage, q } from '@/lib/api'
import { PLACEHOLDERS, chipsFromText, emptySection, getDraft, hintsFromText, hintsToText, isOwn, move, newAsk, templateProblems } from '@/lib/templates'

type Search = { key?: string; draft?: string }

export const Route = createFileRoute('/templates/edit')({
  validateSearch: (s: Record<string, unknown>): Search => ({
    ...(typeof s.key === 'string' ? { key: s.key } : {}),
    ...(typeof s.draft === 'string' ? { draft: s.draft } : {}),
  }),
  component: TemplateEditorPage,
})

const RECIPIENTS: RecipientKind[] = ['rentenversicherung', 'pflegekasse', 'versorgungsamt', 'arzt', 'krankenkasse', 'sonstige']
const AUDIENCES: Audience[] = ['behoerde', 'arzt', 'patient', 'frei']
const REF_FROM: (PatientRef | 'none')[] = ['none', 'pension_no', 'insurance_no', 'insurer']

function TemplateEditorPage() {
  const { t } = useTranslation()
  const { key, draft } = Route.useSearch()
  const templates = useQuery(q.templates())
  const [def, setDef] = useState<Template | null>(() => (draft ? (getDraft(draft) ?? null) : null))
  useEffect(() => {
    if (!def && key && templates.data) setDef(templates.data.find((x) => x.key === key) ?? null)
  }, [def, key, templates.data])

  if (!def) {
    return (
      <>
        <PageHeader title={t('tplEditor.title')} />
        <Page>{(templates.isFetched || draft) && <EmptyState title={t('tplEditor.missing')} />}</Page>
      </>
    )
  }
  return <Editor def={def} setDef={setDef} saved={!!key && !draft} />
}

function Editor({ def, setDef, saved }: { def: Template; setDef: (t: Template) => void; saved: boolean }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [dirty, setDirty] = useState(!saved)
  const [leftover, setLeftover] = useState<string[] | null>(null)
  const update = (patch: Partial<Template>) => {
    setDef({ ...def, ...patch })
    setDirty(true)
    setLeftover(null)
  }
  const setSection = (i: number, patch: Partial<SectionDef>) => update({ sections: def.sections.map((s, j) => (j === i ? { ...s, ...patch } : s)) })
  const problems = templateProblems(def)

  const save = useMutation({
    mutationFn: () => api.templateSave(def),
    onSuccess: (t2) => {
      setDef(t2)
      setDirty(false)
      setLeftover(null)
      void qc.invalidateQueries({ queryKey: ['templates'] })
      toast.success(t('tplEditor.saved', { version: t2.version }))
      void navigate({ to: '/templates/edit', search: { key: t2.key }, replace: true })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  // A template is used for every patient: names found in it are listed first.
  const check = useMutation({
    mutationFn: () => api.templateCheck(def),
    onSuccess: (found) => (found.length ? setLeftover(found) : save.mutate()),
    onError: (e) => toast.error(errorMessage(e)),
  })

  return (
    <>
      <PageHeader
        title={def.title || t('tplEditor.title')}
        subtitle={
          <span>
            <Link to="/templates" className="hover:underline">
              {t('templates.title')}
            </Link>{' '}
            · {t('tplEditor.subtitle')}
            {saved && !dirty && ` · ${t('tplEditor.version', { version: def.version })}`}
          </span>
        }
        actions={
          <>
            {dirty && <Badge variant="outline">{t('tplEditor.unsaved')}</Badge>}
            <Button
              disabled={!dirty || problems.length > 0 || save.isPending || check.isPending || !isOwn(def)}
              onClick={() => (leftover ? save.mutate() : check.mutate())}
              data-testid="tpl-save"
            >
              {leftover ? t('style.saveAnyway') : t('common.save')}
            </Button>
          </>
        }
      />
      <Page>
        {leftover && (
          <p className="rounded-md bg-warning/15 px-3 py-2 text-[13px]" data-testid="tpl-leftover">
            {t('tplEditor.names', { count: leftover.length, list: leftover.slice(0, 6).join(', ') })}
          </p>
        )}
        {problems.length > 0 && (
          <ul className="rounded-md bg-warning/15 px-3 py-2 text-[13px]" data-testid="tpl-problems">
            {problems.map((p) => (
              <li key={p}>{t(`tplEditor.problem.${p}`)}</li>
            ))}
          </ul>
        )}

        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('tplEditor.general')}</CardTitle>
          </CardHeader>
          <CardContent className="grid grid-cols-2 gap-4 text-sm">
            <Field label={t('tplEditor.name')}>
              <Input value={def.title} onChange={(e) => update({ title: e.target.value })} data-testid="tpl-title" />
            </Field>
            <Field label={t('tplEditor.description')}>
              <Input value={def.description ?? ''} onChange={(e) => update({ description: e.target.value })} placeholder={t('tplEditor.descriptionPlaceholder')} />
            </Field>
            <Field label={t('tplEditor.categoryLabel')} hint={t('tplEditor.categoryHint')}>
              <Select value={def.category} onValueChange={(v) => update({ category: v as Category, audience: v === 'bericht' ? (def.audience ?? 'frei') : def.audience })}>
                <SelectTrigger data-testid="tpl-category">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {(['bericht', 'brief'] as Category[]).map((c) => (
                    <SelectItem key={c} value={c}>
                      {t(`tplEditor.category.${c}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
            <Field label={t('tplEditor.audienceLabel')} hint={t('tplEditor.audienceHint')}>
              <Select value={def.audience ?? 'none'} onValueChange={(v) => update({ audience: v === 'none' ? null : (v as Audience) })}>
                <SelectTrigger data-testid="tpl-audience">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {def.category === 'brief' && <SelectItem value="none">{t('tplEditor.audienceNone')}</SelectItem>}
                  {AUDIENCES.map((a) => (
                    <SelectItem key={a} value={a}>
                      {t(`tplEditor.audience.${a}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
            <Field label={t('tplEditor.recipients')} hint={t('tplEditor.recipientsHint')} wide>
              <div className="flex flex-wrap gap-1.5">
                {RECIPIENTS.map((r) => (
                  <FilterChip
                    key={r}
                    active={def.recipients.includes(r)}
                    onClick={() => update({ recipients: def.recipients.includes(r) ? def.recipients.filter((x) => x !== r) : [...def.recipients, r] })}
                  >
                    {t(`enums.recipient_kind.${r}`)}
                  </FilterChip>
                ))}
              </div>
            </Field>
            <label className="col-span-2 flex items-center gap-2">
              <Switch checked={def.release} onCheckedChange={(v) => update({ release: v })} data-testid="tpl-release" />
              {t('tplEditor.release')}
            </label>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('tplEditor.frame')}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4 text-sm">
            <Field label={t('tplEditor.subject')} hint={t('tplEditor.placeholdersHint')}>
              <Input value={def.subject ?? ''} onChange={(e) => update({ subject: e.target.value })} data-testid="tpl-subject" />
              <Placeholders onPick={(p) => update({ subject: `${def.subject ?? ''}${def.subject?.endsWith(' ') || !def.subject ? '' : ' '}${p}` })} />
            </Field>
            <div className="space-y-1.5">
              <Label>{t('tplEditor.refs')}</Label>
              {def.refs.map((r, i) => (
                <div key={i} className="flex items-center gap-2" data-testid="tpl-ref">
                  <Input
                    value={r.label}
                    onChange={(e) => update({ refs: def.refs.map((x, j) => (j === i ? { ...x, label: e.target.value } : x)) })}
                    placeholder={t('tplEditor.refLabel')}
                    className="flex-1"
                  />
                  <Select
                    value={r.from ?? 'none'}
                    onValueChange={(v) => update({ refs: def.refs.map((x, j) => (j === i ? { ...x, from: v === 'none' ? null : (v as PatientRef) } : x)) })}
                  >
                    <SelectTrigger className="w-56">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {REF_FROM.map((f) => (
                        <SelectItem key={f} value={f}>
                          {t(`tplEditor.refFrom.${f}`)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <label className="flex items-center gap-1.5 text-xs text-muted-foreground">
                    <Switch checked={r.required} onCheckedChange={(v) => update({ refs: def.refs.map((x, j) => (j === i ? { ...x, required: v } : x)) })} />
                    {t('tplEditor.required')}
                  </label>
                  <Button variant="ghost" size="icon" aria-label={t('common.delete')} onClick={() => update({ refs: def.refs.filter((_, j) => j !== i) })}>
                    <X />
                  </Button>
                </div>
              ))}
              <Button variant="outline" size="sm" onClick={() => update({ refs: [...def.refs, { label: '', required: false }] })} data-testid="tpl-add-ref">
                <Plus /> {t('tplEditor.addRef')}
              </Button>
            </div>
            <Field label={t('tplEditor.closing')}>
              <Input value={def.closing ?? ''} onChange={(e) => update({ closing: e.target.value })} placeholder={t('tplEditor.closingPlaceholder')} />
            </Field>
            <Field label={t('tplEditor.pages')}>
              <Input
                type="number"
                min={1}
                max={20}
                className="w-24"
                value={def.rules.target_pages ?? ''}
                onChange={(e) => update({ rules: { ...def.rules, target_pages: e.target.value ? Math.max(1, Math.min(20, Number(e.target.value))) : null } })}
              />
            </Field>
          </CardContent>
        </Card>

        <div className="flex items-center justify-between">
          <h2 className="text-sm font-medium text-muted-foreground">{t('tplEditor.sections', { count: def.sections.length })}</h2>
          <Button
            variant="outline"
            size="sm"
            onClick={() => update({ sections: [...def.sections, emptySection(def.sections.map((s) => s.key), t('tplEditor.newSection'))] })}
            data-testid="tpl-add-section"
          >
            <Plus /> {t('tplEditor.addSection')}
          </Button>
        </div>
        {def.sections.map((s, i) => (
          <SectionEditor
            key={s.key}
            s={s}
            first={i === 0}
            last={i === def.sections.length - 1}
            onChange={(patch) => setSection(i, patch)}
            onMove={(by) => update({ sections: move(def.sections, i, by) })}
            onDelete={def.sections.length > 1 ? () => update({ sections: def.sections.filter((_, j) => j !== i) }) : undefined}
          />
        ))}
      </Page>
    </>
  )
}

function SectionEditor({
  s,
  first,
  last,
  onChange,
  onMove,
  onDelete,
}: {
  s: SectionDef
  first: boolean
  last: boolean
  onChange: (patch: Partial<SectionDef>) => void
  onMove: (by: -1 | 1) => void
  onDelete?: () => void
}) {
  const { t } = useTranslation()
  const diagnoses = s.field === 'diagnoses'
  // Hints are edited as lines; the text is kept as typed so blank lines don't jump away.
  const [hints, setHints] = useState(hintsToText(s.hints))
  return (
    <Card data-testid="tpl-section">
      <CardContent className="space-y-3 pt-4 text-sm">
        <div className="flex items-center gap-2">
          <Input
            value={s.heading}
            onChange={(e) => onChange({ heading: e.target.value })}
            placeholder={t('tplEditor.headingPlaceholder')}
            className="flex-1 font-medium"
            data-testid="tpl-heading"
          />
          <Button variant="ghost" size="icon" disabled={first} aria-label={t('tplEditor.up')} onClick={() => onMove(-1)}>
            <ArrowUp />
          </Button>
          <Button variant="ghost" size="icon" disabled={last} aria-label={t('tplEditor.down')} onClick={() => onMove(1)}>
            <ArrowDown />
          </Button>
          {onDelete && (
            <Button variant="ghost" size="icon" aria-label={t('tplEditor.deleteSection')} onClick={onDelete} data-testid="tpl-delete-section">
              <Trash2 />
            </Button>
          )}
        </div>
        <div className="flex flex-wrap gap-x-5 gap-y-2 text-[13px]">
          <Toggle
            checked={diagnoses}
            onChange={(v) => onChange(v ? { field: 'diagnoses', ai_allowed: false, asks: [], starter: null } : { field: 'prose', ai_allowed: true })}
            label={t('tplEditor.diagnosesField')}
            testId="tpl-diagnoses"
          />
          {!diagnoses && <Toggle checked={s.ai_allowed} onChange={(v) => onChange({ ai_allowed: v, ...(v ? {} : { asks: [] }) })} label={t('tplEditor.aiAllowed')} />}
          <Toggle checked={s.required} onChange={(v) => onChange({ required: v, ...(v ? { optional: false } : {}) })} label={t('tplEditor.requiredSection')} />
          <Toggle checked={s.optional} onChange={(v) => onChange({ optional: v, ...(v ? { required: false } : {}) })} label={t('tplEditor.optional')} />
        </div>
        {diagnoses ? (
          <p className="text-[13px] text-muted-foreground">{t('tplEditor.diagnosesNote')}</p>
        ) : (
          <>
            <Field label={t('tplEditor.starter')} hint={t('tplEditor.starterHint')}>
              <Textarea rows={2} value={s.starter ?? ''} onChange={(e) => onChange({ starter: e.target.value, ...(e.target.value.trim() ? { asks: [] } : {}) })} className="text-[13px]" />
              <Placeholders onPick={(p) => onChange({ starter: `${s.starter ?? ''}${p}` })} />
            </Field>
            <Field label={t('tplEditor.hints')} hint={t('tplEditor.hintsHint')}>
              <Textarea
                rows={Math.max(2, s.hints.length + 1)}
                value={hints}
                onChange={(e) => {
                  setHints(e.target.value)
                  onChange({ hints: hintsFromText(e.target.value, s.hints) })
                }}
                className="text-[13px]"
                data-testid="tpl-hints"
              />
            </Field>
            {s.ai_allowed && !s.starter?.trim() && (
              <div className="space-y-1.5">
                <Label>{t('tplEditor.asks')}</Label>
                <p className="text-[11px] text-muted-foreground">{t('tplEditor.asksHint')}</p>
                {s.asks.map((a, i) => (
                  <div key={a.key} className="flex items-center gap-2" data-testid="tpl-ask">
                    <Input
                      value={a.question}
                      onChange={(e) => onChange({ asks: s.asks.map((x, j) => (j === i ? { ...x, question: e.target.value } : x)) })}
                      placeholder={t('tplEditor.question')}
                      className="flex-[2]"
                    />
                    <Input
                      defaultValue={a.chips.join(', ')}
                      onChange={(e) => onChange({ asks: s.asks.map((x, j) => (j === i ? { ...x, chips: chipsFromText(e.target.value) } : x)) })}
                      placeholder={t('tplEditor.chips')}
                      className="flex-1"
                    />
                    <Button variant="ghost" size="icon" aria-label={t('common.delete')} onClick={() => onChange({ asks: s.asks.filter((_, j) => j !== i) })}>
                      <X />
                    </Button>
                  </div>
                ))}
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => onChange({ asks: [...s.asks, { ...newAsk(`frage ${s.asks.length + 1}`, s.asks.map((a) => a.key)), question: '' }] })}
                  data-testid="tpl-add-ask"
                >
                  <Plus /> {t('tplEditor.addAsk')}
                </Button>
              </div>
            )}
          </>
        )}
      </CardContent>
    </Card>
  )
}

function Field({ label, hint, wide, children }: { label: string; hint?: string; wide?: boolean; children: ReactNode }) {
  return (
    <div className={wide ? 'col-span-2 space-y-1.5' : 'space-y-1.5'}>
      <Label>{label}</Label>
      {children}
      {hint && <p className="text-[11px] text-muted-foreground">{hint}</p>}
    </div>
  )
}

function Toggle({ checked, onChange, label, testId }: { checked: boolean; onChange: (v: boolean) => void; label: string; testId?: string }) {
  return (
    <label className="flex items-center gap-2">
      <Switch checked={checked} onCheckedChange={onChange} data-testid={testId} />
      {label}
    </label>
  )
}

function Placeholders({ onPick }: { onPick: (p: string) => void }) {
  return (
    <div className="flex flex-wrap gap-1">
      {PLACEHOLDERS.map((p) => (
        <button key={p} type="button" className="rounded border px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground hover:bg-muted" onClick={() => onPick(p)}>
          {p}
        </button>
      ))}
    </div>
  )
}
