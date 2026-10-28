// Diktat-Werkstatt for a session: dictates towards its Sitzungsnotiz (created on the way in).
import { useEffect } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { formatDate } from '@/components/bits'
import { Workshop } from '@/components/workshop'
import { api, errorMessage, q } from '@/lib/api'

export const Route = createFileRoute('/sessions/$sessionId_/werkstatt')({ component: SessionWorkshopPage })

function SessionWorkshopPage() {
  const { sessionId } = Route.useParams()
  const qc = useQueryClient()
  const session = useQuery(q.session(sessionId))
  const noteId = session.data?.session.note_id
  const create = useMutation({
    mutationFn: () => api.createSessionNote(sessionId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['session', sessionId] })
      void qc.invalidateQueries({ queryKey: ['sessions'] })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const { isIdle, mutate } = create
  useEffect(() => {
    if (session.data && !noteId && isIdle) mutate()
  }, [session.data, noteId, isIdle, mutate])
  const view = useQuery({ ...q.report(noteId ?? ''), enabled: !!noteId })
  if (!view.data || !session.data) return null
  const s = session.data.session
  return <Workshop view={view.data} title={`Sitzung vom ${formatDate(s.scheduled_at ?? s.started_at ?? view.data.report.created_at)}`} />
}
