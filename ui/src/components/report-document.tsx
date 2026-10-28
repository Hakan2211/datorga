import { createContext, useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { EditorContent, NodeViewWrapper, ReactNodeViewRenderer, useEditor, type NodeViewProps } from '@tiptap/react'
import { createNodeFromContent } from '@tiptap/core'
import type { Fragment } from '@tiptap/pm/model'
import { useTranslation } from 'react-i18next'
import { Pencil } from 'lucide-react'
import type { SectionDef } from '@/bindings/SectionDef'
import { EditorToolbar } from '@/components/editor-toolbar'
import { Button } from '@/components/ui/button'
import { ReportLocked, documentExtensions, documentHtml, sectionBodies, sectionPos } from '@/lib/editor'
import { isBlank, parseDiagnoses, plainText } from '@/lib/report'

/** What the page shows around the sections, as on the exported first page. */
export interface DocumentFrame {
  header: string
  practice: string | null
  title: string
  subtitle: string
  date: string
  signedBy: string | null
}

const PAGE_WIDTH_PX = 794 // 21 cm at 96 dpi

interface LockedCtx {
  defs: Record<string, SectionDef>
  bodies: Record<string, string>
  onEditLocked: (key: string) => void
}
const Locked = createContext<LockedCtx>({
  defs: {},
  bodies: {},
  onEditLocked: () => {},
})

/**
 * The whole report in one Word-like editor on an A4 page. Each template section is one node
 * with a fixed heading; edits are mapped back to the section bodies, so storage, drafting,
 * citations and sign-off work as in the section view.
 */
export function ReportDocument({
  sections,
  bodies,
  readOnly,
  frame,
  onChange,
  onActive,
  onEditLocked,
  dictateFor,
}: {
  sections: SectionDef[]
  bodies: Record<string, string>
  readOnly: boolean
  frame: DocumentFrame
  onChange: (key: string, body: string) => void
  onActive: (key: string) => void
  onEditLocked: (key: string) => void
  /** Report id: offers voice mode (dictation at the cursor). */
  dictateFor?: string
}) {
  const { t } = useTranslation()
  const callbacks = useRef({ onChange, onActive })
  callbacks.current = { onChange, onActive }
  // The bodies as the editor last saw them: own edits are not loaded back in.
  const known = useRef<Record<string, string>>({ ...bodies })

  const editor = useEditor({
    extensions: [
      ...documentExtensions(sections, t('report.placeholder')),
      ReportLocked.extend({
        addNodeView: () => ReactNodeViewRenderer(LockedSection),
      }),
    ],
    content: documentHtml(sections, bodies),
    editable: !readOnly,
    immediatelyRender: false,
    editorProps: {
      attributes: { spellcheck: 'true', lang: 'de', class: 'doc-body' },
    },
    onUpdate: ({ editor, transaction }) => {
      if (transaction.getMeta('external')) return
      for (const [key, html] of Object.entries(sectionBodies(editor.state.doc, editor.schema))) {
        if (html !== (known.current[key] ?? '')) {
          known.current[key] = html
          callbacks.current.onChange(key, html)
        }
      }
    },
    onSelectionUpdate: ({ editor }) => {
      const { $from } = editor.state.selection
      if ($from.depth >= 1) callbacks.current.onActive(String($from.node(1).attrs.key))
    },
  })

  useEffect(() => {
    editor?.setEditable(!readOnly)
  }, [editor, readOnly])

  // Text written from outside (AI draft applied, transcript inserted) replaces that section only.
  useEffect(() => {
    if (!editor) return
    const tr = editor.state.tr
    for (const s of sections) {
      const body = bodies[s.key] ?? ''
      if (s.field !== 'prose' || body === (known.current[s.key] ?? '')) continue
      const pos = sectionPos(tr.doc, s.key)
      if (pos) tr.replaceWith(pos.from, pos.to, createNodeFromContent(body || '<p></p>', editor.schema) as Fragment)
    }
    known.current = { ...bodies }
    if (tr.docChanged) editor.view.dispatch(tr.setMeta('external', true).setMeta('addToHistory', false))
  }, [editor, bodies, sections])

  // The page keeps its A4 proportions and shrinks to fit narrow columns (e.g. next to the preview).
  const box = useRef<HTMLDivElement>(null)
  const [zoom, setZoom] = useState(1)
  useLayoutEffect(() => {
    const el = box.current
    if (!el) return
    const fit = () => setZoom(Math.min(1, (el.clientWidth - 32) / PAGE_WIDTH_PX))
    fit()
    const ro = new ResizeObserver(fit)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const defs = Object.fromEntries(sections.map((s) => [s.key, s]))
  return (
    <div ref={box} className="min-w-0">
      {!readOnly && editor && (
        <EditorToolbar
          editor={editor}
          dictateFor={dictateFor}
          className="sticky top-0 z-10 mb-4 justify-center rounded-md border bg-card shadow-xs"
        />
      )}
      <Locked.Provider value={{ defs, bodies, onEditLocked }}>
        <div className="doc-page mx-auto" style={{ zoom }} data-testid="doc-page">
          <div className="doc-header">
            <span>{frame.header}</span>
          </div>
          {frame.practice && <p className="doc-practice">{frame.practice}</p>}
          <h1 className="doc-title">{frame.title}</h1>
          <p className="doc-subtitle">{frame.subtitle}</p>
          <EditorContent editor={editor} />
          <div className="doc-signature">
            <Line label={`${t('report.date')}: ${frame.date}`} />
            <Line label={`${t('report.signature')}${frame.signedBy ? `: ${frame.signedBy}` : ''}`} />
          </div>
        </div>
      </Locked.Provider>
    </div>
  )
}

function Line({ label }: { label: ReactNode }) {
  return (
    <div>
      <div className="border-t border-current/60" />
      <p className="mt-1 text-[9pt]">{label}</p>
    </div>
  )
}

/** The diagnoses table as it prints; edited only in the section view (MDR rule: never by AI or free text). */
function LockedSection({ node }: NodeViewProps) {
  const { t } = useTranslation()
  const { defs, bodies, onEditLocked } = useContext(Locked)
  const key = String(node.attrs.key)
  const def = defs[key]
  const d = parseDiagnoses(bodies[key] ?? '')
  return (
    <NodeViewWrapper className="doc-section group/locked" contentEditable={false} id={`section-${key}`}>
      <div className="flex items-baseline justify-between gap-2">
        <h2 className="doc-section-heading">{def?.number ? `${def.number}. ${def.heading}` : def?.heading}</h2>
        <Button
          variant="ghost"
          size="sm"
          className="h-6 font-sans text-xs opacity-0 transition-opacity group-hover/locked:opacity-100 focus-visible:opacity-100"
          onClick={() => onEditLocked(key)}
        >
          <Pencil className="size-3" /> {t('report.editInSections')}
        </Button>
      </div>
      {d.diagnoses.length > 0 ? (
        <table className="doc-table">
          <thead>
            <tr>
              <th>ICD-10</th>
              <th>Diagnose</th>
              <th>Sicherheit</th>
            </tr>
          </thead>
          <tbody>
            {d.diagnoses.map((x, i) => (
              <tr key={i}>
                <td>{x.code}</td>
                <td>{x.label}</td>
                <td>{x.certainty}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <p className="font-sans text-xs text-neutral-500 italic">{t('report.noDiagnoses')}</p>
      )}
      {!isBlank(d.notes) && <p className="doc-notes">{plainText(d.notes)}</p>}
    </NodeViewWrapper>
  )
}
