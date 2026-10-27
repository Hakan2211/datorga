// Bericht layout. Data arrives as JSON in sys.inputs.data (see render/mod.rs: RenderDoc).
#let d = json(bytes(sys.inputs.data))

#set document(title: d.title)
#set page(paper: "a4", margin: (x: 2.2cm, top: 2.4cm, bottom: 2.2cm),
  header: context {
    set text(8.5pt, fill: luma(90))
    grid(columns: (1fr, auto),
      [Chiffre: *#d.chiffre*#if d.method != none [ #h(1em) Verfahren: #d.method]],
      [#if d.report_no != none [Bericht Nr. #d.report_no #h(1em)] Seite #counter(page).display() / #counter(page).final().first()])
  })
#set text(font: "Libertinus Serif", size: 11pt, lang: "de", hyphenate: true)
#set par(justify: true, leading: 0.58em, spacing: 0.9em)

#let runs(rs) = {
  for r in rs {
    let parts = r.text.split("\n")
    for (i, p) in parts.enumerate() {
      let c = [#p]
      if r.bold { c = strong(c) }
      if r.italic { c = emph(c) }
      c
      if i < parts.len() - 1 { linebreak() }
    }
  }
}

#let blocks(bs) = {
  for b in bs {
    if b.type == "p" { par(runs(b.runs)) }
    else if b.type == "ul" { list(..b.items.map(it => runs(it))) }
    else if b.type == "ol" { enum(..b.items.map(it => runs(it))) }
  }
}

#if d.practice != none {
  align(right, text(9pt, d.practice))
  v(0.6em)
}

#text(14pt, weight: "bold", d.title)
#v(0.2em)
#text(10.5pt, fill: luma(60), d.subtitle)
#v(1em)

#for s in d.sections [
  #block(above: 1.1em, below: 0.5em, sticky: true,
    text(weight: "bold", [#if s.number != none [#s.number. ]#s.heading]))
  #if s.diagnoses.len() > 0 {
    table(columns: (auto, 1fr, auto), stroke: 0.4pt + luma(170), inset: 5pt,
      table.header([*ICD-10*], [*Diagnose*], [*Sicherheit*]),
      ..s.diagnoses.map(x => (x.code, x.label, x.certainty)).flatten())
  }
  #blocks(s.blocks)
]

#v(2.2em)
#grid(columns: (1fr, 1fr), gutter: 2em,
  [#line(length: 100%, stroke: 0.5pt) #text(9pt)[Datum: #d.date]],
  [#line(length: 100%, stroke: 0.5pt) #text(9pt)[Unterschrift#if d.signed_by != none [: #d.signed_by]]])
