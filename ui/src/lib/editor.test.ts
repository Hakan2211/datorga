import { describe, expect, it } from 'vitest'
import { Editor, createNodeFromContent } from '@tiptap/core'
import type { Fragment } from '@tiptap/pm/model'
import type { SectionDef } from '@/bindings/SectionDef'
import { ReportLocked, decideUnbacked, documentExtensions, documentHtml, sectionBodies, sectionPos, unbackedSentences } from './editor'

const def = (key: string, field: SectionDef['field'] = 'prose') =>
  ({ key, heading: key, number: null, field, hints: [], required: true, ai_allowed: true }) as unknown as SectionDef

const sections = [def('anamnese'), def('diagnose', 'diagnoses'), def('plan')]

function editorFor(bodies: Record<string, string>) {
  return new Editor({ extensions: [...documentExtensions(sections, ''), ReportLocked], content: documentHtml(sections, bodies) })
}

describe('document mode mapping', () => {
  it('gives back each section body unchanged', () => {
    const bodies = {
      anamnese: '<h3>Aktuell</h3><p>Seit <u>2024</u> <strong>Panik</strong></p><ul><li><p>Schlaf</p></li></ul>',
      plan: '<p style="text-align: center;">zentriert</p>',
    }
    const ed = editorFor(bodies)
    expect(sectionBodies(ed.state.doc, ed.schema)).toEqual(bodies)
  })

  it('maps an empty section to an empty body and keeps diagnoses out', () => {
    const ed = editorFor({ anamnese: '', plan: '<p>x</p>' })
    expect(sectionBodies(ed.state.doc, ed.schema)).toEqual({ anamnese: '', plan: '<p>x</p>' })
  })

  it('keeps selections inside one section, so sections cannot be deleted', () => {
    const ed = editorFor({ anamnese: '<p>a</p>', plan: '<p>b</p>' })
    ed.commands.selectAll()
    ed.commands.deleteSelection()
    expect(ed.state.doc.childCount).toBe(3)
    expect(sectionBodies(ed.state.doc, ed.schema)).toEqual({ anamnese: '', plan: '<p>b</p>' })

    // A drag from "plan" back into "anamnese" stops at the start of "plan".
    const plan = sectionPos(ed.state.doc, 'plan')!
    ed.commands.setTextSelection({ from: plan.to - 1, to: 2 })
    expect(ed.state.selection.from).toBeGreaterThanOrEqual(plan.from)
    ed.commands.insertContent('x')
    expect(sectionBodies(ed.state.doc, ed.schema)).toEqual({ anamnese: '', plan: '<p>x</p>' })
  })

  it('replaces one section from outside', () => {
    const ed = editorFor({ anamnese: '<p>a</p>', plan: '<p>b</p>' })
    const pos = sectionPos(ed.state.doc, 'plan')!
    ed.view.dispatch(ed.state.tr.replaceWith(pos.from, pos.to, createNodeFromContent('<p>neu</p><p>zwei</p>', ed.schema) as Fragment))
    expect(sectionBodies(ed.state.doc, ed.schema)).toEqual({ anamnese: '<p>a</p>', plan: '<p>neu</p><p>zwei</p>' })
  })
})

describe('sentences "ohne Beleg" (slice 47)', () => {
  const html = '<p>Erster Satz. <mark data-ohne-beleg="">Ohne Beleg.</mark> Dritter Satz.</p><p><mark data-ohne-beleg="">Allein.</mark></p>'

  it('lists the marked sentences in order', () => {
    expect(unbackedSentences(html)).toEqual(['Ohne Beleg.', 'Allein.'])
    expect(unbackedSentences('<p>Nichts.</p>')).toEqual([])
  })

  it('keeps a sentence by removing its mark, or deletes it with its space', () => {
    expect(decideUnbacked(html, 0, true)).toBe('<p>Erster Satz. Ohne Beleg. Dritter Satz.</p><p><mark data-ohne-beleg="">Allein.</mark></p>')
    expect(decideUnbacked(html, 0, false)).toBe('<p>Erster Satz. Dritter Satz.</p><p><mark data-ohne-beleg="">Allein.</mark></p>')
    expect(decideUnbacked(html, 1, false)).toBe('<p>Erster Satz. <mark data-ohne-beleg="">Ohne Beleg.</mark> Dritter Satz.</p>')
    expect(decideUnbacked('<p><mark data-ohne-beleg="">Vorne.</mark> Rest.</p>', 0, false)).toBe('<p>Rest.</p>')
    expect(decideUnbacked(html, 5, true)).toBe(html)
  })

  it('survives the editor: the mark is parsed and written back', () => {
    const editor = new Editor({ extensions: documentExtensions(sections, ''), content: documentHtml(sections, { anamnese: html, plan: '' }) })
    const body = sectionBodies(editor.state.doc, editor.schema).anamnese
    expect(unbackedSentences(body)).toEqual(['Ohne Beleg.', 'Allein.'])
    editor.destroy()
  })
})
