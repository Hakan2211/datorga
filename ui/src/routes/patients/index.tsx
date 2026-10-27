import { useState } from 'react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Plus, Search } from 'lucide-react'
import { Page, PageHeader } from '@/components/app-shell'
import { formatDate } from '@/components/bits'
import { PatientFormDialog } from '@/components/patient-form'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { q } from '@/lib/api'

export const Route = createFileRoute('/patients/')({ component: Patients })

function Patients() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const [query, setQuery] = useState('')
  const patients = useQuery(q.patients(query))

  return (
    <>
      <PageHeader
        title={t('patients.title')}
        actions={
          <PatientFormDialog
            trigger={
              <Button>
                <Plus /> {t('patients.new')}
              </Button>
            }
            onSaved={(p) => navigate({ to: '/patients/$patientId', params: { patientId: p.id } })}
          />
        }
      />
      <Page>
        <div className="relative max-w-sm">
          <Search className="absolute top-2.5 left-3 size-4 text-muted-foreground" />
          <Input className="pl-9" placeholder={t('common.search')} value={query} onChange={(e) => setQuery(e.target.value)} />
        </div>
        <Card>
          <CardContent className="pt-2">
            {patients.data?.length === 0 ? (
              <p className="py-6 text-sm text-muted-foreground">{t('common.empty')}</p>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t('patients.lastName')}, {t('patients.firstName')}</TableHead>
                    <TableHead>{t('patients.birthDate')}</TableHead>
                    <TableHead>{t('patients.chiffre')}</TableHead>
                    <TableHead>{t('patients.externalRef')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {patients.data?.map((p) => (
                    <TableRow key={p.id}>
                      <TableCell className="font-medium">
                        <Link to="/patients/$patientId" params={{ patientId: p.id }} className="hover:underline">
                          {p.last_name}, {p.first_name}
                        </Link>
                      </TableCell>
                      <TableCell>{formatDate(p.birth_date)}</TableCell>
                      <TableCell className="font-mono text-xs">{p.chiffre ?? '—'}</TableCell>
                      <TableCell>{p.external_ref ?? '—'}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </CardContent>
        </Card>
      </Page>
    </>
  )
}
