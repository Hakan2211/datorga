import { useMemo, useState, type ReactNode } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Search } from 'lucide-react'
import type { BerichtCandidate } from '@/bindings/BerichtCandidate'
import { PatientAvatar } from '@/components/patient-avatar'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { api, errorMessage, q } from '@/lib/api'
import { cn } from '@/lib/utils'

/**
 * The one way into a Bericht from a case: opens its unsigned draft of the suggested kind, or
 * creates the application with the suggested kind and opens the Lotse's start step.
 */
export function useStartBericht() {
  const qc = useQueryClient()
  const navigate = useNavigate()
  return useMutation({
    mutationFn: async (c: BerichtCandidate) => {
      if (c.open_report_id) return c.open_report_id
      const app = await api.createApplication(c.episode_id, c.suggested)
      for (const key of [['applications', c.episode_id], ['reports'], ['berichtCandidates'], ['contingent'], ['timeline']]) {
        void qc.invalidateQueries({ queryKey: key })
      }
      return app.report_id
    },
    onSuccess: (reportId) => {
      if (reportId) void navigate({ to: '/reports/$reportId', params: { reportId }, search: { step: 'start' } })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
}

/** "VT · KZT 1 · Sitzung 10 von 12" */
export function caseLine(c: BerichtCandidate, t: (key: string, o?: Record<string, unknown>) => string) {
  const method = c.method
  return c.contingent.stage === 'probatorik'
    ? t('lotse.caseLineProbatorik', { method })
    : t('lotse.caseLine', { method, stage: t(`enums.stage.${c.contingent.stage}`), used: c.contingent.used, limit: c.contingent.limit })
}

/** "Neuer Bericht": pick the case, thera suggests the Antragsart or the draft to resume. */
export function NewReportDialog({ trigger }: { trigger: ReactNode }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [picked, setPicked] = useState<string | null>(null)
  const candidates = useQuery({ ...q.berichtCandidates(), enabled: open })
  const start = useStartBericht()
  const shown = useMemo(() => {
    const needle = query.trim().toLowerCase()
    const all = candidates.data ?? []
    if (!needle) return all
    return all.filter((c) => `${c.first_name} ${c.last_name} ${c.chiffre ?? ''}`.toLowerCase().includes(needle))
  }, [candidates.data, query])
  const chosen = shown.find((c) => c.episode_id === picked) ?? (shown.length === 1 ? shown[0] : undefined)

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o)
        if (!o) {
          setQuery('')
          setPicked(null)
        }
      }}
    >
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent className="sm:max-w-xl" data-testid="new-report-dialog">
        <DialogHeader>
          <DialogTitle className="font-serif text-xl font-normal">{t('lotse.newTitle')}</DialogTitle>
          <DialogDescription>{t('lotse.newHint')}</DialogDescription>
        </DialogHeader>
        <div className="relative">
          <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input autoFocus value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t('lotse.search')} className="pl-9" />
        </div>
        <ul className="-mx-1 max-h-[50vh] divide-y overflow-y-auto" role="listbox" aria-label={t('lotse.newTitle')}>
          {candidates.data?.length === 0 && <li className="px-1 py-6 text-sm text-muted-foreground">{t('lotse.noCases')}</li>}
          {candidates.data && candidates.data.length > 0 && shown.length === 0 && (
            <li className="px-1 py-6 text-sm text-muted-foreground">{t('lotse.noMatch')}</li>
          )}
          {shown.map((c) => {
            const selected = chosen?.episode_id === c.episode_id
            return (
              <li key={c.episode_id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={selected}
                  onClick={() => setPicked(c.episode_id)}
                  onDoubleClick={() => start.mutate(c)}
                  className={cn(
                    'flex w-full items-center gap-3 rounded-md px-2 py-2.5 text-left transition-colors hover:bg-foreground/5',
                    selected && 'bg-primary/8 ring-1 ring-primary/30',
                  )}
                  data-testid="new-report-case"
                >
                  <PatientAvatar
                    patientId={c.patient_id}
                    hasPhoto={c.has_photo}
                    firstName={c.first_name}
                    lastName={c.last_name}
                    className="size-10 text-sm"
                  />
                  <span className="flex min-w-0 flex-1 flex-col leading-tight">
                    <span className="truncate text-sm font-medium">
                      {c.first_name} {c.last_name}
                    </span>
                    {c.chiffre && <span className="truncate font-mono text-xs text-muted-foreground">{c.chiffre}</span>}
                  </span>
                  <span className="flex shrink-0 flex-col items-end text-right text-xs leading-snug text-muted-foreground">
                    <span>{caseLine(c, t)}</span>
                    <span className={cn(c.contingent.due && !c.open_report_id && 'font-medium text-primary')}>
                      {c.open_report_id ? t('lotse.openDraft') : t('lotse.suggests', { kind: t(`enums.kind.${c.suggested}`) })}
                    </span>
                  </span>
                </button>
              </li>
            )
          })}
        </ul>
        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)}>
            {t('common.cancel')}
          </Button>
          <Button disabled={!chosen || start.isPending} onClick={() => chosen && start.mutate(chosen)} data-testid="new-report-start">
            {t('calendar.next')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
