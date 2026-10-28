import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { save } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import { api, errorMessage, type ReportView } from '@/lib/api'

export type ExportFormat = 'pdf' | 'docx'

/** Asks for a path (Chiffre in the file name, never the name) and exports the signed report there. */
export function useReportExport(view: ReportView | undefined) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  return useMutation({
    mutationFn: async (format: ExportFormat) => {
      if (!view) return false
      const chiffre = view.patient.chiffre ?? 'ohne-Chiffre'
      const day = (view.session?.scheduled_at ?? view.session?.started_at ?? view.report.created_at).slice(0, 10)
      const base = view.session ? `Sitzungsnotiz-${chiffre}-${day}` : `Bericht-${chiffre}-${view.application?.kind ?? 'bericht'}`
      const path = await save({
        defaultPath: `${base}.${format}`,
        filters: [{ name: format.toUpperCase(), extensions: [format] }],
      })
      if (!path) return false
      await api.exportReport(view.report.id, format, path)
      return true
    },
    onSuccess: (done) => {
      if (!done || !view) return
      toast.success(t('report.exported'))
      void qc.invalidateQueries({ queryKey: ['report', view.report.id] })
      void qc.invalidateQueries({ queryKey: ['reports'] })
      void qc.invalidateQueries({ queryKey: ['sessions'] })
      if (view.session) void qc.invalidateQueries({ queryKey: ['session', view.session.id] })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
}
