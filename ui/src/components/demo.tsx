// Demo-Praxis (slice 34): fictional data in its own folder. Entering or leaving restarts the
// app, so a window only ever shows one of the two practices.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Compass, FlaskConical, LogOut, Sparkles } from 'lucide-react'
import { api, errorMessage } from '@/lib/api'
import { Button } from '@/components/ui/button'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import { useTour } from '@/components/tour'

export function useDemo() {
  const init = useQuery({ queryKey: ['init'], queryFn: api.init, staleTime: Infinity })
  return init.data?.demo ?? false
}

/** Shown on every page of the Demo-Praxis. */
export function DemoBanner() {
  const { t } = useTranslation()
  const demo = useDemo()
  const tour = useTour()
  const leave = useMutation({ mutationFn: api.demoLeave, onError: (e) => toast.error(errorMessage(e)) })
  if (!demo) return null
  return (
    <div role="status" className="flex flex-wrap items-center gap-x-3 gap-y-1.5 border-b bg-sand px-8 py-2.5 text-sm text-sand-foreground" data-testid="demo-banner">
      <FlaskConical className="size-4 shrink-0" />
      <span className="min-w-64 flex-1">
        <span className="font-medium">{t('demo.bannerTitle')}</span> {t('demo.bannerBody')}
      </span>
      <DemoAiButton />
      <Button size="sm" variant="ghost" onClick={tour.start} disabled={tour.running}>
        <Compass /> {t('tour.start')}
      </Button>
      <Button size="sm" variant="outline" onClick={() => leave.mutate()} disabled={leave.isPending} data-testid="demo-leave">
        <LogOut /> {leave.isPending ? t('demo.restarting') : t('demo.leave')}
      </Button>
    </div>
  )
}

/** Opt-in (PLAN-AMENDMENTS §20 H3): use the real practice's AI provider and key in this demo visit. */
function DemoAiButton() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const info = useQuery({ queryKey: ['demo-ai'], queryFn: api.demoAiInfo })
  const use = useMutation({
    mutationFn: api.demoAiUse,
    onSuccess: () => void qc.invalidateQueries(),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const ai = info.data
  if (!ai) return null
  if (ai.active) {
    return (
      <span className="flex items-center gap-1.5 text-xs" data-testid="demo-ai-active">
        <Sparkles className="size-3.5" /> {t('demo.aiActive', { provider: ai.provider })}
      </span>
    )
  }
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button size="sm" variant="ghost" data-testid="demo-ai-use">
          <Sparkles /> {t('demo.aiUse')}
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('demo.aiTitle')}</AlertDialogTitle>
          <AlertDialogDescription>{t('demo.aiBody', { provider: ai.provider, model: ai.model ? ` (${ai.model})` : '' })}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
          <AlertDialogAction onClick={() => use.mutate()} disabled={use.isPending} data-testid="demo-ai-confirm">
            {t('demo.aiAction')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

/** "Mit Beispieldaten ausprobieren": asks once, then restarts into a fresh Demo-Praxis. */
export function DemoEnterButton({ variant = 'outline' }: { variant?: 'outline' | 'secondary' | 'default' }) {
  const { t } = useTranslation()
  const demo = useDemo()
  const enter = useMutation({ mutationFn: api.demoEnter, onError: (e) => toast.error(errorMessage(e)) })
  if (demo) return null
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button variant={variant} data-testid="demo-enter">
          <FlaskConical /> {t('demo.enter')}
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('demo.enterTitle')}</AlertDialogTitle>
          <AlertDialogDescription>{t('demo.enterBody')}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
          <AlertDialogAction onClick={() => enter.mutate()} disabled={enter.isPending} data-testid="demo-enter-confirm">
            {t('demo.enterAction')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
