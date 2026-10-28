// Diktat-Werkstatt for a Bericht (or a Sitzungsnotiz opened from its editor).
import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Workshop } from '@/components/workshop'
import { q } from '@/lib/api'

export const Route = createFileRoute('/reports/$reportId_/werkstatt')({ component: ReportWorkshopPage })

function ReportWorkshopPage() {
  const { reportId } = Route.useParams()
  const { t } = useTranslation()
  const view = useQuery(q.report(reportId))
  if (!view.data) return null
  const { report, template, application } = view.data
  const title =
    report.kind === 'sitzungsnotiz'
      ? template.title
      : application
        ? `${t(`enums.kind.${application.kind}`)}${application.kind === 'fortfuehrung' ? ` Nr. ${application.seq}` : ''}`
        : template.title
  return <Workshop view={view.data} title={title} />
}
