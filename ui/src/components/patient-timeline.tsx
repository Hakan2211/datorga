import { useState, type ReactNode } from 'react'
import { Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { CalendarX2, FileText, History, Pencil, ShieldCheck } from 'lucide-react'
import type { TimelineGroup } from '@/bindings/TimelineGroup'
import type { TimelineItem } from '@/bindings/TimelineItem'
import type { Contingent } from '@/bindings/Contingent'
import { EmptyState, StatusBadge, StatusPill, formatDate } from '@/components/bits'
import { SummaryPoints, SummaryStatus } from '@/components/session-summary'
import { Button } from '@/components/ui/button'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { q } from '@/lib/api'
import { cn } from '@/lib/utils'

type Filter = 'all' | 'sessions'

/** "11:00": the date is already in the timeline's left column. */
function timeOf(iso: string | null) {
  return iso ? new Date(iso).toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' }) : ''
}

/** The patient's course: newest first, one block per Behandlungsfall. */
export function PatientTimeline({ patientId }: { patientId: string }) {
  const { t } = useTranslation()
  const timeline = useQuery(q.timeline(patientId))
  const [filter, setFilter] = useState<Filter>('all')
  if (!timeline.data) return null
  const groups = timeline.data
  const empty = groups.every((g) => g.items.length === 0)

  return (
    <div className="space-y-6" data-testid="timeline">
      <div className="flex items-center justify-between gap-3">
        <p className="text-sm text-muted-foreground">{t('timeline.intro')}</p>
        <ToggleGroup
          type="single"
          variant="outline"
          size="sm"
          value={filter}
          onValueChange={(v) => v && setFilter(v as Filter)}
          aria-label={t('timeline.filter')}
        >
          <ToggleGroupItem value="all" className="px-3" data-testid="timeline-all">
            {t('timeline.all')}
          </ToggleGroupItem>
          <ToggleGroupItem value="sessions" className="px-3" data-testid="timeline-sessions">
            {t('timeline.sessionsOnly')}
          </ToggleGroupItem>
        </ToggleGroup>
      </div>
      {empty ? (
        <EmptyState icon={History} title={t('timeline.emptyTitle')} description={t('timeline.emptyBody')} />
      ) : (
        groups.map((g) => <Group key={g.episode?.id ?? 'none'} group={g} patientId={patientId} filter={filter} />)
      )}
    </div>
  )
}

function Group({ group, patientId, filter }: { group: TimelineGroup; patientId: string; filter: Filter }) {
  const { t } = useTranslation()
  const items = group.items.filter((i) => filter === 'all' || i.type === 'session')
  const e = group.episode
  return (
    <section className="space-y-3">
      <h3 className="flex flex-wrap items-baseline gap-x-2 text-sm font-semibold">
        {e ? (
          <>
            {t('timeline.case', { method: e.method })}
            <span className="font-normal text-muted-foreground">
              {t(`enums.age_group.${e.age_group}`)} · {t('timeline.sessionCount', { count: group.sessions })} · {t('episodes.since')}{' '}
              {formatDate(e.started_at)}
              {e.ended_at && ` – ${formatDate(e.ended_at)}`}
            </span>
          </>
        ) : (
          t('timeline.noCase')
        )}
      </h3>
      {items.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t('timeline.groupEmpty')}</p>
      ) : (
        <ol className="relative ml-[7.5rem] border-l-2 border-primary/25">
          {items.map((item, i) => (
            <Node key={i} item={item} patientId={patientId} contingent={group.contingent} />
          ))}
        </ol>
      )}
    </section>
  )
}

function Node({ item, patientId, contingent }: { item: TimelineItem; patientId: string; contingent: Contingent | null }) {
  const cancelled = item.type === 'session' && item.session.status === 'cancelled'
  return (
    <li className="relative pb-5 pl-6 last:pb-0" data-testid={`timeline-${item.type}`}>
      <span className="absolute top-0.5 -left-[7.5rem] w-[6.5rem] text-right text-xs text-muted-foreground tabular-nums">
        {formatDate(item.at)}
      </span>
      <span
        className={cn(
          'absolute top-1 -left-[7px] size-3 rounded-full border-2 border-background',
          item.type === 'session' ? (cancelled ? 'bg-muted-foreground/40' : 'bg-primary') : 'bg-sand-foreground/50',
        )}
      />
      {item.type === 'session' && <SessionNode item={item} patientId={patientId} contingent={contingent} />}
      {item.type === 'application' && <ApplicationNode item={item} />}
      {item.type === 'consent' && <ConsentNode item={item} />}
    </li>
  )
}

function Line({ icon, children }: { icon?: ReactNode; children: ReactNode }) {
  return <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">{icon}{children}</div>
}

function SessionNode({
  item,
  patientId,
  contingent,
}: {
  item: Extract<TimelineItem, { type: 'session' }>
  patientId: string
  contingent: Contingent | null
}) {
  const { t } = useTranslation()
  const [editing, setEditing] = useState(false)
  const { session, number, summary, probatorik } = item
  // "Sitzung 9/12 · KZT 1" once the therapy started; the sessions before are the Probatorik.
  const counted = contingent && contingent.stage !== 'probatorik'
  const label = probatorik
    ? t('timeline.probatorik', { n: number })
    : counted
      ? t('timeline.sessionOf', { n: number, limit: contingent.limit, stage: t(`enums.stage.${contingent.stage}`) })
      : t('timeline.session', { n: number })
  if (session.status === 'cancelled') {
    return (
      <Line icon={<CalendarX2 className="size-4 text-muted-foreground" />}>
        <span className="text-muted-foreground">
          {session.cancelled_reason ? t('timeline.cancelledReason', { reason: session.cancelled_reason }) : t('timeline.cancelled')}
        </span>
      </Line>
    )
  }
  return (
    <div className="space-y-2">
      <Line>
        <Link to="/sessions/$sessionId" params={{ sessionId: session.id }} className="font-medium hover:underline">
          {timeOf(session.scheduled_at ?? session.started_at)} · {label}
        </Link>
        {session.note_status ? (
          <StatusPill tone={session.note_status === 'signed' || session.note_status === 'exported' ? 'success' : 'warning'}>
            {t(`timeline.note.${session.note_status}`)}
          </StatusPill>
        ) : (
          <StatusPill>{t('timeline.noNote')}</StatusPill>
        )}
        {summary && <SummaryStatus summary={summary} />}
        {summary && !editing && (
          <Button size="sm" variant="link" className="h-auto p-0 text-xs" onClick={() => setEditing(true)}>
            <Pencil className="size-3" /> {t('timeline.editSummary')}
          </Button>
        )}
      </Line>
      {summary ? (
        <SummaryPoints
          key={`${summary.updated_at}-${editing}`}
          summary={summary}
          patientId={patientId}
          editing={editing}
          onDone={() => setEditing(false)}
          compact
        />
      ) : (
        <p className="text-[13px] text-muted-foreground">{t('timeline.noSummary')}</p>
      )}
    </div>
  )
}

function ApplicationNode({ item }: { item: Extract<TimelineItem, { type: 'application' }> }) {
  const { t } = useTranslation()
  const a = item.application
  return (
    <Line icon={<FileText className="size-4 text-muted-foreground" />}>
      <span className="font-medium">
        {t(`enums.kind.${a.kind}`)}
        {a.kind === 'fortfuehrung' ? ` Nr. ${a.seq}` : ''}
      </span>
      {item.report_status && <StatusBadge status={item.report_status} />}
      {item.exported_at ? (
        <span className="text-xs text-muted-foreground">{t('timeline.exportedAt', { date: formatDate(item.exported_at) })}</span>
      ) : item.signed_at ? (
        <span className="text-xs text-muted-foreground">{t('timeline.signedAt', { date: formatDate(item.signed_at) })}</span>
      ) : null}
      {a.report_id && (
        <Link to="/reports/$reportId" params={{ reportId: a.report_id }} className="text-xs underline-offset-2 hover:underline">
          {t('timeline.openReport')}
        </Link>
      )}
    </Line>
  )
}

function ConsentNode({ item }: { item: Extract<TimelineItem, { type: 'consent' }> }) {
  const { t } = useTranslation()
  return (
    <Line icon={<ShieldCheck className={cn('size-4', item.granted ? 'text-success' : 'text-muted-foreground')} />}>
      <span>
        {t(`consent.kinds.${item.kind}.title`)} {t(item.granted ? 'timeline.granted' : 'timeline.revoked')}
      </span>
    </Line>
  )
}
