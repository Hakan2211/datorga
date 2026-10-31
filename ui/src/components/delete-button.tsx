import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
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

/** The one way to delete something: a trash button, a confirm, then every query refreshes.
 *  Signed documents land in the Papierkorb; the toast then links there. */
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
  const navigate = useNavigate()
  const remove = useMutation({
    mutationFn: action,
    onSuccess: (outcome) => {
      onDeleted?.()
      void qc.invalidateQueries()
      if ((outcome as { trashed?: boolean } | undefined)?.trashed) {
        toast.success(t('trash.moved'), {
          action: { label: t('trash.open'), onClick: () => void navigate({ to: '/settings/papierkorb' }) },
        })
      } else {
        toast.success(t('common.deleted'))
      }
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
      description={
        documented ? `${t('sessions.deleteText')} ${signed(session.note_status) ? '' : t('common.irreversible')}` : t('sessions.deleteEmptyText')
      }
      warning={signed(session.note_status) ? t('common.toTrash') : null}
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
      description={`${t('reports.deleteText')} ${signed(report.status) ? '' : t('common.irreversible')}`}
      warning={signed(report.status) ? t('common.toTrash') : null}
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
