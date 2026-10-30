// Rundgang (slice 34): coach marks over the real pages. Each step opens its page and points at
// an element marked `data-tour="…"`; without one it shows in the middle. It starts once after
// the first setup (and the Notfall-Kit) and can be restarted from Hilfe and Einstellungen.
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { useNavigate, useRouterState } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { openUrl } from '@tauri-apps/plugin-opener'
import { PlayCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Popover, PopoverAnchor, PopoverContent } from '@/components/ui/popover'
import { VIDEO_URL } from '@/lib/help'

/** `fallback` is pointed at when `target` doesn't show up, e.g. a list that is still empty. */
type Step = { id: string; to: string; target: string; fallback?: string; side: 'right' | 'bottom' }

export const TOUR_STEPS: Step[] = [
  { id: 'heute', to: '/', target: 'nav-dashboard', side: 'right' },
  { id: 'kalender', to: '/calendar', target: 'nav-calendar', side: 'right' },
  { id: 'patient', to: '/patients', target: 'patient-new', side: 'bottom' },
  { id: 'aufnehmen', to: '/sessions', target: 'nav-sessions', side: 'right' },
  { id: 'autopilot', to: '/settings/allgemein', target: 'autopilot', side: 'bottom' },
  { id: 'notiz', to: '/sessions', target: 'session-filters', fallback: 'nav-sessions', side: 'bottom' },
  { id: 'bericht', to: '/reports', target: 'nav-reports', side: 'right' },
  { id: 'ki', to: '/settings/ki', target: 'llm-settings', side: 'bottom' },
]

/** `done` | `skipped`; unset = the tour hasn't run on this computer yet. */
export const TOUR_KEY = 'thera.tour'
/** How long a step waits for its element before it shows in the middle. */
const FIND_MS = 2500
/** How long a step waits for its own element before it takes the fallback. */
const FALLBACK_MS = 1000
const PAD = 6

type TourApi = { start: () => void; running: boolean }
const TourContext = createContext<TourApi>({ start: () => {}, running: false })

export function useTour() {
  return useContext(TourContext)
}

export function TourProvider({ children }: { children: ReactNode }) {
  const [step, setStep] = useState<number | null>(null)
  const start = useCallback(() => setStep(0), [])
  const api = useMemo(() => ({ start, running: step !== null }), [start, step])

  useEffect(() => {
    if (localStorage.getItem(TOUR_KEY)) return
    const timer = window.setTimeout(() => setStep((s) => s ?? 0), 800)
    return () => window.clearTimeout(timer)
  }, [])

  const end = (how: 'done' | 'skipped') => {
    localStorage.setItem(TOUR_KEY, how)
    setStep(null)
  }

  return (
    <TourContext.Provider value={api}>
      {children}
      {step !== null && (
        <TourStep
          index={step}
          onBack={() => setStep(Math.max(0, step - 1))}
          onNext={() => (step + 1 < TOUR_STEPS.length ? setStep(step + 1) : end('done'))}
          onEnd={() => end('skipped')}
        />
      )}
    </TourContext.Provider>
  )
}

type Rect = { top: number; left: number; width: number; height: number }

/** The step's element, measured while the step shows; `null` when it isn't there (yet). */
function useTarget(target: string, fallback: string | undefined, ready: boolean): { rect: Rect | null; side: 'own' | 'fallback'; searching: boolean } {
  const [rect, setRect] = useState<Rect | null>(null)
  const [searching, setSearching] = useState(true)
  const [which, setWhich] = useState<'own' | 'fallback'>('own')
  useEffect(() => {
    setRect(null)
    setSearching(true)
    setWhich('own')
    if (!ready) return
    const began = Date.now()
    let scrolled = false
    const tick = () => {
      const find = (key: string) => document.querySelector<HTMLElement>(`[data-tour="${key}"]`)
      const own = find(target)
      const el = own ?? (fallback && Date.now() - began > FALLBACK_MS ? find(fallback) : null)
      if (!el) {
        if (Date.now() - began > FIND_MS) setSearching(false)
        return
      }
      if (!scrolled) {
        el.scrollIntoView({ block: 'center', inline: 'nearest' })
        scrolled = true
      }
      const r = el.getBoundingClientRect()
      setWhich(own ? 'own' : 'fallback')
      setSearching(false)
      setRect((old) =>
        old && old.top === r.top && old.left === r.left && old.width === r.width && old.height === r.height
          ? old
          : { top: r.top, left: r.left, width: r.width, height: r.height },
      )
    }
    tick()
    const timer = window.setInterval(tick, 150)
    return () => window.clearInterval(timer)
  }, [target, fallback, ready])
  return { rect, side: which, searching }
}

function TourStep({ index, onBack, onNext, onEnd }: { index: number; onBack: () => void; onNext: () => void; onEnd: () => void }) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const step = TOUR_STEPS[index]
  const path = useRouterState({ select: (s) => s.location.pathname })
  const there = path === step.to

  useEffect(() => {
    if (!there) void navigate({ to: step.to })
  }, [there, step.to, navigate])

  const { rect, side, searching } = useTarget(step.target, step.fallback, there)
  // Fallbacks are sidebar entries.
  const placement = side === 'fallback' ? 'right' : step.side
  const last = index === TOUR_STEPS.length - 1
  // Without an element the card sits in the middle of the window.
  const anchor: Rect = rect
    ? { top: rect.top - PAD, left: rect.left - PAD, width: rect.width + 2 * PAD, height: rect.height + 2 * PAD }
    : { top: window.innerHeight / 2 - 120, left: window.innerWidth / 2, width: 0, height: 0 }

  return (
    <>
      {/* Blocks the page while the tour runs; dims it when there is no spotlight. */}
      <div className={rect ? 'fixed inset-0 z-[60]' : 'fixed inset-0 z-[60] bg-black/45'} data-testid="tour-overlay" />
      <Popover open={!searching}>
        <PopoverAnchor asChild>
          <div
            className="pointer-events-none fixed z-[61] rounded-lg transition-all duration-200"
            style={{ ...anchor, boxShadow: rect ? '0 0 0 9999px rgb(0 0 0 / 0.45), 0 0 0 2px var(--primary)' : undefined }}
            data-testid="tour-spotlight"
          />
        </PopoverAnchor>
        <PopoverContent
          side={rect ? placement : 'bottom'}
          align={rect && placement === 'right' ? 'start' : 'center'}
          sideOffset={10}
          collisionPadding={16}
          className="z-[70] w-80 space-y-3"
          onInteractOutside={(e) => e.preventDefault()}
          onEscapeKeyDown={onEnd}
          data-testid="tour-card"
        >
          <div className="text-sm">
            <span className="text-muted-foreground">{t('tour.progress', { n: index + 1, total: TOUR_STEPS.length })}</span>
            <span className="text-muted-foreground"> · </span>
            <span className="font-medium">{t(`tour.steps.${step.id}.title`)}</span>
          </div>
          <p className="text-sm leading-relaxed">{t(`tour.steps.${step.id}.body`)}</p>
          {last && <p className="text-xs text-muted-foreground">{t('tour.restartHint')}</p>}
          {last && VIDEO_URL && (
            <button type="button" className="flex items-center gap-1.5 text-sm text-primary hover:underline" onClick={() => void openUrl(VIDEO_URL!)} data-testid="tour-video">
              <PlayCircle className="size-4" /> {t('tour.video')}
            </button>
          )}
          <div className="flex items-center justify-between gap-2 border-t pt-3">
            <Button variant="outline" size="sm" onClick={onBack} disabled={index === 0} data-testid="tour-back">
              {t('common.back')}
            </Button>
            <Button size="sm" onClick={onNext} data-testid="tour-next">
              {last ? t('tour.finish') : t('tour.next')}
            </Button>
          </div>
          {!last && (
            <button type="button" className="block w-full text-center text-xs text-primary hover:underline" onClick={onEnd} data-testid="tour-end">
              {t('tour.end')}
            </button>
          )}
        </PopoverContent>
      </Popover>
    </>
  )
}
