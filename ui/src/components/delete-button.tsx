import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Trash2 } from 'lucide-react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
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
import { api, errorMessage } from '@/lib/api'

/** The one way to delete something: a trash button, a confirm, then every query refreshes. */
export function DeleteButton({
  title,
  description,
  warning,
  action,
  onDeleted,
  label,
  testId,
}: {
  title: string
  description: ReactNode
  /** Extra line in the warning colour, e.g. the retention period of signed documents. */
  warning?: string | null
  action: () => Promise<unknown>
  onDeleted?: () => void
  /** Shown next to the icon; without it the button is icon-only (list rows). */
  label?: string
  testId?: string
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const remove = useMutation({
    mutationFn: action,
    onSuccess: () => {
      onDeleted?.()
      void qc.invalidateQueries()
      toast.success(t('common.deleted'))
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button
          variant="ghost"
          size={label ? 'sm' : 'icon'}
          className="shrink-0 text-muted-foreground hover:text-destructive"
          disabled={remove.isPending}
          title={label ? undefined : title}
          aria-label={title}
          onClick={(e) => e.stopPropagation()}
          data-testid={testId}
        >
          <Trash2 /> {label}
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent onClick={(e) => e.stopPropagation()}>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
          {warning && <p className="text-sm font-medium text-warning-foreground">{warning}</p>}
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={() => remove.mutate()} data-testid="delete-confirm">
            {t('common.delete')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

const signed = (status: string | null | undefined) => status === 'signed' || status === 'exported'

export function DeleteSession({
  session,
  documented = true,
  onDeleted,
  label,
}: {
  session: { id: string; note_status: string | null }
  /** False for an appointment with nothing attached yet: the confirm stays short. */
  documented?: boolean
  onDeleted?: () => void
  label?: boolean
}) {
  const { t } = useTranslation()
  return (
    <DeleteButton
      title={t('sessions.delete')}
      description={t(documented ? 'sessions.deleteText' : 'sessions.deleteEmptyText')}
      warning={signed(session.note_status) ? t('common.retention') : null}
      action={() => api.deleteSession(session.id)}
      onDeleted={onDeleted}
      label={label ? t('common.delete') : undefined}
      testId="session-delete"
    />
  )
}

export function DeleteReport({ report, onDeleted, label }: { report: { id: string; status: string }; onDeleted?: () => void; label?: boolean }) {
  const { t } = useTranslation()
  return (
    <DeleteButton
      title={t('reports.delete')}
      description={t('reports.deleteText')}
      warning={signed(report.status) ? t('common.retention') : null}
      action={() => api.deleteReport(report.id)}
      onDeleted={onDeleted}
      label={label ? t('common.delete') : undefined}
      testId="report-delete"
    />
  )
}

export function DeletePatient({ patientId, onDeleted, label }: { patientId: string; onDeleted?: () => void; label?: boolean }) {
  const { t } = useTranslation()
  return (
    <DeleteButton
      title={t('patients.erase')}
      description={t('patients.eraseConfirm')}
      action={() => api.erasePatient(patientId)}
      onDeleted={onDeleted}
      label={label ? t('patients.erase') : undefined}
      testId="patient-delete"
    />
  )
}
