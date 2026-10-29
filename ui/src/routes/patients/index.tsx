import { useState, type ReactNode } from 'react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { LayoutGrid, Plus, Rows3, Search, Users } from 'lucide-react'
import type { PatientCard } from '@/bindings/PatientCard'
import { Page, PageHeader } from '@/components/app-shell'
import { EmptyState, formatDate } from '@/components/bits'
import { PatientAvatar } from '@/components/patient-avatar'
import { PatientFormDialog } from '@/components/patient-form'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { q } from '@/lib/api'
import { ageOn, chaptersIn, matches, noFilter, sortCards, type CardFilter, type CardSort } from '@/lib/patients'
import { cn } from '@/lib/utils'

export const Route = createFileRoute('/patients/')({ component: Patients })

type View = 'cards' | 'table'
const VIEW_KEY = 'thera.patientsView'
const SORT_KEY = 'thera.patientsSort'

function stored<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  const v = localStorage.getItem(key)
  return allowed.includes(v as T) ? (v as T) : fallback
}

/** "Do 31.10., 10:00" */
function shortWhen(iso: string): string {
  const d = new Date(iso)
  const day = d.toLocaleDateString('de-DE', { weekday: 'short' }).replace('.', '')
  const date = d.toLocaleDateString('de-DE', { day: '2-digit', month: '2-digit' })
  const time = d.toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' })
  return `${day} ${date}, ${time}`
}

function Patients() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const [query, setQuery] = useState('')
  const [view, setViewState] = useState<View>(() => stored(VIEW_KEY, ['cards', 'table'], 'cards'))
  const [sort, setSortState] = useState<CardSort>(() => stored(SORT_KEY, ['name', 'next', 'last'], 'next'))
  const [filter, setFilter] = useState<CardFilter>(noFilter)
  // One "now" per visit, to the minute: keeps the query key stable across renders.
  const [now] = useState(() => {
    const d = new Date()
    d.setSeconds(0, 0)
    return d
  })
  const cards = useQuery(q.patientCards(query, now.toISOString()))

  const setView = (v: View) => {
    localStorage.setItem(VIEW_KEY, v)
    setViewState(v)
  }
  const setSort = (s: CardSort) => {
    localStorage.setItem(SORT_KEY, s)
    setSortState(s)
  }

  const all = cards.data ?? []
  const shown = sortCards(
    all.filter((c) => matches(c, filter, now)),
    sort,
  )
  const filtered = filter !== noFilter
  const methods = (['VT', 'TP', 'AP'] as const).filter((m) => all.some((c) => c.method === m))
  const chapters = chaptersIn(all)
  const toggle = <K extends keyof CardFilter>(key: K, value: CardFilter[K]) =>
    setFilter((f) => ({ ...f, [key]: f[key] === value ? null : value }))

  const newButton = (
    <PatientFormDialog
      trigger={
        <Button data-testid="patient-new">
          <Plus /> {t('patients.new')}
        </Button>
      }
      onSaved={(p) => navigate({ to: '/patients/$patientId', params: { patientId: p.id } })}
    />
  )

  return (
    <>
      <PageHeader
        title={t('patients.title')}
        actions={
          <>
            <ToggleGroup
              type="single"
              variant="outline"
              value={view}
              onValueChange={(v) => v && setView(v as View)}
              aria-label={t('patients.cards')}
            >
              <ToggleGroupItem value="cards" className="px-3" data-testid="patients-view-cards">
                <LayoutGrid /> {t('patients.cards')}
              </ToggleGroupItem>
              <ToggleGroupItem value="table" className="px-3" data-testid="patients-view-table">
                <Rows3 /> {t('patients.table')}
              </ToggleGroupItem>
            </ToggleGroup>
            <div className="relative w-64">
              <Search className="absolute top-2.5 left-3 size-4 text-muted-foreground" />
              <Input className="pl-9" placeholder={t('patients.searchPlaceholder')} value={query} onChange={(e) => setQuery(e.target.value)} />
            </div>
            {newButton}
          </>
        }
      />
      <Page>
        {cards.isSuccess && all.length === 0 && !query ? (
          <EmptyState icon={Users} title={t('patients.emptyTitle')} description={t('patients.emptyBody')} action={newButton} />
        ) : (
          <>
            <div className="flex flex-wrap items-center gap-2" role="toolbar" aria-label={t('patients.filterLabel')}>
              <Chip active={!filtered} onClick={() => setFilter(noFilter)}>
                {t('patients.filters.all')}
              </Chip>
              <Chip active={filter.ageGroup === 'adult'} onClick={() => toggle('ageGroup', 'adult')}>
                {t('patients.filters.adult')}
              </Chip>
              <Chip active={filter.ageGroup === 'kjp'} onClick={() => toggle('ageGroup', 'kjp')}>
                {t('patients.filters.kjp')}
              </Chip>
              {(['w', 'm', 'd'] as const).map((g) => (
                <Chip key={g} active={filter.gender === g} onClick={() => toggle('gender', g)}>
                  {t(`enums.gender.${g}`)}
                </Chip>
              ))}
              {chapters.map((c) => (
                <Chip key={c} active={filter.chapter === c} onClick={() => toggle('chapter', c)}>
                  {t(`icd.${c}`)}
                </Chip>
              ))}
              {methods.map((m) => (
                <Chip key={m} active={filter.method === m} onClick={() => toggle('method', m)}>
                  {m}
                </Chip>
              ))}
              <div className="ml-auto flex items-center gap-2 text-sm text-muted-foreground">
                {t('patients.sort')}
                <Select value={sort} onValueChange={(v) => setSort(v as CardSort)}>
                  <SelectTrigger className="w-44 bg-card" aria-label={t('patients.sort')}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {(['next', 'last', 'name'] as const).map((s) => (
                      <SelectItem key={s} value={s}>
                        {t(`patients.sortBy.${s}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>

            {cards.isSuccess && shown.length === 0 ? (
              <EmptyState
                title={t('patients.noMatch')}
                action={
                  filtered ? (
                    <Button variant="outline" size="sm" onClick={() => setFilter(noFilter)}>
                      {t('patients.clearFilters')}
                    </Button>
                  ) : undefined
                }
              />
            ) : view === 'cards' ? (
              <div className="grid grid-cols-[repeat(auto-fill,minmax(15.5rem,1fr))] gap-4" data-testid="patient-cards">
                {shown.map((c) => (
                  <PatientTile key={c.patient.id} card={c} now={now} />
                ))}
              </div>
            ) : (
              <PatientTable cards={shown} now={now} />
            )}
          </>
        )}
      </Page>
    </>
  )
}

function Chip({ active, onClick, children }: { active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        'h-8 rounded-full border px-3.5 text-sm transition-colors focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none',
        active ? 'border-primary bg-primary text-primary-foreground' : 'bg-card text-foreground hover:bg-accent',
      )}
    >
      {children}
    </button>
  )
}

function ageGender(card: PatientCard, now: Date, t: (k: string, o?: Record<string, unknown>) => string): string {
  const age = ageOn(card.patient.birth_date, now)
  return [age !== null ? t('patients.age', { count: age }) : null, card.patient.gender ? t(`enums.gender.${card.patient.gender}`) : null]
    .filter(Boolean)
    .join(' · ')
}

function PatientTile({ card, now }: { card: PatientCard; now: Date }) {
  const { t } = useTranslation()
  const p = card.patient
  const [main, ...more] = card.diagnoses
  return (
    <Link
      to="/patients/$patientId"
      params={{ patientId: p.id }}
      className="group rounded-xl focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
      data-testid="patient-card"
    >
      <Card className="h-full gap-0 py-0 transition-shadow group-hover:shadow-md">
        <CardContent className="flex gap-4 px-5 pt-5 pb-4">
          <PatientAvatar patientId={p.id} hasPhoto={card.has_photo} firstName={p.first_name} lastName={p.last_name} className="size-20 text-2xl" />
          <div className="min-w-0 space-y-1">
            <div className="truncate font-serif text-lg leading-tight">
              {p.first_name} {p.last_name}
            </div>
            {p.chiffre && <div className="font-mono text-xs tracking-wide text-primary">{p.chiffre}</div>}
            <div className="text-sm text-muted-foreground">{ageGender(card, now, t) || ' '}</div>
            <div className="flex flex-wrap gap-1.5 pt-1">
              {card.method && (
                <Badge variant="outline" className="border-primary/40 text-primary">
                  {card.method}
                </Badge>
              )}
              {main && (
                <Badge variant="secondary" className="font-mono" title={main.label}>
                  {main.code}
                  {main.certainty !== 'G' ? ` ${main.certainty}` : ''}
                </Badge>
              )}
              {more.length > 0 && <Badge variant="secondary">+{more.length}</Badge>}
            </div>
          </div>
        </CardContent>
        <div className="mt-auto flex items-center justify-between gap-2 border-t px-5 py-3 text-sm">
          <span className="truncate text-muted-foreground">
            {card.next_at ? (
              <>
                {t('patients.nextSession')}: <span className="text-foreground">{shortWhen(card.next_at)}</span>
              </>
            ) : (
              t('patients.noSession')
            )}
          </span>
          {card.open_items > 0 && <OpenDot count={card.open_items} />}
        </div>
      </Card>
    </Link>
  )
}

function OpenDot({ count }: { count: number }) {
  const { t } = useTranslation()
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="flex shrink-0 items-center gap-1.5 text-xs whitespace-nowrap">
          <span className="size-2 rounded-full bg-amber-500" />
          {t('patients.open', { count })}
        </span>
      </TooltipTrigger>
      <TooltipContent>{t('patients.openHint')}</TooltipContent>
    </Tooltip>
  )
}

function PatientTable({ cards, now }: { cards: PatientCard[]; now: Date }) {
  const { t } = useTranslation()
  return (
    <Card>
      <CardContent className="pt-2">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>
                {t('patients.lastName')}, {t('patients.firstName')}
              </TableHead>
              <TableHead>{t('patients.birthDate')}</TableHead>
              <TableHead>{t('patients.gender')}</TableHead>
              <TableHead>{t('patients.chiffre')}</TableHead>
              <TableHead>{t('episodes.method')}</TableHead>
              <TableHead>{t('episodes.diagnoses')}</TableHead>
              <TableHead>{t('patients.nextSession')}</TableHead>
              <TableHead className="text-right">{t('patients.openColumn')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {cards.map((c) => {
              const p = c.patient
              const age = ageOn(p.birth_date, now)
              return (
                <TableRow key={p.id}>
                  <TableCell className="font-medium">
                    <Link to="/patients/$patientId" params={{ patientId: p.id }} className="flex items-center gap-2.5 hover:underline">
                      <PatientAvatar patientId={p.id} hasPhoto={c.has_photo} firstName={p.first_name} lastName={p.last_name} className="size-7 text-xs" />
                      {p.last_name}, {p.first_name}
                    </Link>
                  </TableCell>
                  <TableCell>
                    {formatDate(p.birth_date)}
                    {age !== null && <span className="ml-1.5 text-muted-foreground">({age})</span>}
                  </TableCell>
                  <TableCell>{p.gender ? t(`enums.gender.${p.gender}`) : '—'}</TableCell>
                  <TableCell className="font-mono text-xs">{p.chiffre ?? '—'}</TableCell>
                  <TableCell>{c.method ?? '—'}</TableCell>
                  <TableCell className="font-mono text-xs">{c.diagnoses.map((d) => d.code).join(', ') || '—'}</TableCell>
                  <TableCell>{c.next_at ? shortWhen(c.next_at) : '—'}</TableCell>
                  <TableCell className="text-right tabular-nums">{c.open_items || '—'}</TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  )
}
