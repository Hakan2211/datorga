// Strg+K: jump to a page, patient or session, or run an action. Actions that need a session
// (dictate) only appear on a session page; "Neue Sitzung" first asks for the patient.
import { useState } from 'react'
import { useMatch, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { FileText, House, Library, Mic, NotebookPen, Plus, ScrollText, Settings, Square, User, Users } from 'lucide-react'
import { formatDate } from '@/components/bits'
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
} from '@/components/ui/command'
import { api, errorMessage, q } from '@/lib/api'
import { useCaptureDevice } from '@/lib/capture'

const pages = [
  { to: '/', label: 'nav.dashboard', icon: House },
  { to: '/patients', label: 'nav.patients', icon: Users },
  { to: '/sessions', label: 'nav.sessions', icon: Mic },
  { to: '/reports', label: 'nav.reports', icon: FileText },
  { to: '/templates', label: 'nav.templates', icon: Library },
  { to: '/audit', label: 'nav.audit', icon: ScrollText },
  { to: '/settings', label: 'nav.settings', icon: Settings },
] as const

const RECENT_SESSIONS = 8

export function CommandPalette({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [mode, setMode] = useState<'root' | 'newSession'>('root')
  const [search, setSearch] = useState('')
  const patients = useQuery({ ...q.patients(), enabled: open })
  const sessions = useQuery({ ...q.sessions(), enabled: open })
  const recording = useQuery({ ...q.recording(), refetchInterval: false, enabled: open })
  const sessionId = useMatch({ from: '/sessions/$sessionId', shouldThrow: false })?.params.sessionId
  const mic = useCaptureDevice()

  const close = () => {
    onOpenChange(false)
    setMode('root')
    setSearch('')
  }
  const go = (to: string, params?: Record<string, string>) => {
    close()
    void navigate({ to, params })
  }
  const refresh = () => {
    void qc.invalidateQueries({ queryKey: ['recording'] })
    void qc.invalidateQueries({ queryKey: ['session'] })
    void qc.invalidateQueries({ queryKey: ['sessions'] })
  }

  const newSession = useMutation({
    mutationFn: async (patientId: string) => {
      // The running treatment case, if any; the patient page's choice works the same way.
      const episode = (await api.episodes(patientId)).find((e) => !e.ended_at) ?? null
      return api.createSession({
        patient_id: patientId,
        episode_id: episode?.id ?? null,
        scheduled_at: new Date().toISOString(),
        modality: 'praesenz',
      })
    },
    onSuccess: (s) => {
      refresh()
      go('/sessions/$sessionId', { sessionId: s.id })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const dictate = useMutation({
    mutationFn: (id: string) => api.startRecording(id, 'dictation', mic.startId),
    onSuccess: refresh,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const stop = useMutation({
    mutationFn: api.stopRecording,
    onSuccess: refresh,
    onError: (e) => toast.error(errorMessage(e)),
  })

  const name = (id: string) => {
    const p = patients.data?.find((x) => x.id === id)
    return p ? `${p.last_name}, ${p.first_name}` : ''
  }
  const recent = [...(sessions.data ?? [])]
    .sort((a, b) => (b.scheduled_at ?? b.started_at ?? '').localeCompare(a.scheduled_at ?? a.started_at ?? ''))
    .slice(0, RECENT_SESSIONS)

  return (
    <CommandDialog
      open={open}
      onOpenChange={(o) => (o ? onOpenChange(true) : close())}
      title={t('palette.title')}
      description={t('palette.description')}
      showCloseButton={false}
    >
      <CommandInput
        value={search}
        onValueChange={setSearch}
        placeholder={mode === 'newSession' ? t('palette.pickPatient') : t('shell.search')}
        onKeyDown={(e) => {
          if (mode !== 'root' && e.key === 'Backspace' && !search) setMode('root')
        }}
      />
      <CommandList>
        <CommandEmpty>{t('palette.empty')}</CommandEmpty>
        {mode === 'newSession' ? (
          <CommandGroup heading={t('palette.newSessionFor')}>
            {patients.data?.map((p) => (
              <CommandItem
                key={p.id}
                value={`${p.last_name} ${p.first_name} ${p.chiffre ?? ''} ${p.id}`}
                disabled={newSession.isPending}
                onSelect={() => newSession.mutate(p.id)}
              >
                <User /> {p.last_name}, {p.first_name}
                {p.chiffre && <span className="ml-auto font-mono text-xs text-muted-foreground">{p.chiffre}</span>}
              </CommandItem>
            ))}
          </CommandGroup>
        ) : (
          <>
            <CommandGroup heading={t('palette.actions')}>
              {recording.data ? (
                <CommandItem value={t('palette.stop')} onSelect={() => (stop.mutate(), close())}>
                  <Square /> {t('palette.stop')}
                </CommandItem>
              ) : (
                sessionId && (
                  <CommandItem value={t('palette.dictate')} onSelect={() => (dictate.mutate(sessionId), close())}>
                    <NotebookPen /> {t('palette.dictate')}
                  </CommandItem>
                )
              )}
              <CommandItem
                value={t('palette.newSession')}
                onSelect={() => {
                  setMode('newSession')
                  setSearch('')
                }}
              >
                <Plus /> {t('palette.newSession')}
              </CommandItem>
            </CommandGroup>
            <CommandSeparator />
            <CommandGroup heading={t('palette.pages')}>
              {pages.map((p) => (
                <CommandItem key={p.to} value={`${t('palette.goTo')} ${t(p.label)}`} onSelect={() => go(p.to)}>
                  <p.icon /> {t(p.label)}
                </CommandItem>
              ))}
            </CommandGroup>
            {!!patients.data?.length && (
              <>
                <CommandSeparator />
                <CommandGroup heading={t('nav.patients')}>
                  {patients.data.map((p) => (
                    <CommandItem
                      key={p.id}
                      value={`${p.last_name} ${p.first_name} ${p.chiffre ?? ''} ${p.id}`}
                      onSelect={() => go('/patients/$patientId', { patientId: p.id })}
                    >
                      <User /> {p.last_name}, {p.first_name}
                      {p.chiffre && <span className="ml-auto font-mono text-xs text-muted-foreground">{p.chiffre}</span>}
                    </CommandItem>
                  ))}
                </CommandGroup>
              </>
            )}
            {recent.length > 0 && (
              <>
                <CommandSeparator />
                <CommandGroup heading={t('palette.sessions')}>
                  {recent.map((s) => {
                    const when = formatDate(s.scheduled_at ?? s.started_at, true)
                    return (
                      <CommandItem
                        key={s.id}
                        value={`${t('note.session')} ${name(s.patient_id)} ${when} ${s.id}`}
                        onSelect={() => go('/sessions/$sessionId', { sessionId: s.id })}
                      >
                        <Mic /> {name(s.patient_id)}
                        <span className="ml-auto text-xs text-muted-foreground">{when}</span>
                      </CommandItem>
                    )
                  })}
                </CommandGroup>
              </>
            )}
          </>
        )}
      </CommandList>
    </CommandDialog>
  )
}
