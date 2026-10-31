import { createFileRoute, Link } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { FileText, CalendarDays, RotateCcw, Trash2 } from 'lucide-react'
import { formatDate } from '@/components/bits'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
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
import { api, errorMessage } from '@/lib/api'
import type { TrashItem } from '@/bindings/TrashItem'

export const Route = createFileRoute('/settings/papierkorb')({ component: Papierkorb })

const TRASH_DAYS = 30

function Papierkorb() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const items = useQuery({ queryKey: ['trash'], queryFn: api.trash })
  const purgeAll = useMutation({
    mutationFn: () => api.trashPurge(),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['trash'] }),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const count = items.data?.length ?? 0

  return (
    <>
      <p className="flex items-center justify-between gap-4 text-sm text-muted-foreground">
        {t('trash.hint', { days: TRASH_DAYS })}
        {count > 0 && (
          <Confirm
            title={t('trash.purgeAllTitle')}
            text={t('trash.purgeAllText', { count })}
            action={t('trash.purgeAll')}
            onConfirm={() => purgeAll.mutate()}
            trigger={
              <Button size="sm" variant="outline" className="shrink-0" disabled={purgeAll.isPending}>
                <Trash2 /> {t('trash.purgeAll')}
              </Button>
            }
          />
        )}
      </p>
      <Card>
        <CardContent className="divide-y py-2">
          {items.data && count === 0 && <p className="py-6 text-center text-sm text-muted-foreground">{t('trash.empty')}</p>}
          {items.data?.map((item) => <Row key={item.id} item={item} />)}
        </CardContent>
      </Card>
    </>
  )
}

function Row({ item }: { item: TrashItem }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const restore = useMutation({
    mutationFn: () => api.trashRestore(item.id),
    onSuccess: () => {
      void qc.invalidateQueries()
      toast.success(t('trash.restored'))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const purge = useMutation({
    mutationFn: () => api.trashPurge(item.id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['trash'] }),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const session = item.kind === 'session'
  const Icon = session ? CalendarDays : FileText
  const what = session ? t('trash.session') : item.detail ? t(`enums.kind.${item.detail}`) : t('trash.bericht')
  return (
    <div className="flex items-center gap-3 py-3" data-testid="trash-item">
      <Icon className="size-4 shrink-0 text-muted-foreground" />
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm">
          <span className="font-medium">{what}</span> {formatDate(item.occurred_at, session)} ·{' '}
          <Link to="/patients/$patientId" params={{ patientId: item.patient_id }} className="hover:underline">
            {item.patient_name}
          </Link>
        </div>
        <div className="text-xs text-muted-foreground">
          {t('trash.deletedAt', { date: formatDate(item.deleted_at) })} · {t('trash.purgeAt', { date: formatDate(item.purge_at) })}
        </div>
      </div>
      <Button size="sm" variant="outline" onClick={() => restore.mutate()} disabled={restore.isPending} data-testid="trash-restore">
        <RotateCcw /> {t('trash.restore')}
      </Button>
      <Confirm
        title={t('trash.purgeTitle')}
        text={t('trash.purgeText')}
        action={t('trash.purge')}
        onConfirm={() => purge.mutate()}
        trigger={
          <Button size="icon" variant="ghost" className="text-muted-foreground hover:text-destructive" title={t('trash.purge')} aria-label={t('trash.purge')} disabled={purge.isPending}>
            <Trash2 />
          </Button>
        }
      />
    </div>
  )
}

function Confirm({ title, text, action, onConfirm, trigger }: { title: string; text: string; action: string; onConfirm: () => void; trigger: ReactNode }) {
  const { t } = useTranslation()
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>{trigger}</AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{text}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={onConfirm}>
            {action}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
