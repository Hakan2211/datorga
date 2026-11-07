import { useState } from 'react'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { BookUser, Pencil, Plus, Search } from 'lucide-react'
import { Page, PageHeader } from '@/components/app-shell'
import { EmptyState } from '@/components/bits'
import { RecipientFormDialog } from '@/components/briefe/recipient-form'
import { DeleteButton } from '@/components/delete-button'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { api, q } from '@/lib/api'

export const Route = createFileRoute('/briefe/empfaenger')({ component: AddressBook })

function AddressBook() {
  const { t } = useTranslation()
  const [query, setQuery] = useState('')
  const recipients = useQuery(q.recipients(query))
  return (
    <>
      <PageHeader
        title={t('briefe.recipients.title')}
        subtitle={
          <span>
            <Link to="/briefe" className="hover:underline">
              {t('briefe.title')}
            </Link>{' '}
            · {t('briefe.recipients.subtitle')}
          </span>
        }
        actions={
          <RecipientFormDialog
            trigger={
              <Button>
                <Plus /> {t('briefe.recipients.new')}
              </Button>
            }
          />
        }
      />
      <Page>
        <div className="relative w-80">
          <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t('briefe.recipients.search')} className="pl-9" />
        </div>
        {recipients.data?.length === 0 && !query ? (
          <EmptyState icon={BookUser} title={t('briefe.recipients.emptyTitle')} description={t('briefe.recipients.emptyBody')} />
        ) : (
          <div className="rounded-lg border bg-card">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t('briefe.name')}</TableHead>
                  <TableHead>{t('briefe.kind')}</TableHead>
                  <TableHead>{t('briefe.addressLines')}</TableHead>
                  <TableHead className="text-right">{t('briefe.title')}</TableHead>
                  <TableHead className="w-24" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {recipients.data?.map((r) => (
                  <TableRow key={r.id} data-testid="recipient-row">
                    <TableCell>
                      <span className="flex flex-col leading-tight">
                        <span className="font-medium">{r.name}</span>
                        {r.department && <span className="text-xs text-muted-foreground">{r.department}</span>}
                      </span>
                    </TableCell>
                    <TableCell>{t(`enums.recipient_kind.${r.kind}`)}</TableCell>
                    <TableCell className="text-sm text-muted-foreground">{r.lines.join(', ')}</TableCell>
                    <TableCell className="text-right tabular-nums">{t('briefe.recipients.uses', { count: r.use_count })}</TableCell>
                    <TableCell>
                      <div className="flex justify-end">
                        <RecipientFormDialog
                          recipient={r}
                          trigger={
                            <Button variant="ghost" size="icon" aria-label={t('briefe.recipients.edit')}>
                              <Pencil />
                            </Button>
                          }
                        />
                        <DeleteButton
                          title={t('briefe.recipients.delete')}
                          description={t('briefe.recipients.deleteText')}
                          action={() => api.deleteRecipient(r.id)}
                        />
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </Page>
    </>
  )
}
