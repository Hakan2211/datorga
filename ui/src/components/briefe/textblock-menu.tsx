import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import type { Editor } from '@tiptap/react'
import { useEditorState } from '@tiptap/react'
import { Link } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { BookmarkPlus, Settings2, TextQuote } from 'lucide-react'
import type { TextBlock } from '@/bindings/TextBlock'
import { TextBlockFormDialog } from '@/components/briefe/textblock-form'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { api, errorMessage, q } from '@/lib/api'
import { insertBlock, selectionHtml, slashQuery, type SlashQuery } from '@/lib/editor'
import { blockSnippet, orderBlocks, toPlaceholders } from '@/lib/letters'
import { cn } from '@/lib/utils'

/** The section the cursor is in (document editor). */
const sectionAt = (editor: Editor): string | null => {
  const { $from } = editor.state.selection
  return $from.depth >= 1 ? String($from.node(1).attrs.key ?? '') || null : null
}

/**
 * Textbausteine in the letter editor: a toolbar button with a searchable list, "/" in the text
 * for the same list at the cursor, and "Als Textbaustein speichern" for the selected text.
 * Placeholders are filled by the backend from the letter's patient.
 */
export function TextBlockTools({
  editor,
  reportId,
  templateKey,
  patient,
}: {
  editor: Editor
  reportId: string
  templateKey: string
  /** The letter's patient: their name and birth date in saved text become placeholders. */
  patient: { first_name: string; last_name: string; birth_date: string | null }
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const blocks = useQuery(q.textblocks(templateKey))
  const [open, setOpen] = useState(false)
  const [saving, setSaving] = useState<{ body: string; section: string | null; replaced: boolean } | null>(null)
  const s = useEditorState({ editor, selector: ({ editor }) => ({ selected: !editor.state.selection.empty, section: sectionAt(editor) }) })

  const insert = async (b: TextBlock) => {
    try {
      const html = await api.insertTextblock(b.id, reportId)
      insertBlock(editor, html, slashQuery(editor.state))
      void qc.invalidateQueries({ queryKey: ['textblocks'] })
    } catch (e) {
      toast.error(errorMessage(e))
    }
  }
  const ordered = orderBlocks(blocks.data ?? [], templateKey, s.section)

  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            type="button"
            title={t('textblocks.insertHint')}
            onMouseDown={(e) => e.preventDefault()}
            className="flex items-center gap-1 rounded px-1.5 py-1 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
            data-testid="textblock-insert"
          >
            <TextQuote className="size-3.5" /> {t('textblocks.insert')}
          </button>
        </PopoverTrigger>
        <PopoverContent className="w-96 p-0" align="start" onCloseAutoFocus={(e) => e.preventDefault()}>
          <Command>
            <CommandInput placeholder={t('textblocks.search')} />
            <CommandList>
              <CommandEmpty>{t('textblocks.none')}</CommandEmpty>
              {[
                ['section', ordered.section],
                ['rest', ordered.rest],
              ].map(([group, list]) =>
                (list as TextBlock[]).length ? (
                  <CommandGroup key={group as string} heading={group === 'section' ? t('textblocks.forSection') : t('textblocks.forType')}>
                    {(list as TextBlock[]).map((b) => (
                      <CommandItem
                        key={b.id}
                        value={`${b.title} ${b.id}`}
                        onSelect={() => {
                          setOpen(false)
                          void insert(b)
                        }}
                        data-testid="textblock-option"
                      >
                        <BlockRow block={b} />
                      </CommandItem>
                    ))}
                  </CommandGroup>
                ) : null,
              )}
            </CommandList>
            <Link
              to="/briefe/textbausteine"
              className="flex items-center gap-1.5 border-t px-3 py-2 text-xs text-muted-foreground hover:text-foreground"
            >
              <Settings2 className="size-3.5" /> {t('textblocks.manage')}
            </Link>
          </Command>
        </PopoverContent>
      </Popover>
      <button
        type="button"
        title={t('textblocks.saveSelectionHint')}
        disabled={!s.selected}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => {
          const raw = selectionHtml(editor)
          const { html, replaced } = toPlaceholders(raw.startsWith('<') ? raw : `<p>${raw}</p>`, patient)
          setSaving({ body: html, section: s.section, replaced })
        }}
        className="flex items-center gap-1 rounded px-1.5 py-1 text-xs text-muted-foreground hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-40"
        data-testid="textblock-save-selection"
      >
        <BookmarkPlus className="size-3.5" /> {t('textblocks.saveSelection')}
      </button>
      {saving && (
        <TextBlockFormDialog
          open
          onOpenChange={(o) => !o && setSaving(null)}
          initial={{ body: saving.body, template_keys: [templateKey], section_key: saving.section }}
          note={saving.replaced ? t('textblocks.namesReplaced') : undefined}
          onSaved={() => setSaving(null)}
        />
      )}
      <SlashMenu editor={editor} blocks={blocks.data ?? []} templateKey={templateKey} onPick={(b) => void insert(b)} />
    </>
  )
}

function BlockRow({ block }: { block: TextBlock }) {
  return (
    <span className="flex min-w-0 flex-col leading-tight">
      <span className="truncate">{block.title}</span>
      <span className="truncate text-xs text-muted-foreground">{blockSnippet(block.body, 90)}</span>
    </span>
  )
}

/** The list at the cursor after "/" (search by title); arrows, Enter/Tab, Escape. */
function SlashMenu({
  editor,
  blocks,
  templateKey,
  onPick,
}: {
  editor: Editor
  blocks: TextBlock[]
  templateKey: string
  onPick: (b: TextBlock) => void
}) {
  const { t } = useTranslation()
  const [slash, setSlash] = useState<SlashQuery | null>(null)
  const [index, setIndex] = useState(0)
  // Escape closes the menu for this "/" until a new one is typed.
  const dismissed = useRef<number | null>(null)

  useEffect(() => {
    const update = () => {
      const s = editor.isEditable && editor.isFocused ? slashQuery(editor.state) : null
      if (s && dismissed.current === s.from) return setSlash(null)
      if (!s) dismissed.current = null
      setSlash((old) => (old?.from === s?.from && old?.query === s?.query ? old : s))
    }
    editor.on('transaction', update)
    editor.on('blur', update)
    return () => {
      editor.off('transaction', update)
      editor.off('blur', update)
    }
  }, [editor])

  const { section, rest } = orderBlocks(blocks, templateKey, slash ? sectionAt(editor) : null)
  const needle = slash?.query.trim().toLowerCase() ?? ''
  const items = [...section, ...rest].filter((b) => !needle || b.title.toLowerCase().includes(needle)).slice(0, 8)
  const shown = !!slash && (items.length > 0 || needle.length > 0)

  useEffect(() => setIndex(0), [slash?.from, needle])

  // While open, the menu takes the keys before the editor does.
  const state = useRef({ shown, items, index })
  state.current = { shown, items, index }
  useEffect(() => {
    const storage = (editor.storage as unknown as Record<string, { onKey: ((e: KeyboardEvent) => boolean) | null } | undefined>).slashKeys
    if (!storage) return
    storage.onKey = (e) => {
      const { shown, items, index } = state.current
      if (!shown) return false
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        const n = items.length || 1
        setIndex((i) => (i + (e.key === 'ArrowDown' ? 1 : n - 1)) % n)
        return true
      }
      if ((e.key === 'Enter' || e.key === 'Tab') && items[index]) {
        onPick(items[index])
        return true
      }
      if (e.key === 'Escape') {
        dismissed.current = slashQuery(editor.state)?.from ?? null
        setSlash(null)
        return true
      }
      return false
    }
    return () => {
      storage.onKey = null
    }
  }, [editor, onPick])

  if (!shown || !slash) return null
  let coords: { left: number; bottom: number }
  try {
    coords = editor.view.coordsAtPos(slash.from)
  } catch {
    return null
  }
  return createPortal(
    <div
      className="fixed z-50 w-80 rounded-md border bg-popover p-1 text-popover-foreground shadow-md"
      style={{ left: coords.left, top: coords.bottom + 4 }}
      data-testid="textblock-slash-menu"
      onMouseDown={(e) => e.preventDefault()}
    >
      <p className="px-2 py-1 text-[11px] text-muted-foreground">{t('textblocks.slashTitle')}</p>
      {items.length === 0 ? (
        <p className="px-2 py-1.5 text-sm text-muted-foreground">{t('textblocks.none')}</p>
      ) : (
        items.map((b, i) => (
          <button
            key={b.id}
            type="button"
            className={cn('flex w-full rounded-sm px-2 py-1.5 text-left text-sm', i === index ? 'bg-accent text-accent-foreground' : 'hover:bg-muted')}
            onMouseEnter={() => setIndex(i)}
            onClick={() => onPick(b)}
            data-testid="textblock-slash-option"
          >
            <BlockRow block={b} />
          </button>
        ))
      )}
    </div>,
    document.body,
  )
}
