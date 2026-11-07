import { useCallback, useEffect, useRef, useState } from 'react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { save } from '@tauri-apps/plugin-dialog'
import { AlertTriangle, BookUser, Compass, CheckCircle2, Download, Eye, FileSignature, Plus, Send, Star, X } from 'lucide-react'
import type { Letter } from '@/bindings/Letter'
import type { LetterMeta } from '@/bindings/LetterMeta'
import type { RecipientKind } from '@/bindings/RecipientKind'
import { Page, PageHeader } from '@/components/app-shell'
import { SectionCard, formatDate } from '@/components/bits'
import { ActiveSectionCard, LetterAiCard, RequestCard, UnbackedCard } from '@/components/briefe/letter-ai'
import { LetterStatePill } from '@/components/briefe/letter-state'
import { RecipientPicker } from '@/components/briefe/recipient-picker'
import { ReleaseDialog } from '@/components/briefe/release'
import { SendDialog } from '@/components/briefe/send-dialog'
import { TextBlockTools } from '@/components/briefe/textblock-menu'
import { DeleteReport } from '@/components/delete-button'
import { StyleFromReportButton } from '@/components/style-exemplars'
import { ReportDocument } from '@/components/report-document'
import { ReportPreview } from '@/components/report-preview'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { Toggle } from '@/components/ui/toggle'
import { api, errorMessage, q, type ReportView } from '@/lib/api'
import { SlashKeys } from '@/lib/editor'
import { DEFAULT_SALUTATION, RECIPIENT_KINDS, metaOf, germanDay, isoDay, linesFromText, placeDate, practiceLine } from '@/lib/letters'
import { cn } from '@/lib/utils'

/** The letter editor's extra: "/" opens the Textbausteine at the cursor. */
const LETTER_EXTENSIONS = [SlashKeys]

export const Route = createFileRoute('/briefe/$briefId')({ component: LetterPage })

function LetterPage() {
  const { briefId } = Route.useParams()
  const view = useQuery({ ...q.report(briefId), staleTime: Infinity })
  const letter = useQuery({ ...q.letter(briefId), staleTime: Infinity })
  if (!view.data || !letter.data) return null
  const locked = view.data.report.status === 'signed' || view.data.report.status === 'exported'
  // Fresh local state after signing (read-only from then on).
  return <LetterEditor key={`${briefId}:${locked}`} view={view.data} letter={letter.data} />
}

const SAVE_DELAY_MS = 700
const NO_CASE = 'none'


function LetterEditor({ view, letter }: { view: ReportView; letter: Letter }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const { report, template, patient } = view
  const readOnly = report.status === 'signed' || report.status === 'exported'
  const reportId = report.id
  const practice = useQuery(q.practice())
  const logo = useQuery(q.practiceAsset('logo'))
  const therapist = useQuery(q.setting<string>('therapist_name'))
  const episodes = useQuery(q.episodes(patient.id))
  const linked = useQuery(q.patientRecipients(patient.id))
  const checklist = useQuery(q.checklist(reportId))
  const [preview, setPreview] = useState(false)
  const [active, setActive] = useState<string | null>(null)

  const refresh = useCallback(() => {
    void qc.invalidateQueries({ queryKey: ['checklist', reportId] })
    void qc.invalidateQueries({ queryKey: ['letters'] })
    void qc.invalidateQueries({ queryKey: ['timeline', patient.id] })
  }, [qc, reportId, patient.id])

  // ---- sections: debounced autosave, as in the Bericht editor ----
  const [bodies, setBodies] = useState<Record<string, string>>(() => Object.fromEntries(report.sections.map((s) => [s.key, s.body])))
  const queued = useRef<Record<string, string>>({})
  const timers = useRef<Record<string, ReturnType<typeof setTimeout>>>({})
  const [pending, setPending] = useState(0)
  const saveSection = useCallback(
    (key: string, body: string) => {
      delete queued.current[key]
      setPending((n) => n + 1)
      api
        .saveSection(reportId, key, body)
        .then(refresh)
        .catch((e) => toast.error(errorMessage(e)))
        .finally(() => setPending((n) => n - 1))
    },
    [reportId, refresh],
  )
  const update = useCallback(
    (key: string, body: string) => {
      setBodies((b) => ({ ...b, [key]: body }))
      queued.current[key] = body
      clearTimeout(timers.current[key])
      timers.current[key] = setTimeout(() => saveSection(key, body), SAVE_DELAY_MS)
    },
    [saveSection],
  )

  // ---- letter data: one debounced save for address, Betreff, references, dates ----
  const [meta, setMetaState] = useState<LetterMeta>(() => metaOf(letter))
  const [linesText, setLinesText] = useState(() => letter.recipient.lines.join('\n'))
  const metaQueued = useRef<LetterMeta | null>(null)
  const metaTimer = useRef<ReturnType<typeof setTimeout>>(undefined)
  const saveMeta = useCallback(
    (m: LetterMeta) => {
      metaQueued.current = null
      setPending((n) => n + 1)
      api
        .saveLetterMeta(reportId, m)
        .then((l) => {
          qc.setQueryData(q.letter(reportId).queryKey, l)
          refresh()
        })
        .catch((e) => toast.error(errorMessage(e)))
        .finally(() => setPending((n) => n - 1))
    },
    [qc, reportId, refresh],
  )
  const setMeta = (patch: Partial<LetterMeta>) => {
    setMetaState((m) => {
      const next = { ...m, ...patch }
      metaQueued.current = next
      clearTimeout(metaTimer.current)
      metaTimer.current = setTimeout(() => saveMeta(next), SAVE_DELAY_MS)
      return next
    })
  }

  const flush = useCallback(async () => {
    const waiting = Object.entries(queued.current)
    queued.current = {}
    for (const [key] of waiting) clearTimeout(timers.current[key])
    clearTimeout(metaTimer.current)
    const m = metaQueued.current
    metaQueued.current = null
    await Promise.all([
      ...waiting.map(([key, body]) => api.saveSection(reportId, key, body)),
      ...(m ? [api.saveLetterMeta(reportId, m).then((l) => qc.setQueryData(q.letter(reportId).queryKey, l))] : []),
    ])
    refresh()
  }, [qc, reportId, refresh])

  // After AI text or new request sections: the sections as stored now.
  const reload = useCallback(async () => {
    const v = await qc.fetchQuery({ ...q.report(reportId), staleTime: 0 })
    const fresh = Object.fromEntries(v.report.sections.map((s) => [s.key, s.body]))
    setBodies(fresh)
    void qc.invalidateQueries({ queryKey: ['letter', reportId] })
    refresh()
  }, [qc, reportId, refresh])

  // Leaving the page must not drop edits still waiting for their debounce.
  useEffect(
    () => () => {
      for (const [key, body] of Object.entries(queued.current)) {
        clearTimeout(timers.current[key])
        void api.saveSection(reportId, key, body)
      }
      clearTimeout(metaTimer.current)
      if (metaQueued.current) void api.saveLetterMeta(reportId, metaQueued.current)
    },
    [reportId],
  )

  // ---- sign, export, send ----
  const [signing, setSigning] = useState(false)
  const [sending, setSending] = useState(false)
  const afterChange = () => {
    void qc.invalidateQueries({ queryKey: ['report', reportId] })
    void qc.invalidateQueries({ queryKey: ['letter', reportId] })
    void qc.invalidateQueries({ queryKey: ['recipients'] })
    refresh()
  }
  const exportAs = useMutation({
    mutationFn: async (format: 'pdf' | 'docx') => {
      await flush()
      const day = (letter.letter_date ?? report.signed_at ?? isoDay()).slice(0, 10)
      const base = `${template.title}-${patient.last_name}-${day}${readOnly ? '' : '-unsigniert'}`.replace(/[\\/:*?"<>|]/g, '')
      const path = await save({ defaultPath: `${base}.${format}`, filters: [{ name: format.toUpperCase(), extensions: [format] }] })
      if (!path) return false
      await api.exportReport(reportId, format, path)
      return true
    },
    onSuccess: (done) => {
      if (!done) return
      toast.success(t('briefe.exported'))
      afterChange()
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const unsend = useMutation({
    mutationFn: () => api.markLetterSent(reportId, null, null),
    onSuccess: afterChange,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const remember = useMutation({
    mutationFn: (id: string) => api.linkRecipient(patient.id, id, null),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['recipients'] }),
    onError: (e) => toast.error(errorMessage(e)),
  })

  const p = practice.data
  const letterheadEmpty = practice.isSuccess && !p?.name.trim()
  const titles = (p?.title_lines ?? '').split('\n').map((l) => l.trim()).filter(Boolean)
  const signer = report.signed_by ?? therapist.data ?? ''
  const date = meta.letter_date ?? report.signed_at ?? isoDay()
  const content = `${report.status}:${JSON.stringify(bodies)}:${JSON.stringify(meta)}`
  const isLinked = !!meta.recipient_id && !!linked.data?.some((r) => r.id === meta.recipient_id)
  const name = `${patient.first_name} ${patient.last_name}`
  const prose = template.sections.find((s) => s.field === 'prose')
  const problems = checklist.data?.filter((c) => !c.ok) ?? []
  const blocked = problems.some((c) => c.severity === 'block')

  const head = (
    <div>
      <div className="doc-letterhead">
        {logo.data && <img src={logo.data} alt="" />}
        <div className="doc-letterhead-name">{p?.name}</div>
        {p?.subtitle && <div className="doc-letterhead-sub">{p.subtitle}</div>}
      </div>
      <div className="doc-window" data-testid="letter-window">
        <div className="doc-sender">{practiceLine(p)}</div>
        {[meta.recipient.name, meta.recipient.department, ...meta.recipient.lines].filter((l) => l.trim()).map((l, i) => (
          <div key={i}>{l}</div>
        ))}
      </div>
      <div className="doc-placedate">{placeDate(meta.place, date)}</div>
      <div className="doc-subject">
        {readOnly ? (
          meta.subject
        ) : (
          <input
            className="doc-inline font-bold"
            value={meta.subject}
            placeholder={t('briefe.subjectPlaceholder')}
            onChange={(e) => setMeta({ subject: e.target.value })}
            aria-label={t('briefe.subject')}
            data-testid="letter-subject"
          />
        )}
      </div>
      <div className="doc-refs">
        {meta.refs
          .filter((r) => r.value.trim())
          .map((r, i) => (
            <div key={i}>
              {r.label}: {r.value}
            </div>
          ))}
      </div>
      <div className="doc-salutation">
        {readOnly ? (
          meta.salutation
        ) : (
          <input
            className="doc-inline"
            value={meta.salutation}
            onChange={(e) => setMeta({ salutation: e.target.value })}
            aria-label={t('briefe.salutation')}
          />
        )}
      </div>
    </div>
  )
  const foot = (
    <div className="doc-closing">
      {template.closing && <p>{template.closing}</p>}
      <p className="mt-3">{t('briefe.greeting')}</p>
      <div className="h-12" />
      <div>{signer}</div>
      <div className="doc-closing-titles">
        {titles.map((l, i) => (
          <div key={i}>{l}</div>
        ))}
      </div>
    </div>
  )

  return (
    <div className="flex min-h-full flex-col">
      <PageHeader
        title={
          <span className="flex items-center gap-3">
            {template.title}
            <LetterStatePill state={letter.sent_at ? 'sent' : readOnly ? 'signed' : 'draft'} />
          </span>
        }
        subtitle={
          <span className="flex items-center gap-2">
            <Link to="/briefe" className="hover:underline">
              {t('briefe.title')}
            </Link>
            ·
            <Link to="/patients/$patientId" params={{ patientId: patient.id }} className="hover:underline">
              {name}
            </Link>
            <span className="text-xs">· {pending > 0 ? t('briefe.saving') : t('briefe.allSaved')}</span>
          </span>
        }
        actions={
          <>
            <Toggle variant="outline" size="sm" className="px-3" pressed={preview} onPressedChange={setPreview} data-testid="letter-preview-toggle">
              <Eye /> {t('briefe.preview')}
            </Toggle>
            {template.category === 'bericht' && (
              <Button asChild variant="outline" size="sm">
                <Link to="/reports/$reportId" params={{ reportId }} search={{ step: readOnly ? 'fertig' : 'start' }} data-testid="letter-guided">
                  <Compass /> {t('briefe.guided')}
                </Link>
              </Button>
            )}
            <DeleteReport report={report} label onDeleted={() => void navigate({ to: '/briefe' })} />
          </>
        }
      />
      <Page>
        <div className={cn('grid gap-6', preview ? 'grid-cols-[minmax(0,1fr)_minmax(0,1fr)]' : 'grid-cols-[minmax(0,1fr)_22rem]')}>
          <div className="min-w-0 space-y-4">
            {readOnly && (
              <p className="rounded-md bg-success/10 px-4 py-2 text-sm text-success">
                {t('briefe.readOnly', { name: report.signed_by, date: formatDate(report.signed_at, true) })}
              </p>
            )}
            {letterheadEmpty && (
              <div className="flex items-center justify-between gap-3 rounded-md border border-warning/40 bg-warning/10 px-4 py-2 text-sm">
                <span className="flex items-center gap-2">
                  <AlertTriangle className="size-4" /> {t('briefe.letterheadMissing')}
                </span>
                <Button asChild size="sm" variant="outline">
                  <Link to="/settings/allgemein">{t('briefe.letterheadSetup')}</Link>
                </Button>
              </div>
            )}
            <ReportDocument
              key={template.sections.map((s) => s.key).join(',')}
              sections={template.sections}
              bodies={bodies}
              readOnly={readOnly}
              head={head}
              foot={foot}
              onChange={update}
              onActive={setActive}
              onEditLocked={() => {}}
              dictateFor={reportId}
              extensions={LETTER_EXTENSIONS}
              toolbarExtra={(editor) => <TextBlockTools editor={editor} reportId={reportId} templateKey={template.key} patient={patient} />}
            />
          </div>
          {preview ? (
            <div className="sticky top-4 h-[calc(100vh-12rem)] min-h-96 self-start">
              <ReportPreview reportId={reportId} content={content} flush={flush} onClose={() => setPreview(false)} />
            </div>
          ) : (
            <aside className="space-y-4">
              {!readOnly && (
                <>
                  <UnbackedCard
                    sections={template.sections}
                    bodies={bodies}
                    onChange={update}
                    onShow={(key) => document.getElementById(`section-${key}`)?.scrollIntoView({ behavior: 'smooth', block: 'center' })}
                  />
                  <LetterAiCard view={view} letter={letter} bodies={bodies} flush={flush} onWritten={reload} />
                  <ActiveSectionCard view={view} sectionKey={active} bodies={bodies} flush={flush} onWritten={reload} />
                  <RequestCard
                    view={view}
                    letter={letter}
                    bodies={bodies}
                    flush={flush}
                    onSections={reload}
                    onDeadline={(day) => setMeta({ follow_up_on: day, follow_up_note: meta.follow_up_note || t('letterAi.deadlineNote') })}
                  />
                </>
              )}
              <SectionCard
                title={t('briefe.recipientCard')}
                actions={
                  !readOnly && (
                    <RecipientPicker
                      patientId={patient.id}
                      onPick={(r) => {
                        setLinesText(r.lines.join('\n'))
                        setMeta({
                          recipient_id: r.id,
                          recipient: { kind: r.kind, name: r.name, department: r.department, lines: r.lines },
                          salutation: r.salutation ?? DEFAULT_SALUTATION,
                        })
                      }}
                      trigger={
                        <Button variant="outline" size="sm" data-testid="letter-pick-recipient">
                          <BookUser /> {t('briefe.fromBook')}
                        </Button>
                      }
                    />
                  )
                }
                contentClassName="space-y-3"
              >
                <Field label={t('briefe.name')}>
                  <Input
                    value={meta.recipient.name}
                    disabled={readOnly}
                    onChange={(e) => setMeta({ recipient: { ...meta.recipient, name: e.target.value } })}
                    data-testid="letter-recipient-name"
                  />
                </Field>
                <Field label={t('briefe.department')}>
                  <Input
                    value={meta.recipient.department}
                    disabled={readOnly}
                    onChange={(e) => setMeta({ recipient: { ...meta.recipient, department: e.target.value } })}
                  />
                </Field>
                <Field label={t('briefe.addressLines')} hint={t('briefe.addressHint')}>
                  <Textarea
                    rows={3}
                    value={linesText}
                    disabled={readOnly}
                    onChange={(e) => {
                      setLinesText(e.target.value)
                      setMeta({ recipient: { ...meta.recipient, lines: linesFromText(e.target.value) } })
                    }}
                    data-testid="letter-recipient-lines"
                  />
                </Field>
                <Field label={t('briefe.kind')}>
                  <Select
                    value={meta.recipient.kind ?? 'sonstige'}
                    disabled={readOnly}
                    onValueChange={(v) => setMeta({ recipient: { ...meta.recipient, kind: v as RecipientKind } })}
                  >
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
                </Field>
                {meta.recipient_id &&
                  (isLinked ? (
                    <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
                      <Star className="size-3.5 text-primary" /> {t('briefe.remembered', { name: patient.first_name })}
                    </p>
                  ) : (
                    <Button variant="link" size="sm" className="h-auto p-0 text-xs" onClick={() => remember.mutate(meta.recipient_id!)}>
                      <Star className="size-3.5" /> {t('briefe.remember', { name: patient.first_name })}
                    </Button>
                  ))}
              </SectionCard>

              <SectionCard title={t('briefe.letterCard')} contentClassName="space-y-3">
                <Field label={t('briefe.refs')}>
                  <div className="space-y-1.5">
                    {meta.refs.map((r, i) => (
                      <div key={i} className="flex gap-1.5">
                        <Input
                          className="w-2/5"
                          placeholder={t('briefe.refLabel')}
                          value={r.label}
                          disabled={readOnly}
                          onChange={(e) => setMeta({ refs: meta.refs.map((x, j) => (j === i ? { ...x, label: e.target.value } : x)) })}
                        />
                        <Input
                          placeholder={t('briefe.refValue')}
                          value={r.value}
                          disabled={readOnly}
                          onChange={(e) => setMeta({ refs: meta.refs.map((x, j) => (j === i ? { ...x, value: e.target.value } : x)) })}
                        />
                        {!readOnly && (
                          <Button variant="ghost" size="icon" onClick={() => setMeta({ refs: meta.refs.filter((_, j) => j !== i) })}>
                            <X />
                          </Button>
                        )}
                      </div>
                    ))}
                    {!readOnly && (
                      <Button variant="ghost" size="sm" onClick={() => setMeta({ refs: [...meta.refs, { label: '', value: '' }] })}>
                        <Plus /> {t('briefe.addRef')}
                      </Button>
                    )}
                  </div>
                </Field>
                <div className="grid grid-cols-2 gap-2">
                  <Field label={t('briefe.place')}>
                    <Input value={meta.place ?? ''} disabled={readOnly} onChange={(e) => setMeta({ place: e.target.value || null })} />
                  </Field>
                  <Field label={t('briefe.date')} hint={t('briefe.dateHint')}>
                    <Input type="date" value={meta.letter_date ?? ''} disabled={readOnly} onChange={(e) => setMeta({ letter_date: e.target.value || null })} />
                  </Field>
                </div>
                <Field label={t('briefe.case')}>
                  <Select value={meta.episode_id ?? NO_CASE} onValueChange={(v) => setMeta({ episode_id: v === NO_CASE ? null : v })}>
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={NO_CASE}>{t('briefe.noCase')}</SelectItem>
                      {episodes.data?.map((e) => (
                        <SelectItem key={e.id} value={e.id}>
                          {e.method} · {t('episodes.since')} {formatDate(e.started_at)}
                          {e.ended_at ? ` – ${formatDate(e.ended_at)}` : ''}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
                <Field label={t('briefe.followUp')} hint={t('briefe.followUpHint')}>
                  <div className="space-y-1.5">
                    <Input type="date" value={meta.follow_up_on ?? ''} onChange={(e) => setMeta({ follow_up_on: e.target.value || null })} data-testid="letter-follow-up" />
                    {meta.follow_up_on && (
                      <Input
                        placeholder={t('briefe.followUpNote')}
                        value={meta.follow_up_note ?? ''}
                        onChange={(e) => setMeta({ follow_up_note: e.target.value || null })}
                      />
                    )}
                  </div>
                </Field>
              </SectionCard>

              {!readOnly && prose && (prose.hints.length > 0 || template.intro) && (
                <SectionCard title={t('briefe.hintsTitle')} contentClassName="space-y-2 text-[13px] text-muted-foreground">
                  {template.intro && <p>{template.intro}</p>}
                  <ul className="list-disc space-y-0.5 pl-5">
                    {prose.hints.map((h, i) => (
                      <li key={i}>{h.text}</li>
                    ))}
                  </ul>
                </SectionCard>
              )}

              <SectionCard title={t('briefe.finishCard')} contentClassName="space-y-3 text-sm">
                {!readOnly && (
                  <ul className="space-y-1" data-testid="letter-checks">
                    {checklist.data?.map((c, i) => (
                      <li key={i} className="flex items-start gap-2 text-[13px]">
                        {c.ok ? (
                          <CheckCircle2 className="mt-0.5 size-3.5 shrink-0 text-success" />
                        ) : (
                          <AlertTriangle className={cn('mt-0.5 size-3.5 shrink-0', c.severity === 'block' ? 'text-destructive' : 'text-warning')} />
                        )}
                        <span className={cn(c.ok && 'text-muted-foreground')}>
                          {c.message}
                          {!c.ok && c.key === 'letter_release' && (
                            <ReleaseDialog
                              patientId={patient.id}
                              recipientId={meta.recipient_id}
                              recipientName={meta.recipient.name}
                              kind={meta.recipient.kind}
                              reportId={reportId}
                              trigger={
                                <Button variant="link" size="sm" className="ml-1 h-auto p-0 text-[13px]" data-testid="letter-release">
                                  {t('release.addNow')}
                                </Button>
                              }
                            />
                          )}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
                {!readOnly ? (
                  <Button className="w-full" disabled={blocked} onClick={() => void flush().then(() => setSigning(true))} data-testid="letter-sign">
                    <FileSignature /> {t('briefe.sign')}
                  </Button>
                ) : (
                  <>
                    <div className="grid grid-cols-2 gap-2">
                      <Button variant="outline" onClick={() => exportAs.mutate('pdf')} disabled={exportAs.isPending} data-testid="letter-export-pdf">
                        <Download /> {t('briefe.exportPdf')}
                      </Button>
                      <Button variant="outline" onClick={() => exportAs.mutate('docx')} disabled={exportAs.isPending}>
                        <Download /> {t('briefe.exportDocx')}
                      </Button>
                    </div>
                    {letter.sent_at ? (
                      <p className="flex flex-wrap items-center gap-x-2 text-[13px] text-muted-foreground" data-testid="letter-sent">
                        <Send className="size-3.5" />
                        {t('briefe.sentOn', {
                          date: germanDay(letter.sent_at),
                          via: letter.sent_via ? t(`enums.sent_via.${letter.sent_via}`) : '',
                        })}
                        <Button variant="link" size="sm" className="h-auto p-0 text-xs" onClick={() => unsend.mutate()}>
                          {t('briefe.unmarkSent')}
                        </Button>
                      </p>
                    ) : (
                      <Button className="w-full" onClick={() => setSending(true)} data-testid="letter-mark-sent">
                        <Send /> {t('briefe.markSent')}
                      </Button>
                    )}
                    <StyleFromReportButton reportId={report.id} />
                  </>
                )}
                {!readOnly && pending === 0 && (
                  <Button variant="ghost" size="sm" className="w-full" onClick={() => exportAs.mutate('pdf')} disabled={exportAs.isPending}>
                    <Download /> {t('briefe.exportPdf')} ({t('enums.status.draft')})
                  </Button>
                )}
              </SectionCard>
            </aside>
          )}
        </div>
      </Page>
      {signing && (
        <SignDialog
          reportId={reportId}
          defaultName={therapist.data ?? ''}
          offerSave={!meta.recipient_id && !!meta.recipient.name.trim()}
          onClose={() => setSigning(false)}
          onSigned={afterChange}
        />
      )}
      {sending && <SendDialog reportId={reportId} onClose={() => setSending(false)} onSent={afterChange} />}
    </div>
  )
}

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1">
      <Label className="text-xs">{label}</Label>
      {children}
      {hint && <p className="text-[11px] text-muted-foreground">{hint}</p>}
    </div>
  )
}

function SignDialog({
  reportId,
  defaultName,
  offerSave,
  onClose,
  onSigned,
}: {
  reportId: string
  defaultName: string
  offerSave: boolean
  onClose: () => void
  onSigned: () => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState(defaultName)
  const [saveRecipient, setSaveRecipient] = useState(true)
  const sign = useMutation({
    mutationFn: () => api.signLetter(reportId, name, offerSave && saveRecipient),
    onSuccess: () => {
      toast.success(t('briefe.signed'))
      onClose()
      onSigned()
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('briefe.signTitle')}</DialogTitle>
          <DialogDescription>{t('briefe.signText')}</DialogDescription>
        </DialogHeader>
        <div className="space-y-1.5">
          <Label htmlFor="letter-signer">{t('briefe.signedBy')}</Label>
          <Input id="letter-signer" value={name} onChange={(e) => setName(e.target.value)} data-testid="letter-signer" />
        </div>
        {offerSave && (
          <label className="flex items-center gap-2 text-sm">
            <Checkbox checked={saveRecipient} onCheckedChange={(v) => setSaveRecipient(v === true)} data-testid="letter-save-recipient" />
            {t('briefe.saveRecipient')}
          </label>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button disabled={!name.trim() || sign.isPending} onClick={() => sign.mutate()} data-testid="letter-sign-confirm">
            <FileSignature /> {t('briefe.sign')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
