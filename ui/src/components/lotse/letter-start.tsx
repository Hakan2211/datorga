// Lotse step 1 for a letter-type Bericht (slice 46): instead of the Antragsart, who it goes to,
// the Bezugszeichen under the Betreff, the Behandlungsfall and the Schweigepflichtentbindung.
// Address details, Betreff and Anrede are edited on the letter page.
import { useState, type ReactNode } from 'react'
import { Link } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { AlertTriangle, BookUser, CheckCircle2, Mail, ShieldCheck } from 'lucide-react'
import type { LetterMeta } from '@/bindings/LetterMeta'
import { formatDate } from '@/components/bits'
import { RecipientPicker } from '@/components/briefe/recipient-picker'
import { ReleaseDialog } from '@/components/briefe/release'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { api, errorMessage, q, type ReportView } from '@/lib/api'
import { DEFAULT_SALUTATION, metaOf } from '@/lib/letters'

export function LetterStartCard({ view, readOnly, children }: { view: ReportView; readOnly: boolean; children?: ReactNode }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const { report, template, patient, episode } = view
  const letter = useQuery(q.letter(report.id))
  const checklist = useQuery(q.checklist(report.id))
  const save = useMutation({
    mutationFn: (patch: Partial<LetterMeta>) => api.saveLetterMeta(report.id, { ...metaOf(letter.data!), ...patch }),
    onSuccess: (l) => {
      qc.setQueryData(q.letter(report.id).queryKey, l)
      void qc.invalidateQueries({ queryKey: ['checklist', report.id] })
      void qc.invalidateQueries({ queryKey: ['letters'] })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const l = letter.data
  const release = checklist.data?.find((c) => c.key === 'letter_release')
  const address = l ? [l.recipient.department, ...l.recipient.lines].filter((x) => x.trim()) : []
  const required = new Set(template.refs.filter((r) => r.required).map((r) => r.label))

  return (
    <Card className="self-start" data-testid="lotse-letter-start">
      <CardHeader>
        <CardTitle className="text-base">{template.title}</CardTitle>
        {template.description && <CardDescription>{template.description}</CardDescription>}
      </CardHeader>
      <CardContent className="space-y-4 text-sm">
        <section className="space-y-1.5">
          <h3 className="text-xs font-medium text-muted-foreground">{t('lotse.letter.recipient')}</h3>
          {l && l.recipient.name.trim() ? (
            <div className="rounded-md border px-3 py-2" data-testid="lotse-letter-recipient">
              <div className="font-medium">{l.recipient.name}</div>
              {address.map((line, i) => (
                <div key={i} className="text-muted-foreground">
                  {line}
                </div>
              ))}
            </div>
          ) : (
            <p className="text-[13px] text-muted-foreground">{t('lotse.letter.noRecipient')}</p>
          )}
          {!readOnly && l && (
            <RecipientPicker
              patientId={patient.id}
              onPick={(r) =>
                save.mutate({
                  recipient_id: r.id,
                  recipient: { kind: r.kind, name: r.name, department: r.department, lines: r.lines },
                  salutation: r.salutation ?? DEFAULT_SALUTATION,
                })
              }
              trigger={
                <Button variant="outline" size="sm" data-testid="lotse-letter-pick">
                  <BookUser /> {t('briefe.fromBook')}
                </Button>
              }
            />
          )}
        </section>

        {l && l.refs.length > 0 && (
          <section className="space-y-1.5">
            <h3 className="text-xs font-medium text-muted-foreground">{t('lotse.letter.refs')}</h3>
            {l.refs.map((r, i) => (
              <RefField
                key={`${r.label}:${i}`}
                label={r.label}
                value={r.value}
                required={required.has(r.label)}
                readOnly={readOnly}
                onSave={(value) => save.mutate({ refs: l.refs.map((x, j) => (j === i ? { ...x, value } : x)) })}
              />
            ))}
          </section>
        )}

        <section className="space-y-1.5">
          <h3 className="text-xs font-medium text-muted-foreground">{t('lotse.letter.case')}</h3>
          {episode ? (
            <p>
              {t(`enums.method.${episode.method}`)} · {t('episodes.since')} {formatDate(episode.started_at)}
            </p>
          ) : (
            <p className="flex items-start gap-2 text-[13px] text-warning-foreground">
              <AlertTriangle className="mt-0.5 size-4 shrink-0" /> {t('lotse.letter.noCase')}
            </p>
          )}
        </section>

        {release && (
          <section className="space-y-1.5">
            <h3 className="text-xs font-medium text-muted-foreground">{t('lotse.letter.release')}</h3>
            {release.ok ? (
              <p className="flex items-start gap-2 text-[13px]" data-testid="lotse-letter-release">
                <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-success" /> {release.message}
              </p>
            ) : (
              <div className="flex flex-wrap items-center gap-2 text-[13px]" data-testid="lotse-letter-release">
                <span className="flex items-start gap-2">
                  <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning-foreground" /> {release.message}
                </span>
                {!readOnly && l && (
                  <ReleaseDialog
                    patientId={patient.id}
                    recipientId={l.recipient_id}
                    recipientName={l.recipient.name}
                    kind={l.recipient.kind}
                    reportId={report.id}
                    trigger={
                      <Button size="sm" variant="outline">
                        <ShieldCheck /> {t('release.addNow')}
                      </Button>
                    }
                  />
                )}
              </div>
            )}
          </section>
        )}

        <Button asChild variant="link" size="sm" className="h-auto p-0 text-xs">
          <Link to="/briefe/$briefId" params={{ briefId: report.id }} data-testid="lotse-letter-open">
            <Mail className="size-3.5" /> {t('lotse.letter.openLetter')}
          </Link>
        </Button>
        {children}
      </CardContent>
    </Card>
  )
}

/** One reference line; saved when the field loses focus. */
function RefField({
  label,
  value,
  required,
  readOnly,
  onSave,
}: {
  label: string
  value: string
  required: boolean
  readOnly: boolean
  onSave: (value: string) => void
}) {
  const { t } = useTranslation()
  const [text, setText] = useState(value)
  return (
    <label className="grid grid-cols-[minmax(0,2fr)_minmax(0,3fr)] items-center gap-2">
      <span className="truncate text-[13px]">
        {label}
        {required && <span className="ml-1 text-xs text-muted-foreground">({t('lotse.letter.required')})</span>}
      </span>
      <Input
        value={text}
        disabled={readOnly}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => text !== value && onSave(text)}
        aria-invalid={required && !text.trim()}
        data-testid={`lotse-letter-ref-${label}`}
      />
    </label>
  )
}
