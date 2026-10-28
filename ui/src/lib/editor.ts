import { Extension, Node, getHTMLFromFragment, mergeAttributes, type Extensions } from '@tiptap/core'
import type { Node as PMNode, ResolvedPos, Schema } from '@tiptap/pm/model'
import { Plugin, TextSelection } from '@tiptap/pm/state'
import StarterKit from '@tiptap/starter-kit'
import { Placeholder } from '@tiptap/extension-placeholder'
import { TextAlign } from '@tiptap/extension-text-align'
import type { SectionDef } from '@/bindings/SectionDef'

/**
 * Formatting both editors offer. Only what `render/richtext.rs` turns into PDF and DOCX:
 * paragraphs, one sub-heading level, lists, bold/italic/underline, alignment. Justified is the
 * default (no style attribute), like the export.
 */
export function richExtensions({ document = true, placeholder }: { document?: boolean; placeholder?: string } = {}): Extensions {
  return [
    StarterKit.configure({
      document: document ? undefined : false,
      heading: { levels: [3] },
      codeBlock: false,
      code: false,
      blockquote: false,
      horizontalRule: false,
      strike: false,
      link: false,
    }),
    TextAlign.configure({
      types: ['heading', 'paragraph'],
      alignments: ['left', 'center', 'right', 'justify'],
    }),
    Placeholder.configure({ placeholder, includeChildren: true }),
  ]
}

// ---------- document mode: the whole report in one editor ----------

/** The report's top node: one part per template section, in template order. */
const ReportDoc = Node.create({
  name: 'doc',
  topNode: true,
  content: 'reportPart+',
})

/** A prose section. The heading comes from the template and is not part of the content, so it can't be edited or deleted. */
export const ReportSection = Node.create<{ labels: Record<string, string> }>({
  name: 'reportSection',
  group: 'reportPart',
  content: 'block+',
  isolating: true,
  defining: true,
  selectable: false,
  addOptions: () => ({ labels: {} }),
  addAttributes: () => ({
    key: {
      default: '',
      parseHTML: (el) => el.getAttribute('data-key'),
      renderHTML: (a) => ({ 'data-key': a.key }),
    },
  }),
  parseHTML: () => [{ tag: 'section[data-key]', contentElement: 'div[data-body]' }],
  renderHTML({ node, HTMLAttributes }) {
    return [
      'section',
      mergeAttributes(HTMLAttributes, {
        class: 'doc-section',
        id: `section-${node.attrs.key}`,
      }),
      ['h2', { contenteditable: 'false', class: 'doc-section-heading' }, this.options.labels[node.attrs.key] ?? node.attrs.key],
      ['div', { 'data-body': '' }, 0],
    ]
  },
})

/** A section edited elsewhere (diagnoses: never in free text). Shown read-only by a node view. */
export const ReportLocked = Node.create({
  name: 'reportLocked',
  group: 'reportPart',
  atom: true,
  selectable: false,
  addAttributes: () => ({
    key: {
      default: '',
      parseHTML: (el) => el.getAttribute('data-locked'),
      renderHTML: (a) => ({ 'data-locked': a.key }),
    },
  }),
  parseHTML: () => [{ tag: 'div[data-locked]' }],
  renderHTML: ({ HTMLAttributes }) => ['div', HTMLAttributes],
})

/**
 * Sections stay put: a selection never spans two sections (Strg+A selects the current one, a
 * drag across sections stops at the first), so typing or deleting can't merge or drop them.
 * A change that would still add, drop or reorder sections is rejected.
 */
const KeepSections = Extension.create({
  name: 'keepSections',
  addKeyboardShortcuts: () => ({
    'Mod-a': ({ editor }) => {
      const { $from } = editor.state.selection
      if ($from.depth < 1) return false
      const doc = editor.state.doc
      editor.view.dispatch(editor.state.tr.setSelection(TextSelection.between(doc.resolve($from.start(1)), doc.resolve($from.end(1)))))
      return true
    },
  }),
  addProseMirrorPlugins: () => [
    new Plugin({
      filterTransaction: (tr, state) => !tr.docChanged || partKeys(tr.doc).join() === partKeys(state.doc).join(),
      appendTransaction: (_trs, old, state) => {
        const { $anchor, $head } = state.selection
        if ($anchor.depth >= 1 && $head.depth >= 1 && $anchor.before(1) === $head.before(1)) return null
        const doc = state.doc
        const bounds = ($p: ResolvedPos) => [doc.resolve($p.start(1)), doc.resolve($p.end(1))] as const
        if ($anchor.depth >= 1) {
          // Keep the end the selection started from and stop at its section's edge.
          const [start, end] = bounds($anchor)
          return state.tr.setSelection(TextSelection.between($anchor, $head.pos < $anchor.pos ? start : end))
        }
        // No end inside a section (e.g. select all): the section the cursor was in.
        const $was = old.doc.eq(doc) ? doc.resolve(old.selection.from) : null
        if (!$was || $was.depth < 1) return null
        return state.tr.setSelection(TextSelection.between(...bounds($was)))
      },
    }),
  ],
})

export function documentExtensions(sections: SectionDef[], placeholder: string): Extensions {
  const labels = Object.fromEntries(sections.map((s) => [s.key, s.number ? `${s.number}. ${s.heading}` : s.heading]))
  return [...richExtensions({ document: false, placeholder }), ReportDoc, ReportSection.configure({ labels }), KeepSections]
}

const partKeys = (doc: PMNode) => {
  const keys: string[] = []
  doc.forEach((n) => keys.push(String(n.attrs.key)))
  return keys
}

/** Initial content: every section's stored HTML wrapped in its section node. */
export function documentHtml(sections: SectionDef[], bodies: Record<string, string>): string {
  return sections
    .map((s) =>
      s.field === 'prose'
        ? `<section data-key="${s.key}"><div data-body>${bodies[s.key] || '<p></p>'}</div></section>`
        : `<div data-locked="${s.key}"></div>`,
    )
    .join('')
}

/** Section bodies as the section editor would store them ('' when empty). */
export function sectionBodies(doc: PMNode, schema: Schema): Record<string, string> {
  const out: Record<string, string> = {}
  doc.forEach((node) => {
    if (node.type.name !== 'reportSection') return
    const empty = node.childCount === 1 && node.firstChild!.type.name === 'paragraph' && node.firstChild!.content.size === 0
    out[node.attrs.key as string] = empty ? '' : getHTMLFromFragment(node.content, schema)
  })
  return out
}

/** Position of a section node, for replacing its content from outside (AI draft, transcript). */
export function sectionPos(doc: PMNode, key: string): { from: number; to: number } | null {
  let found: { from: number; to: number } | null = null
  doc.forEach((node, offset) => {
    if (node.type.name === 'reportSection' && node.attrs.key === key) found = { from: offset + 1, to: offset + node.nodeSize - 1 }
  })
  return found
}
