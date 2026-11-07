// Plan or change a session (slice 22): date via the calendar popover, start, length or end, modality, and
// for a new one an optional weekly series. Editing also cancels, re-plans or deletes it.
import { useEffect, useState } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { addMinutes, format } from 'date-fns'
import { de } from 'date-fns/locale'
import { Building2, CalendarDays, Undo2, Video, XCircle } from 'lucide-react'
import type { Session } from '@/bindings/Session'
import { DeleteSession } from '@/components/delete-button'
import { Button } from '@/components/ui/button'
import { Calendar } from '@/components/ui/calendar'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { DurationPicker, TimePicker } from '@/components/ui/date-picker'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { api, errorMessage, q } from '@/lib/api'
import { DEFAULT_MINUTES, atMinute, minuteOfDay, parseTime, seriesDates } from '@/lib/calendar'

const SERIES_COUNTS = [1, 2, 4, 6, 8, 10, 12, 15, 20, 25]

export type SessionDialogMode =
  | { kind: 'create'; at?: Date; patientId?: string }
  | {
      kind: 'edit'
      session: Pick<Session, 'id' | 'scheduled_at' | 'started_at' | 'planned_minutes' | 'modality' | 'status' | 'note_status'>
      /** Has a recording or a note: it can still move, but not be cancelled; deleting asks more firmly. */
      documented?: boolean
    }

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

  const minute = parseTime(time)
  const start = () => atMinute(day, minute ?? 0)
  const length = Number(minutes)
  const lengthOk = Number.isInteger(length) && length >= 5 && length <= 480
  const endOf = () => format(addMinutes(start(), lengthOk ? length : 0), 'HH:mm')
  // Ende and Dauer follow each other: a typed end sets the length, a new start or length moves the end.
  const [end, setEnd] = useState(endOf)
  const endMinute = parseTime(end)
  const endLength = minute !== null && endMinute !== null ? (endMinute - minute + 24 * 60) % (24 * 60) : null
  const endOk = endLength !== null && endLength >= 5 && endLength <= 480
  useEffect(() => {
    if (minute !== null && lengthOk && !(endOk && endLength === length)) setEnd(endOf())
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [time, minutes, day])
  const changeEnd = (v: string) => {
    setEnd(v)
    const m = parseTime(v)
    if (minute === null || m === null) return
    const len = (m - minute + 24 * 60) % (24 * 60)
    if (len >= 5 && len <= 480) setMinutes(String(len))
  }
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

  const cancelled = editing?.status === 'cancelled'
  const documented = mode.kind === 'edit' && !!mode.documented
  const busy = save.isPending || cancel.isPending || reopen.isPending

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
        <DialogDescription>{editing ? t(documented ? 'calendar.editDocumentedHint' : 'calendar.editHint') : t('calendar.newHint')}</DialogDescription>
      </DialogHeader>
      <div className="grid gap-4">
        {!editing && (
          <div className="grid gap-2">
            <Label>{t('calendar.patient')}</Label>
            <Select value={patientId} onValueChange={setPatientId}>
              <SelectTrigger className="w-full" data-testid="session-patient">
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
        <div className="grid grid-cols-[1fr_8rem] gap-3">
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
                <Calendar
                  mode="single"
                  selected={day}
                  onSelect={(d) => {
                    // The picked date keeps the typed time.
                    if (d) setDay(d)
                    setDayOpen(false)
                  }}
                  defaultMonth={day}
                  weekStartsOn={1}
                />
              </PopoverContent>
            </Popover>
          </div>
          <div className="grid gap-2">
            <Label htmlFor="session-time">{t('calendar.time')}</Label>
            <TimePicker
              id="session-time"
              value={time}
              onChange={setTime}
              onBlur={() => minute !== null && setTime(format(start(), 'HH:mm'))}
              invalid={minute === null}
              data-testid="session-time"
            />
          </div>
        </div>
        <div className="grid grid-cols-[1fr_8rem] gap-3">
          <div className="grid gap-2">
            <Label htmlFor="session-length">{t('calendar.length')}</Label>
            <DurationPicker id="session-length" value={minutes} onChange={setMinutes} invalid={!lengthOk} data-testid="session-length" />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="session-end">{t('calendar.end')}</Label>
            <TimePicker
              id="session-end"
              value={end}
              onChange={changeEnd}
              onBlur={() => setEnd(endOf())}
              invalid={!endOk}
              data-testid="session-end"
            />
          </div>
          {!lengthOk && (
            <p className="col-span-2 -mt-1 text-xs text-destructive">{t('calendar.lengthRange')}</p>
          )}
        </div>
        <div className="grid gap-2">
          <Label>{t('calendar.modality')}</Label>
          <ToggleGroup
            type="single"
            variant="outline"
            value={modality}
            onValueChange={(v) => v && setModality(v)}
            className="w-full"
            aria-label={t('calendar.modality')}
          >
            <ToggleGroupItem value="praesenz" className="flex-1" data-testid="session-praesenz">
              <Building2 /> {t('calendar.praesenz')}
            </ToggleGroupItem>
            <ToggleGroupItem value="video" className="flex-1" data-testid="session-video">
              <Video /> {t('calendar.video')}
            </ToggleGroupItem>
          </ToggleGroup>
        </div>
        {!editing && (
          <div className="grid grid-cols-2 gap-3">
            <div className="grid gap-2">
              <Label>{t('calendar.series')}</Label>
              <Select value={count} onValueChange={setCount}>
                <SelectTrigger className="w-full" data-testid="session-count">
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
                <SelectTrigger className="w-full">
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
          {editing && !cancelled && !documented && (
            <Button variant="ghost" className="text-destructive" onClick={() => setCancelling(true)} disabled={busy} data-testid="session-cancel">
              <XCircle /> {t('calendar.cancelSession')}
            </Button>
          )}
          {cancelled && (
            <Button variant="ghost" onClick={() => reopen.mutate()} disabled={busy} data-testid="session-reopen">
              <Undo2 /> {t('calendar.reopen')}
            </Button>
          )}
          {editing && <DeleteSession session={editing} documented={documented} onDeleted={close} />}
        </div>
        <Button onClick={() => save.mutate()} disabled={busy || (!editing && !patientId) || minute === null || !lengthOk} data-testid="session-save">
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
