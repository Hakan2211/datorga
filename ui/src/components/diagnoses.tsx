import { useTranslation } from 'react-i18next'
import { Plus, Trash2 } from 'lucide-react'
import type { Diagnosis } from '@/bindings/Diagnosis'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

const CERTAINTIES = ['G', 'V', 'Z', 'A'] as const

/** ICD-10 rows (code, label, certainty) with add/remove. Therapist input only; no AI path writes here. */
export function DiagnosisRows({
  value,
  onChange,
  readOnly = false,
}: {
  value: Diagnosis[]
  onChange: (next: Diagnosis[]) => void
  readOnly?: boolean
}) {
  const { t } = useTranslation()
  const setRow = (i: number, patch: Partial<Diagnosis>) => onChange(value.map((d, j) => (j === i ? { ...d, ...patch } : d)))

  return (
    <div className="space-y-2">
      {value.map((d, i) => (
        <div key={i} className="grid grid-cols-[7rem_1fr_11rem_auto] gap-2">
          <Input
            className="font-mono"
            placeholder={t('report.code')}
            value={d.code}
            disabled={readOnly}
            onChange={(e) => setRow(i, { code: e.target.value.toUpperCase() })}
          />
          <Input placeholder={t('report.label')} value={d.label} disabled={readOnly} onChange={(e) => setRow(i, { label: e.target.value })} />
          <Select value={d.certainty} disabled={readOnly} onValueChange={(v) => setRow(i, { certainty: v })}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {CERTAINTIES.map((c) => (
                <SelectItem key={c} value={c}>
                  {t(`enums.certainty.${c}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            variant="ghost"
            size="icon"
            disabled={readOnly}
            aria-label={t('common.delete')}
            onClick={() => onChange(value.filter((_, j) => j !== i))}
          >
            <Trash2 />
          </Button>
        </div>
      ))}
      {!readOnly && (
        <Button variant="outline" size="sm" onClick={() => onChange([...value, { code: '', label: '', certainty: 'G' }])}>
          <Plus /> {t('report.addDiagnosis')}
        </Button>
      )}
    </div>
  )
}
