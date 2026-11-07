import { useState } from 'react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { BookUser, Mail, Plus, Search, TextQuote } from 'lucide-react'
import { Page, PageHeader } from '@/components/app-shell'
import { EmptyState, FilterChip, formatDate } from '@/components/bits'
import { LetterStatePill } from '@/components/briefe/letter-state'
import { NewLetterDialog } from '@/components/briefe/new-letter-dialog'
import { DeleteReport } from '@/components/delete-button'
import { HelpLink } from '@/components/help-link'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { q } from '@/lib/api'
import { HELP } from '@/lib/help'
import { germanDay, matchesLetter, type LetterFilter } from '@/lib/letters'

type BriefeSearch = { neu?: boolean; patient?: string }

export const Route = createFileRoute('/briefe/')({
  component: Briefe,
  // `neu`: open "Neuer Brief" (Strg+K); `patient`: with that patient chosen.
  validateSearch: (s: Record<string, unknown>): BriefeSearch => ({
    ...(s.neu === true || s.neu === 'true' ? { neu: true } : {}),
    ...(typeof s.patient === 'string' ? { patient: s.patient } : {}),
  }),
})

const FILTERS: LetterFilter[] = ['all', 'draft', 'signed', 'sent']

function Briefe() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const search = Route.useSearch()
  const letters = useQuery(q.letters())
  const [filter, setFilter] = useState<LetterFilter>('all')
  const [query, setQuery] = useState('')
  const all = letters.data ?? []
  const shown = all.filter((l) => matchesLetter(l, filter, query))
  const closeNew = () => void navigate({ to: '/briefe', search: {}, replace: true })

  return (
    <>
      <PageHeader
        title={
          <span className="flex items-center gap-1">
            {t('briefe.title')} <HelpLink slug={HELP.letters} />
          </span>
        }
        subtitle={t('briefe.subtitle')}
        actions={
          <>
            <Button asChild variant="outline">
              <Link to="/briefe/empfaenger">
                <BookUser /> {t('briefe.addressBook')}
              </Link>
            </Button>
            <Button asChild variant="outline">
              <Link to="/briefe/textbausteine" data-testid="textblocks-link">
                <TextQuote /> {t('briefe.textblocks')}
              </Link>
            </Button>
            <NewLetterDialog
              trigger={
                <Button data-testid="new-letter">
                  <Plus /> {t('briefe.new')}
                </Button>
              }
            />
          </>
        }
      />
      {search.neu && <NewLetterDialog key={search.patient ?? 'any'} patientId={search.patient} open onOpenChange={(o) => !o && closeNew()} />}
      <Page>
        {all.length === 0 && letters.isSuccess ? (
          <EmptyState icon={Mail} title={t('briefe.emptyTitle')} description={t('briefe.emptyBody')} />
        ) : (
          <>
            <div className="flex flex-wrap items-center gap-2">
              {FILTERS.map((f) => (
                <FilterChip
                  key={f}
                  active={filter === f}
                  onClick={() => setFilter(f)}
                  count={f === 'all' ? undefined : all.filter((l) => l.state === f).length}
                >
                  {t(`briefe.filters.${f}`)}
                </FilterChip>
              ))}
              <div className="relative ml-auto w-72">
                <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t('briefe.search')} className="pl-9" />
              </div>
            </div>
            {shown.length === 0 ? (
              <p className="py-6 text-sm text-muted-foreground">{t('common.noMatch')}</p>
            ) : (
              <div className="rounded-lg border bg-card">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{t('briefe.columns.patient')}</TableHead>
                      <TableHead>{t('briefe.columns.type')}</TableHead>
                      <TableHead>{t('briefe.columns.recipient')}</TableHead>
                      <TableHead>{t('briefe.columns.date')}</TableHead>
                      <TableHead>{t('briefe.columns.status')}</TableHead>
                      <TableHead>{t('briefe.columns.followUp')}</TableHead>
                      <TableHead className="w-10" />
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {shown.map((l) => (
                      <TableRow
                        key={l.report_id}
                        className="cursor-pointer"
                        onClick={() => void navigate({ to: '/briefe/$briefId', params: { briefId: l.report_id } })}
                        data-testid="letter-row"
                      >
                        <TableCell className="font-medium">{l.patient_name}</TableCell>
                        <TableCell>
                          <span className="flex flex-col leading-tight">
                            <span>{l.template_title}</span>
                            <span className="max-w-72 truncate text-xs text-muted-foreground">{l.subject}</span>
                          </span>
                        </TableCell>
                        <TableCell>{l.recipient_name || <span className="text-muted-foreground">{t('briefe.noRecipient')}</span>}</TableCell>
                        <TableCell className="tabular-nums">{formatDate(l.sent_at ?? l.signed_at ?? l.updated_at)}</TableCell>
                        <TableCell>
                          <LetterStatePill state={l.state} />
                        </TableCell>
                        <TableCell className="tabular-nums">{l.follow_up_on ? germanDay(l.follow_up_on) : ''}</TableCell>
                        <TableCell onClick={(e) => e.stopPropagation()}>
                          <DeleteReport report={{ id: l.report_id, status: l.status }} />
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
            )}
          </>
        )}
      </Page>
    </>
  )
}
