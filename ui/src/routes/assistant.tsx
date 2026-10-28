// The assistant as a page (slice 25): all threads on the left, the chosen one on the right.
import { useRef, useState } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { MessageSquare, Plus, Trash2 } from 'lucide-react'
import { formatDate } from '@/components/bits'
import { AssistantChat } from '@/components/assistant'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { api, errorMessage, q } from '@/lib/api'
import { cn } from '@/lib/utils'

type Search = { thread?: string }

export const Route = createFileRoute('/assistant')({
  component: AssistantPage,
  validateSearch: (s: Record<string, unknown>): Search => ({ thread: typeof s.thread === 'string' ? s.thread : undefined }),
})

function AssistantPage() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const navigate = useNavigate({ from: Route.fullPath })
  const { thread } = Route.useSearch()
  const threads = useQuery(q.chatThreads())
  const [deleting, setDeleting] = useState<string | null>(null)
  const remove = useMutation({
    mutationFn: (id: string) => api.deleteChatThread(id),
    onSuccess: (_, id) => {
      void qc.invalidateQueries({ queryKey: ['chat'] })
      if (id === thread) void navigate({ search: {} })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const choose = (id?: string) => void navigate({ search: { thread: id } })
  // A thread created by the first question keeps its chat mounted (the answer is still coming).
  const created = useRef<string | null>(null)
  const [draft, setDraft] = useState(0)
  const startNew = () => {
    created.current = null
    setDraft((n) => n + 1)
    choose(undefined)
  }
  const chatKey = thread && thread !== created.current ? thread : `new-${draft}`

  return (
    <div className="grid h-full min-h-0 grid-cols-[17rem_minmax(0,1fr)]">
      <aside className="flex min-h-0 flex-col border-r">
        <div className="flex items-center justify-between px-4 pt-5 pb-3">
          <h1 className="font-serif text-xl">{t('assistant.title')}</h1>
          <Button size="sm" variant="outline" onClick={startNew} data-testid="assistant-new">
            <Plus /> {t('assistant.new')}
          </Button>
        </div>
        <ul className="min-h-0 flex-1 space-y-0.5 overflow-y-auto px-2 pb-4">
          {threads.data?.length === 0 && <li className="px-2 py-1 text-xs text-muted-foreground">{t('assistant.noThreads')}</li>}
          {threads.data?.map((th) => (
            <li key={th.id} className="group relative">
              <button
                type="button"
                onClick={() => choose(th.id)}
                className={cn(
                  'flex w-full items-start gap-2 rounded-md px-2 py-2 text-left text-sm hover:bg-foreground/5',
                  th.id === thread && 'bg-card shadow-xs',
                )}
              >
                <MessageSquare className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
                <span className="min-w-0 flex-1">
                  <span className="block truncate pr-5">{th.title ?? t('assistant.untitled')}</span>
                  <span className="block truncate text-xs text-muted-foreground">
                    {th.patient_name ? `${th.patient_name} · ` : ''}
                    {formatDate(th.updated_at, true)}
                  </span>
                </span>
              </button>
              <button
                type="button"
                onClick={() => setDeleting(th.id)}
                className="absolute top-2 right-2 hidden text-muted-foreground group-hover:block hover:text-destructive"
                aria-label={t('assistant.delete')}
                title={t('assistant.delete')}
              >
                <Trash2 className="size-3.5" />
              </button>
            </li>
          ))}
        </ul>
      </aside>
      <AssistantChat
        key={chatKey}
        threadId={thread ?? null}
        scope="global"
        onThread={(id) => {
          created.current = id
          choose(id)
        }}
      />
      <AlertDialog open={!!deleting} onOpenChange={(o) => !o && setDeleting(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('assistant.deleteTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('assistant.deleteText')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction onClick={() => deleting && remove.mutate(deleting)}>{t('assistant.delete')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
