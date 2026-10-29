// Demo-Praxis (slice 34): fictional data in its own folder. Entering or leaving restarts the
// app, so a window only ever shows one of the two practices.
import { useMutation, useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Compass, FlaskConical, LogOut } from 'lucide-react'
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
    <div role="status" className="flex items-center gap-3 border-b bg-sand px-8 py-2.5 text-sm text-sand-foreground" data-testid="demo-banner">
      <FlaskConical className="size-4 shrink-0" />
      <span className="min-w-0 flex-1">
        <span className="font-medium">{t('demo.bannerTitle')}</span> {t('demo.bannerBody')}
      </span>
      <Button size="sm" variant="ghost" onClick={tour.start} disabled={tour.running}>
        <Compass /> {t('tour.start')}
      </Button>
      <Button size="sm" variant="outline" onClick={() => leave.mutate()} disabled={leave.isPending} data-testid="demo-leave">
        <LogOut /> {leave.isPending ? t('demo.restarting') : t('demo.leave')}
      </Button>
    </div>
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
