import { useEffect, useRef } from 'react'
import { EditorContent, useEditor, useEditorState } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import { Placeholder } from '@tiptap/extension-placeholder'
import { Bold, Italic, List, ListOrdered } from 'lucide-react'
import { cn } from '@/lib/utils'

/** One TipTap field per report section. German spellcheck comes from the webview (lang="de"). */
export function RichField({
  value,
  onChange,
  placeholder,
  readOnly,
}: {
  value: string
  onChange: (html: string) => void
  placeholder?: string
  readOnly?: boolean
}) {
  const onChangeRef = useRef(onChange)
  onChangeRef.current = onChange

  const editor = useEditor({
    extensions: [
      StarterKit.configure({ heading: false, codeBlock: false, code: false, blockquote: false, horizontalRule: false }),
      Placeholder.configure({ placeholder }),
    ],
    content: value,
    editable: !readOnly,
    immediatelyRender: false,
    editorProps: { attributes: { spellcheck: 'true', lang: 'de' } },
    onUpdate: ({ editor }) => onChangeRef.current(editor.isEmpty ? '' : editor.getHTML()),
  })

  useEffect(() => {
    editor?.setEditable(!readOnly)
  }, [editor, readOnly])

  const state = useEditorState({
    editor,
    selector: ({ editor }) => ({
      bold: editor?.isActive('bold') ?? false,
      italic: editor?.isActive('italic') ?? false,
      bullet: editor?.isActive('bulletList') ?? false,
      ordered: editor?.isActive('orderedList') ?? false,
    }),
  })

  return (
    <div
      className={cn(
        'prose-editor rounded-md border bg-card text-[15px] transition-colors focus-within:border-ring',
        readOnly && 'bg-muted/40',
      )}
    >
      {!readOnly && editor && (
        <div className="flex gap-0.5 border-b px-1.5 py-1">
          <ToolButton active={state?.bold} onClick={() => editor.chain().focus().toggleBold().run()} label="Fett">
            <Bold className="size-3.5" />
          </ToolButton>
          <ToolButton active={state?.italic} onClick={() => editor.chain().focus().toggleItalic().run()} label="Kursiv">
            <Italic className="size-3.5" />
          </ToolButton>
          <ToolButton active={state?.bullet} onClick={() => editor.chain().focus().toggleBulletList().run()} label="Aufzählung">
            <List className="size-3.5" />
          </ToolButton>
          <ToolButton
            active={state?.ordered}
            onClick={() => editor.chain().focus().toggleOrderedList().run()}
            label="Nummerierung"
          >
            <ListOrdered className="size-3.5" />
          </ToolButton>
        </div>
      )}
      <EditorContent editor={editor} className="px-3 py-2" />
    </div>
  )
}

function ToolButton({
  active,
  onClick,
  label,
  children,
}: {
  active?: boolean
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
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
      className={cn(
        'rounded p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground',
        active && 'bg-accent text-accent-foreground',
      )}
    >
      {children}
    </button>
  )
}
