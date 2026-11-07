// Date and time fields on top of the shadcn Calendar + Popover: type "12.3.1985" or pick from the
// calendar. Values stay what the app stores: "yyyy-MM-dd" days and "HH:mm" times.
import * as React from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from 'cn'
import { CalendarDays, Clock, Hourglass } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Calendar } from '@/components/ui/calendar'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { dateToDay, dayToDate, formatTypedDay, parseTypedDay, timeSlots } from '@/lib/dates'

type DatePickerProps = {
  value: string | null | undefined
  onChange: (day: string | null) => void
  id?: string
  min?: string
  max?: string
  disabled?: boolean
  placeholder?: string
  /** Month/year dropdowns in the calendar header, for far-away dates like birthdays. */
  dropdowns?: boolean
  className?: string
  'data-testid'?: string
}

export function DatePicker({ value, onChange, id, min, max, disabled, placeholder = 'TT.MM.JJJJ', dropdowns, className, ...rest }: DatePickerProps) {
  const { t } = useTranslation()
  const [open, setOpen] = React.useState(false)
  const [text, setText] = React.useState(formatTypedDay(value))
  // Follow outside changes (another field, a reset) unless the typed text already means this day.
  React.useEffect(() => {
    setText((cur) => (parseTypedDay(cur) === (value ?? null) && cur ? cur : formatTypedDay(value)))
  }, [value])

  const selected = dayToDate(value) ?? undefined
  const minDate = dayToDate(min) ?? undefined
  const maxDate = dayToDate(max) ?? undefined
  const inRange = (day: string) => (!min || day >= min) && (!max || day <= max)
  const parsed = parseTypedDay(text)
  const invalid = !!text.trim() && (parsed === null || !inRange(parsed))
  const today = dateToDay(new Date())
  const [month, setMonth] = React.useState<Date>(selected ?? maxDate ?? new Date())
  React.useEffect(() => {
    if (open) setMonth(selected ?? (max && max < today ? maxDate! : new Date()))
    // Only when opening: browsing months must not snap back.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  const pick = (day: string | null) => {
    onChange(day)
    setText(formatTypedDay(day))
    setOpen(false)
  }

  return (
    <div className={cn('relative', className)}>
      <Input
        id={id}
        value={text}
        disabled={disabled}
        placeholder={placeholder}
        inputMode="numeric"
        autoComplete="off"
        aria-invalid={invalid || undefined}
        className="pr-9 tabular-nums"
        onChange={(e) => {
          const next = e.target.value
          setText(next)
          const day = parseTypedDay(next)
          if (!next.trim()) onChange(null)
          else if (day && inRange(day)) onChange(day)
        }}
        onBlur={() => !invalid && setText(formatTypedDay(value))}
        onKeyDown={(e) => {
          if (e.key === 'ArrowDown' && e.altKey) {
            e.preventDefault()
            setOpen(true)
          }
        }}
        data-testid={rest['data-testid']}
      />
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            disabled={disabled}
            className="absolute top-1/2 right-1 size-7 -translate-y-1/2 text-muted-foreground hover:text-foreground"
            aria-label={t('common.pickDate')}
          >
            <CalendarDays className="size-4" />
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-auto overflow-hidden p-0" align="end" sideOffset={8}>
          <Calendar
            mode="single"
            selected={selected}
            month={month}
            onMonthChange={setMonth}
            onSelect={(d) => d && pick(dateToDay(d))}
            weekStartsOn={1}
            captionLayout={dropdowns ? 'dropdown' : 'label'}
            startMonth={dropdowns ? (minDate ?? new Date(1920, 0)) : minDate}
            endMonth={dropdowns ? (maxDate ?? new Date(new Date().getFullYear() + 5, 11)) : maxDate}
            disabled={[...(minDate ? [{ before: minDate }] : []), ...(maxDate ? [{ after: maxDate }] : [])]}
          />
          <div className="flex items-center justify-between gap-2 border-t px-3 py-2">
            <Button type="button" variant="ghost" size="sm" className="h-7 text-muted-foreground" onClick={() => pick(null)} disabled={!value}>
              {t('common.clear')}
            </Button>
            <Button type="button" variant="outline" size="sm" className="h-7" onClick={() => pick(today)} disabled={!inRange(today)}>
              {t('common.today')}
            </Button>
          </div>
        </PopoverContent>
      </Popover>
    </div>
  )
}

type TimePickerProps = {
  value: string
  onChange: (time: string) => void
  onBlur?: () => void
  id?: string
  invalid?: boolean
  disabled?: boolean
  className?: string
  'data-testid'?: string
}

/** "HH:mm" typed freely (9.40, 0940) or picked from quarter-hour slots. */
export function TimePicker({ value, onChange, onBlur, id, invalid, disabled, className, ...rest }: TimePickerProps) {
  const { t } = useTranslation()
  const [open, setOpen] = React.useState(false)
  const slots = React.useMemo(() => timeSlots(6, 22), [])
  const list = React.useRef<HTMLDivElement>(null)
  // Scroll the current (or next) slot into view when the list opens.
  const current = slots.find((s) => s >= value) ?? slots.at(-1)
  React.useEffect(() => {
    if (!open) return
    const id = requestAnimationFrame(() => list.current?.querySelector<HTMLElement>(`[data-slot-time="${current}"]`)?.scrollIntoView({ block: 'center' }))
    return () => cancelAnimationFrame(id)
  }, [open, current])

  return (
    <div className={cn('relative', className)}>
      <Input
        id={id}
        inputMode="numeric"
        maxLength={5}
        placeholder="09:00"
        autoComplete="off"
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
        onBlur={onBlur}
        aria-invalid={invalid || undefined}
        className="pr-9 tabular-nums"
        data-testid={rest['data-testid']}
      />
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            disabled={disabled}
            className="absolute top-1/2 right-1 size-7 -translate-y-1/2 text-muted-foreground hover:text-foreground"
            aria-label={t('common.pickTime')}
          >
            <Clock className="size-4" />
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-32 p-1" align="end" sideOffset={8}>
          <div ref={list} className="max-h-64 overflow-y-auto overscroll-contain" role="listbox">
            {slots.map((s) => (
              <button
                key={s}
                type="button"
                role="option"
                aria-selected={s === value}
                data-slot-time={s}
                onClick={() => {
                  onChange(s)
                  setOpen(false)
                }}
                className={cn(
                  'flex w-full items-center justify-center rounded-sm px-2 py-1.5 text-sm tabular-nums outline-none hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent',
                  s === value && 'bg-primary text-primary-foreground hover:bg-primary hover:text-primary-foreground',
                  s.endsWith(':00') && s !== value && 'font-medium',
                )}
              >
                {s}
              </button>
            ))}
          </div>
        </PopoverContent>
      </Popover>
    </div>
  )
}

const DURATIONS = [10, 15, 20, 25, 30, 40, 45, 50, 60, 75, 90, 100, 120, 150, 180, 240]

type DurationPickerProps = {
  /** Minutes as typed. */
  value: string
  onChange: (minutes: string) => void
  id?: string
  invalid?: boolean
  disabled?: boolean
  className?: string
  'data-testid'?: string
}

/** Minutes typed freely or picked from common lengths. */
export function DurationPicker({ value, onChange, id, invalid, disabled, className, ...rest }: DurationPickerProps) {
  const { t } = useTranslation()
  const [open, setOpen] = React.useState(false)
  const list = React.useRef<HTMLDivElement>(null)
  const current = String(DURATIONS.find((m) => m >= Number(value)) ?? DURATIONS.at(-1))
  React.useEffect(() => {
    if (!open) return
    const id = requestAnimationFrame(() => list.current?.querySelector<HTMLElement>(`[data-slot-minutes="${current}"]`)?.scrollIntoView({ block: 'center' }))
    return () => cancelAnimationFrame(id)
  }, [open, current])

  return (
    <div className={cn('relative', className)}>
      <Input
        id={id}
        inputMode="numeric"
        maxLength={3}
        autoComplete="off"
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value.replace(/\D/g, ''))}
        aria-invalid={invalid || undefined}
        className="pr-20 tabular-nums"
        data-testid={rest['data-testid']}
      />
      <span className="pointer-events-none absolute top-1/2 right-10 -translate-y-1/2 text-sm text-muted-foreground">{t('calendar.minutesShort')}</span>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            disabled={disabled}
            className="absolute top-1/2 right-1 size-7 -translate-y-1/2 text-muted-foreground hover:text-foreground"
            aria-label={t('calendar.pickLength')}
          >
            <Hourglass className="size-4" />
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-48 p-1" align="end" sideOffset={8}>
          <div ref={list} className="max-h-64 overflow-y-auto overscroll-contain" role="listbox">
            {DURATIONS.map((m) => {
              const on = String(m) === value
              return (
                <button
                  key={m}
                  type="button"
                  role="option"
                  aria-selected={on}
                  data-slot-minutes={m}
                  onClick={() => {
                    onChange(String(m))
                    setOpen(false)
                  }}
                  className={cn(
                    'flex w-full items-center justify-between gap-2 rounded-sm px-2 py-1.5 text-sm whitespace-nowrap tabular-nums outline-none hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent',
                    on && 'bg-primary text-primary-foreground hover:bg-primary hover:text-primary-foreground',
                  )}
                >
                  <span>{t('calendar.minutes', { count: m })}</span>
                  {m % 60 === 0 && <span className={cn('text-xs', on ? 'opacity-80' : 'text-muted-foreground')}>{t('calendar.hours', { count: m / 60 })}</span>}
                </button>
              )
            })}
          </div>
        </PopoverContent>
      </Popover>
    </div>
  )
}
