import { useEffect, useRef } from 'react'
import { EditorContent, useEditor } from '@tiptap/react'
import { EditorToolbar } from '@/components/editor-toolbar'
import { richExtensions } from '@/lib/editor'
import { cn } from '@/lib/utils'

/** One TipTap field per report section. German spellcheck comes from the webview (lang="de"). */
export function RichField({
  value,
  onChange,
  placeholder,
  readOnly,
  dictateFor,
}: {
  value: string
  onChange: (html: string) => void
  placeholder?: string
  readOnly?: boolean
  /** Report id: offers voice mode (dictation into this field). */
  dictateFor?: string
}) {
  const onChangeRef = useRef(onChange)
  onChangeRef.current = onChange

  const editor = useEditor({
    extensions: richExtensions({ placeholder }),
    content: value,
    editable: !readOnly,
    immediatelyRender: false,
    editorProps: { attributes: { spellcheck: 'true', lang: 'de' } },
    onUpdate: ({ editor }) => onChangeRef.current(editor.isEmpty ? '' : editor.getHTML()),
  })

  // No update event: it would save the (unchanged) text into a report that was just signed.
  useEffect(() => {
    editor?.setEditable(!readOnly, false)
  }, [editor, readOnly])

  // Text inserted from outside (e.g. from a transcript). Own edits already match `value`.
  useEffect(() => {
    if (editor && value !== (editor.isEmpty ? '' : editor.getHTML())) {
      editor.commands.setContent(value, { emitUpdate: false })
    }
  }, [editor, value])

  return (
    <div
      className={cn(
        'prose-editor rounded-md border bg-card text-[15px] transition-colors focus-within:border-ring',
        readOnly && 'bg-muted/40',
      )}
    >
      {!readOnly && editor && <EditorToolbar editor={editor} dictateFor={dictateFor} />}
      <EditorContent editor={editor} className="px-3 py-2" />
    </div>
  )
}
