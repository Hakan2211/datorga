// Letter layout, DIN 5008 Form B. Data arrives as JSON in sys.inputs.data (see render/letter.rs:
// LetterDoc); the logo and signature images as bytes in sys.inputs.logo / sys.inputs.signature.
#let d = json(bytes(sys.inputs.data))
#let logo = sys.inputs.at("logo", default: none)
#let signature = sys.inputs.at("signature", default: none)

// Page edges (Form B): text from 25 mm left, address window 45 mm from the top, 20 mm from the
// left, 85 × 45 mm. The page margin is the text area of the following pages.
#let margin-top = 20mm
#let margin-left = 25mm

#set document(title: d.subject)
#set text(font: "Libertinus Serif", size: 11pt, lang: "de", hyphenate: true)
#set par(justify: true, leading: 0.58em, spacing: 0.9em)
#set page(paper: "a4", margin: (left: margin-left, right: 20mm, top: margin-top, bottom: 30mm),
  header: context {
    let n = counter(page).get().first()
    if n > 1 {
      set text(8.5pt, fill: luma(90))
      grid(columns: (1fr, auto), d.short_subject, [Seite #n von #counter(page).final().first()])
    }
  },
  footer: if d.footer.len() > 0 {
    set text(7pt, fill: luma(80))
    set par(justify: false, leading: 0.4em)
    line(length: 100%, stroke: 0.4pt + luma(170))
    v(-0.3em)
    grid(columns: d.footer.map(_ => 1fr), column-gutter: 1em,
      ..d.footer.map(col => col.map(x => [#x]).join(linebreak())))
  },
  // Fold marks (105 mm, 210 mm) and the punch mark (148.5 mm) on the first page.
  background: context if counter(page).get().first() == 1 {
    for y in (105mm, 148.5mm, 210mm) {
      place(top + left, dx: 4mm, dy: y, line(length: if y == 148.5mm { 7mm } else { 5mm }, stroke: 0.3pt + luma(140)))
    }
  })

#let runs(rs) = {
  for r in rs {
    let parts = r.text.split("\n")
    for (i, p) in parts.enumerate() {
      let c = [#p]
      if r.bold { c = strong(c) }
      if r.italic { c = emph(c) }
      if r.underline { c = underline(c) }
      c
      if i < parts.len() - 1 { linebreak() }
    }
  }
}

#let aligned(a, body) = {
  if a == none or a == "justify" { body }
  else { set par(justify: false); align(if a == "center" { center } else if a == "right" { right } else { left }, body) }
}

#let blocks(bs) = {
  for b in bs {
    if b.type == "p" { aligned(b.align, par(runs(b.runs))) }
    else if b.type == "h" { block(above: 1em, below: 0.45em, sticky: true, aligned(b.align, strong(runs(b.runs)))) }
    else if b.type == "ul" { list(..b.items.map(it => runs(it))) }
    else if b.type == "ol" { enum(..b.items.map(it => runs(it))) }
  }
}

// Letterhead: name and subtitle centred, logo top right.
#if logo != none {
  place(top + right, dy: -8mm, box(width: 36mm, height: 22mm, align(right + horizon, image(logo, width: 100%, height: 100%, fit: "contain"))))
}
#place(top + center, dy: -4mm, box(width: 88mm, align(center)[
  #set par(justify: false)
  #text(15pt, weight: "bold", d.practice_name)
  #if d.practice_subtitle != "" { linebreak(); text(9.5pt, fill: luma(60), d.practice_subtitle) }
]))

// Address window.
// The window starts 20 mm from the left edge; its text lines up with the body at 25 mm.
#place(top + left, dy: 45mm - margin-top, box(width: 85mm, height: 45mm, {
  set par(justify: false, leading: 0.45em, spacing: 0.45em)
  v(10mm)
  if d.sender_line != "" { text(7pt, underline(d.sender_line)) }
  v(2.5mm)
  for l in d.recipient { [#l \ ] }
}))

// Body starts below the window (Form B: Betreff at about 98 mm).
#v(98mm - margin-top)
#align(right, d.place_date)
#v(0.8em)
#par(justify: false, strong(d.subject))
#if d.refs.len() > 0 {
  v(-0.4em)
  set par(justify: false, leading: 0.45em)
  for r in d.refs [#r.label: #r.value \ ]
}
#v(0.8em)
#d.salutation
#v(0.2em)

#for s in d.sections [
  #if s.heading != "" {
    block(above: 1.1em, below: 0.5em, sticky: true,
      text(weight: "bold", [#if s.number != none [#s.number. ]#s.heading]))
  }
  #if s.diagnoses.len() > 0 {
    table(columns: (auto, 1fr, auto), stroke: 0.4pt + luma(170), inset: 5pt,
      table.header([*ICD-10*], [*Diagnose*], [*Sicherheit*]),
      ..s.diagnoses.map(x => (x.code, x.label, x.certainty)).flatten())
  }
  #blocks(s.blocks)
]

#block(breakable: false, above: 1.2em)[
  #if d.closing != none [#d.closing \ ]
  #v(0.6em)
  Mit freundlichen Grüßen
  #if signature != none { v(0.3em); image(signature, height: 16mm, fit: "contain"); v(0.1em) } else { v(14mm) }
  #if d.signer != none [#d.signer]
  #set text(9.5pt)
  #for t in d.titles [ \ #t]
]

#for a in d.addenda [
  #block(above: 2em, below: 0.5em, sticky: true, text(weight: "bold", a.heading))
  #blocks(a.blocks)
]
