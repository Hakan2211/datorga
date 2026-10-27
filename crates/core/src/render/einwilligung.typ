// Printable consent form. Data arrives as JSON in sys.inputs.data (see render/mod.rs: ConsentForm).
#let d = json(bytes(sys.inputs.data))

#set document(title: d.title)
#set page(paper: "a4", margin: (x: 2.2cm, top: 2cm, bottom: 2cm),
  footer: context {
    set text(7.5pt, fill: luma(110))
    [Textfassung #d.version · Prüfsumme #d.hash_short #h(1fr) erstellt am #d.created]
  })
#set text(font: "Libertinus Serif", size: 11pt, lang: "de", hyphenate: true)
#set par(justify: true, leading: 0.6em, spacing: 0.9em)

#let sigline(label) = block(width: 100%, above: 2.8em)[
  #grid(columns: (1fr, 1.6fr), column-gutter: 1.2em,
    [#line(length: 100%, stroke: 0.5pt) #v(-0.6em) #text(8.5pt, fill: luma(80))[Ort, Datum]],
    [#line(length: 100%, stroke: 0.5pt) #v(-0.6em) #text(8.5pt, fill: luma(80))[#label]])
]

#if d.practice != none {
  align(right, text(9pt, d.practice))
  v(0.4em)
}
#if d.draft {
  block(width: 100%, inset: 6pt, fill: luma(235), radius: 3pt,
    text(8.5pt)[*Entwurf:* Dieser Einwilligungstext ist noch nicht rechtlich geprüft und darf so noch nicht mit Patientinnen und Patienten verwendet werden.])
}

#text(15pt, weight: "bold")[#d.title]

#table(columns: (auto, 1fr), stroke: none, inset: (x: 0pt, y: 2pt), column-gutter: 1em,
  [*Patientin / Patient*], [#d.patient_name],
  ..if d.birth_date != none { ([*geboren am*], [#d.birth_date]) },
  ..if d.therapist != none { ([*Therapeutin / Therapeut*], [#d.therapist]) },
)

#for p in d.paragraphs { par(p) }

#sigline[Unterschrift Patientin / Patient]
#if d.guardians {
  sigline[Unterschrift Sorgeberechtigte/r]
  sigline[Unterschrift weitere/r Sorgeberechtigte/r]
}

#v(1fr)
#line(length: 100%, stroke: (paint: luma(120), dash: "dashed", thickness: 0.6pt))
#text(8pt, fill: luma(100))[Bei Bedarf abtrennen und in der Praxis abgeben]
#v(0.2em)
#text(12pt, weight: "bold")[Widerruf der Einwilligung]

Hiermit widerrufe ich meine Einwilligung in die Tonaufnahme meiner Therapiesitzungen (#d.patient_name). Der Widerruf gilt ab sofort für alle künftigen Sitzungen.

#sigline[#if d.guardians [Unterschrift Patientin / Patient bzw. Sorgeberechtigte/r] else [Unterschrift Patientin / Patient]]
