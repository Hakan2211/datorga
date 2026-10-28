import type { Editor } from '@tiptap/react'
import { useEditorState } from '@tiptap/react'
import { useTranslation } from 'react-i18next'
import {
  AlignCenter,
  AlignJustify,
  AlignLeft,
  AlignRight,
  Bold,
  Heading3,
  Italic,
  List,
  ListOrdered,
  Redo2,
  Underline,
  Undo2,
} from 'lucide-react'
import { cn } from '@/lib/utils'

const ALIGNMENTS = [
  ['justify', AlignJustify],
  ['left', AlignLeft],
  ['center', AlignCenter],
  ['right', AlignRight],
] as const

/** Formatting for both editors; offers only what the PDF/DOCX export can show. */
export function EditorToolbar({ editor, className }: { editor: Editor; className?: string }) {
  const { t } = useTranslation()
  const s = useEditorState({
    editor,
    selector: ({ editor }) => ({
      bold: editor.isActive('bold'),
      italic: editor.isActive('italic'),
      underline: editor.isActive('underline'),
      heading: editor.isActive('heading'),
      bullet: editor.isActive('bulletList'),
      ordered: editor.isActive('orderedList'),
      // Inside a list alignment isn't exported.
      inList: editor.isActive('listItem'),
      align: ALIGNMENTS.find(([a]) => a !== 'justify' && editor.isActive({ textAlign: a }))?.[0] ?? 'justify',
      undo: editor.can().undo(),
      redo: editor.can().redo(),
    }),
  })
  const chain = () => editor.chain().focus()
  return (
    <div className={cn('flex flex-wrap items-center gap-0.5 border-b px-1.5 py-1', className)}>
      <ToolButton disabled={!s.undo} onClick={() => chain().undo().run()} label={t('editor.undo')}>
        <Undo2 className="size-3.5" />
      </ToolButton>
      <ToolButton disabled={!s.redo} onClick={() => chain().redo().run()} label={t('editor.redo')}>
        <Redo2 className="size-3.5" />
      </ToolButton>
      <Divider />
      <ToolButton
        active={s.heading}
        disabled={s.inList}
        onClick={() => chain().toggleHeading({ level: 3 }).run()}
        label={t('editor.heading')}
      >
        <Heading3 className="size-3.5" />
      </ToolButton>
      <ToolButton active={s.bold} onClick={() => chain().toggleBold().run()} label={t('editor.bold')}>
        <Bold className="size-3.5" />
      </ToolButton>
      <ToolButton active={s.italic} onClick={() => chain().toggleItalic().run()} label={t('editor.italic')}>
        <Italic className="size-3.5" />
      </ToolButton>
      <ToolButton active={s.underline} onClick={() => chain().toggleUnderline().run()} label={t('editor.underline')}>
        <Underline className="size-3.5" />
      </ToolButton>
      <Divider />
      <ToolButton active={s.bullet} onClick={() => chain().toggleBulletList().run()} label={t('editor.bullets')}>
        <List className="size-3.5" />
      </ToolButton>
      <ToolButton active={s.ordered} onClick={() => chain().toggleOrderedList().run()} label={t('editor.numbers')}>
        <ListOrdered className="size-3.5" />
      </ToolButton>
      <Divider />
      {ALIGNMENTS.map(([a, Icon]) => (
        <ToolButton
          key={a}
          active={!s.inList && s.align === a}
          disabled={s.inList}
          onClick={() => (a === 'justify' ? chain().unsetTextAlign().run() : chain().setTextAlign(a).run())}
          label={t(`editor.align.${a}`)}
        >
          <Icon className="size-3.5" />
        </ToolButton>
      ))}
    </div>
  )
}

function Divider() {
  return <span className="mx-1 h-4 w-px bg-border" />
}

function ToolButton({
  active,
  disabled,
  onClick,
  label,
  children,
}: {
  active?: boolean
  disabled?: boolean
  onClick: () => void
  label: string
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      disabled={disabled}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
      className={cn(
        'rounded p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-40',
        active && 'bg-accent text-accent-foreground',
      )}
    >
      {children}
    </button>
  )
}
