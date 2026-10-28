// Plan or change a session (slice 22): date via the calendar popover, time, length, modality, and
// for a new one an optional weekly series. Editing also cancels, re-plans or deletes it.
import { useState } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { format } from 'date-fns'
import { de } from 'date-fns/locale'
import { CalendarDays, Trash2, Undo2, XCircle } from 'lucide-react'
import type { Session } from '@/bindings/Session'
import { Button } from '@/components/ui/button'
import { Calendar } from '@/components/ui/calendar'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { api, errorMessage, q } from '@/lib/api'
import { DEFAULT_MINUTES, atMinute, minuteOfDay, parseTime, seriesDates } from '@/lib/calendar'

const LENGTHS = [25, 50, 75, 100]
const SERIES_COUNTS = [1, 2, 4, 6, 8, 10, 12, 15, 20, 25]

export type SessionDialogMode =
  | { kind: 'create'; at?: Date; patientId?: string }
  | { kind: 'edit'; session: Pick<Session, 'id' | 'scheduled_at' | 'started_at' | 'planned_minutes' | 'modality' | 'status'> }

/** Everything that shows sessions: lists, the session page, the calendar and "Heute". */
export function invalidateSessions(qc: QueryClient) {
  for (const key of ['sessions', 'session', 'calendar', 'patient']) void qc.invalidateQueries({ queryKey: [key] })
}

export function SessionDialog({
  mode,
  onOpenChange,
  openAfterCreate = false,
}: {
  mode: SessionDialogMode | null
  onOpenChange: (open: boolean) => void
  /** Go to the new session's page after creating a single session. */
  openAfterCreate?: boolean
}) {
  return (
    <Dialog open={!!mode} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md" data-testid="session-dialog">
        {mode && <SessionForm key={mode.kind === 'edit' ? mode.session.id : 'new'} mode={mode} close={() => onOpenChange(false)} openAfterCreate={openAfterCreate} />}
      </DialogContent>
    </Dialog>
  )
}

function SessionForm({ mode, close, openAfterCreate }: { mode: SessionDialogMode; close: () => void; openAfterCreate: boolean }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const editing = mode.kind === 'edit' ? mode.session : null
  const initial = editing
    ? new Date(editing.scheduled_at ?? editing.started_at ?? Date.now())
    : (mode.kind === 'create' && mode.at) || nextQuarterHour()

  const patients = useQuery({ ...q.patients(), enabled: !editing })
  const [patientId, setPatientId] = useState(mode.kind === 'create' ? (mode.patientId ?? '') : '')
  const [day, setDay] = useState<Date>(initial)
  const [time, setTime] = useState(format(initial, 'HH:mm'))
  const [minutes, setMinutes] = useState(String(editing?.planned_minutes ?? DEFAULT_MINUTES))
  const [modality, setModality] = useState(editing?.modality ?? 'praesenz')
  const [count, setCount] = useState('1')
  const [every, setEvery] = useState('1')
  const [dayOpen, setDayOpen] = useState(false)
  const [cancelling, setCancelling] = useState(false)
  const [reason, setReason] = useState('')
  const [confirmDelete, setConfirmDelete] = useState(false)

  const minute = parseTime(time)
  const start = () => atMinute(day, minute ?? 0)
  const done = () => {
    invalidateSessions(qc)
    close()
  }

  const save = useMutation({
    mutationFn: async () => {
      const at = start()
      if (editing) {
        await api.updateSession(editing.id, {
          scheduled_at: at.toISOString(),
          planned_minutes: Number(minutes),
          modality,
        })
        return null
      }
      // The running treatment case, like the palette's "Neue Sitzung".
      const episode = (await api.episodes(patientId)).find((e) => !e.ended_at) ?? null
      const dates = seriesDates(at, Number(count), Number(every))
      return api.createSessions(
        dates.map((d) => ({
          patient_id: patientId,
          episode_id: episode?.id ?? null,
          scheduled_at: d.toISOString(),
          modality,
          planned_minutes: Number(minutes),
        })),
      )
    },
    onSuccess: (made) => {
      done()
      if (made && made.length > 1) toast.success(t('calendar.seriesCreated', { count: made.length }))
      if (made && made.length === 1 && openAfterCreate) void navigate({ to: '/sessions/$sessionId', params: { sessionId: made[0].id } })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const cancel = useMutation({
    mutationFn: () => api.cancelSession(editing!.id, reason.trim() || null),
    onSuccess: done,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const reopen = useMutation({
    mutationFn: () => api.reopenSession(editing!.id),
    onSuccess: done,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const remove = useMutation({
    mutationFn: () => api.deleteSession(editing!.id),
    onSuccess: done,
    onError: (e) => toast.error(errorMessage(e)),
  })

  const cancelled = editing?.status === 'cancelled'
  const busy = save.isPending || cancel.isPending || reopen.isPending || remove.isPending

  if (cancelling) {
    return (
      <>
        <DialogHeader>
          <DialogTitle>{t('calendar.cancelTitle')}</DialogTitle>
          <DialogDescription>{t('calendar.cancelHint')}</DialogDescription>
        </DialogHeader>
        <div className="space-y-2">
          <Label htmlFor="cancel-reason">{t('calendar.reason')}</Label>
          <Input id="cancel-reason" value={reason} onChange={(e) => setReason(e.target.value)} placeholder={t('calendar.reasonPlaceholder')} autoFocus />
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => setCancelling(false)}>
            {t('common.back')}
          </Button>
          <Button variant="destructive" onClick={() => cancel.mutate()} disabled={busy} data-testid="cancel-confirm">
            {t('calendar.cancelSession')}
          </Button>
        </DialogFooter>
      </>
    )
  }

  return (
    <>
      <DialogHeader>
        <DialogTitle>{editing ? t('calendar.editTitle') : t('calendar.newTitle')}</DialogTitle>
        <DialogDescription>{editing ? t('calendar.editHint') : t('calendar.newHint')}</DialogDescription>
      </DialogHeader>
      <div className="grid gap-4">
        {!editing && (
          <div className="grid gap-2">
            <Label>{t('calendar.patient')}</Label>
            <Select value={patientId} onValueChange={setPatientId}>
              <SelectTrigger data-testid="session-patient">
                <SelectValue placeholder={t('calendar.pickPatient')} />
              </SelectTrigger>
              <SelectContent>
                {patients.data?.map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    {p.last_name}, {p.first_name}
                    {p.chiffre && <span className="ml-2 font-mono text-xs text-muted-foreground">{p.chiffre}</span>}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}
        <div className="grid grid-cols-[1fr_7rem] gap-3">
          <div className="grid gap-2">
            <Label>{t('calendar.date')}</Label>
            <Popover open={dayOpen} onOpenChange={setDayOpen}>
              <PopoverTrigger asChild>
                <Button variant="outline" className="justify-start font-normal" data-testid="session-day">
                  <CalendarDays className="text-muted-foreground" />
                  {format(day, 'EEEE, d. MMMM yyyy', { locale: de })}
                </Button>
              </PopoverTrigger>
              <PopoverContent className="w-auto p-0" align="start">
                <Calendar mode="single" selected={day} onSelect={(d) => {
                    // The picked date keeps the typed time.
                    if (d) setDay(d)
                    setDayOpen(false)
                  }} defaultMonth={day} weekStartsOn={1} />
              </PopoverContent>
            </Popover>
          </div>
          <div className="grid gap-2">
            <Label htmlFor="session-time">{t('calendar.time')}</Label>
            <Input
              id="session-time"
              inputMode="numeric"
              maxLength={5}
              placeholder="09:00"
              value={time}
              onChange={(e) => setTime(e.target.value)}
              onBlur={() => minute !== null && setTime(format(start(), 'HH:mm'))}
              aria-invalid={minute === null}
              className="tabular-nums"
              data-testid="session-time"
            />
          </div>
        </div>
        <div className="grid grid-cols-2 gap-3">
          <div className="grid gap-2">
            <Label>{t('calendar.length')}</Label>
            <Select value={minutes} onValueChange={setMinutes}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {[...new Set([...LENGTHS, Number(minutes)])].sort((a, b) => a - b).map((m) => (
                  <SelectItem key={m} value={String(m)}>
                    {t('calendar.minutes', { count: m })}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-2">
            <Label>{t('calendar.modality')}</Label>
            <Select value={modality} onValueChange={setModality}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="praesenz">{t('calendar.praesenz')}</SelectItem>
                <SelectItem value="video">{t('calendar.video')}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>
        {!editing && (
          <div className="grid grid-cols-2 gap-3">
            <div className="grid gap-2">
              <Label>{t('calendar.series')}</Label>
              <Select value={count} onValueChange={setCount}>
                <SelectTrigger data-testid="session-count">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {SERIES_COUNTS.map((n) => (
                    <SelectItem key={n} value={String(n)}>
                      {n === 1 ? t('calendar.single') : t('calendar.appointments', { count: n })}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-2">
              <Label>{t('calendar.every')}</Label>
              <Select value={every} onValueChange={setEvery} disabled={count === '1'}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="1">{t('calendar.weekly')}</SelectItem>
                  <SelectItem value="2">{t('calendar.biweekly')}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
        )}
        {!editing && count !== '1' && (
          <p className="text-xs text-muted-foreground">
            {t('calendar.seriesUntil', {
              date: format(seriesDates(start(), Number(count), Number(every)).at(-1)!, 'EEEE, d. MMMM yyyy', { locale: de }),
            })}
          </p>
        )}
      </div>
      <DialogFooter className="sm:justify-between">
        <div className="flex gap-1">
          {editing && !cancelled && (
            <Button variant="ghost" className="text-destructive" onClick={() => setCancelling(true)} disabled={busy} data-testid="session-cancel">
              <XCircle /> {t('calendar.cancelSession')}
            </Button>
          )}
          {cancelled && (
            <Button variant="ghost" onClick={() => reopen.mutate()} disabled={busy} data-testid="session-reopen">
              <Undo2 /> {t('calendar.reopen')}
            </Button>
          )}
          {editing && (
            <Button
              variant="ghost"
              size={confirmDelete ? 'default' : 'icon'}
              className={confirmDelete ? 'text-destructive' : undefined}
              onClick={() => (confirmDelete ? remove.mutate() : setConfirmDelete(true))}
              disabled={busy}
              title={t('calendar.deleteHint')}
              aria-label={t('calendar.delete')}
              data-testid="session-delete"
            >
              <Trash2 /> {confirmDelete && t('calendar.deleteConfirm')}
            </Button>
          )}
        </div>
        <Button onClick={() => save.mutate()} disabled={busy || (!editing && !patientId) || minute === null} data-testid="session-save">
          {editing ? t('common.save') : count === '1' ? t('calendar.create') : t('calendar.createSeries', { count: Number(count) })}
        </Button>
      </DialogFooter>
    </>
  )
}

function nextQuarterHour(): Date {
  const now = new Date()
  return atMinute(now, Math.ceil((minuteOfDay(now) + 1) / 15) * 15)
}
