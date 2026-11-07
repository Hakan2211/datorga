// Stilprofil (slice 52): what Theralo read from her exemplars of one type (sentence length,
// Ich-Form, how she names the person, prose or bullet points, length per section, recurring
// phrases) and her own notes, per type and for all. Both go along with drafts as "Stilprofil".
import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import type { Computed } from '@/bindings/Computed'
import type { ProfileView } from '@/bindings/ProfileView'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { api, errorMessage, q } from '@/lib/api'
import { styleTypes } from '@/lib/documents'
import { ALL_TYPES, profileSummary } from '@/lib/style-profile'

/** "Ø 14 Wörter je Satz · Ich-Form · „Pat.“ · Fließtext" under a type's exemplars. */
export function ProfileLine({ computed, notes }: { computed: Computed | null | undefined; notes: string }) {
  const { t } = useTranslation()
  const parts = profileSummary(computed, t)
  if (!parts.length && !notes) return null
  return (
    <p className="text-xs text-muted-foreground" data-testid="style-profile-line">
      {parts.join(' · ')}
      {notes && `${parts.length ? ' · ' : ''}${t('styleProfile.hasNotes')}`}
    </p>
  )
}

/** Her notes for a type or for all types, with what Theralo computed for the type. */
export function StyleNotesDialog({ templateKey, onClose }: { templateKey: string; onClose: () => void }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const templates = useQuery(q.templates())
  const profiles = useQuery(q.styleProfiles())
  const [key, setKey] = useState(templateKey)
  const profile = profiles.data?.find((p) => p.template_key === key)
  const [notes, setNotes] = useState<string | null>(null)
  const text = notes ?? profile?.notes ?? ''
  const save = useMutation({
    mutationFn: () => api.setStyleNotes(key, text),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['style'] })
      void qc.invalidateQueries({ queryKey: ['draftSources'] })
      void qc.invalidateQueries({ queryKey: ['letter'] })
      toast.success(t('styleProfile.saved'))
      onClose()
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90vh] grid-rows-[auto_minmax(0,1fr)_auto] sm:max-w-2xl" data-testid="style-notes-dialog">
        <DialogHeader>
          <DialogTitle>{t('styleProfile.title')}</DialogTitle>
          <DialogDescription>{t('styleProfile.intro')}</DialogDescription>
        </DialogHeader>
        <div className="min-h-0 space-y-4 overflow-y-auto pr-1 text-sm">
          <div className="space-y-1.5">
            <Label>{t('styleProfile.for')}</Label>
            <Select
              value={key}
              onValueChange={(v) => {
                setKey(v)
                setNotes(null)
              }}
            >
              <SelectTrigger data-testid="style-notes-type">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL_TYPES}>{t('styleProfile.allTypes')}</SelectItem>
                {styleTypes(templates.data ?? []).map((x) => (
                  <SelectItem key={x.key} value={x.key}>
                    {x.title}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {profile?.computed && <ComputedDetails c={profile.computed} />}
          {key !== ALL_TYPES && !profile?.computed && <p className="text-[13px] text-muted-foreground">{t('styleProfile.noExemplars')}</p>}
          <div className="space-y-1.5">
            <Label htmlFor="style-notes">{t('styleProfile.notes')}</Label>
            <Textarea
              id="style-notes"
              rows={5}
              value={text}
              onChange={(e) => setNotes(e.target.value)}
              placeholder={t('styleProfile.notesPlaceholder')}
              data-testid="style-notes"
            />
            <p className="text-[11px] text-muted-foreground">{t('styleProfile.notesHint')}</p>
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button disabled={save.isPending || text.length > 1500} onClick={() => save.mutate()} data-testid="style-notes-save">
            {t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function ComputedDetails({ c }: { c: Computed }) {
  const { t } = useTranslation()
  return (
    <div className="space-y-2 rounded-md border bg-muted/30 p-3 text-[13px]" data-testid="style-computed">
      <p className="font-medium">{t('styleProfile.computed', { count: c.exemplars })}</p>
      <p>{profileSummary(c, t).join(' · ')}</p>
      {c.sections.length > 0 && (
        <p>
          <span className="text-muted-foreground">{t('styleProfile.lengths')}: </span>
          {c.sections.map((s) => `${s.heading || t('letterAi.opening')} ${s.words}`).join(' · ')}
        </p>
      )}
      {c.phrases.length > 0 && (
        <p>
          <span className="text-muted-foreground">{t('styleProfile.phrases')}: </span>
          {c.phrases.map((p) => `„${p}“`).join(', ')}
        </p>
      )}
      <p className="text-[11px] text-muted-foreground">{t('styleProfile.computedHint')}</p>
    </div>
  )
}

export type { ProfileView }
