// Notfall-Kit (recovery kit). Data arrives as JSON in sys.inputs.data (see render/mod.rs: RecoveryKit).
#let d = json(bytes(sys.inputs.data))

#set document(title: "thera – Notfall-Kit")
#set page(paper: "a4", margin: (x: 2.2cm, top: 2.2cm, bottom: 2cm),
  footer: context {
    set text(8pt, fill: luma(110))
    [Vertraulich – getrennt vom Computer aufbewahren. #h(1fr) Seite #counter(page).display() / #counter(page).final().first()]
  })
#set text(font: "Libertinus Serif", size: 11pt, lang: "de", hyphenate: true)
#set par(justify: true, leading: 0.58em, spacing: 0.9em)
#show heading: set text(12pt)
#show heading: set block(above: 1.3em, below: 0.6em)

#text(18pt, weight: "bold")[Notfall-Kit für thera]
#v(-0.3em)
#text(10.5pt, fill: luma(70))[Wiederherstellungsschlüssel für die verschlüsselte Praxisdatenbank]

#v(0.8em)
#block(width: 100%, inset: 14pt, radius: 4pt, stroke: 1.2pt + black)[
  #text(10pt, weight: "bold")[Notfall-Schlüssel]
  #v(0.4em)
  #align(center, text(font: "DejaVu Sans Mono", size: 15pt, tracking: 0.04em)[
    #grid(columns: 4, column-gutter: 1.6em, row-gutter: 0.75em,
      ..d.groups.map(g => [#g]))
  ])
  #v(0.2em)
  #text(9pt, fill: luma(80))[64 Zeichen (0–9, A–F) in 16 Vierergruppen. Bindestriche, Leerzeichen und Groß-/Kleinschreibung spielen bei der Eingabe keine Rolle.]
]

#v(0.4em)
#table(columns: (auto, 1fr), stroke: none, inset: (x: 0pt, y: 3pt), column-gutter: 1.2em,
  [*Erstellt am*], [#d.created],
  [*Anlass*], [#d.reason],
  [*Computer*], [#d.device],
  [*Programm*], [thera #d.app_version],
  [*Datenordner*], [#text(font: "DejaVu Sans Mono", size: 8.5pt)[#d.data_dir]],
  [*Aufbewahrungsort*], [#box(width: 1fr, stroke: (bottom: 0.5pt + luma(120)), height: 1em)],
)

= Wann brauchen Sie diesen Schlüssel?
Der Schlüssel zur Datenbank liegt in der Windows-Anmeldeinformationsverwaltung dieses Benutzerkontos. Geht er dort verloren, meldet thera beim Start *„Wiederherstellung erforderlich“*. Das passiert zum Beispiel nach
- einer Neuinstallation von Windows oder einem neuen Benutzerkonto,
- dem Umzug des Datenordners auf einen anderen Computer (etwa aus einer Datensicherung),
- dem Löschen gespeicherter Anmeldeinformationen.

= So stellen Sie die Daten wieder her
+ Sorgen Sie dafür, dass der Datenordner vollständig vorhanden ist (mindestens `thera.db` und `master.wrapped`, bei Aufnahmen auch der Unterordner `audio`).
+ Starten Sie thera. Auf dem Bildschirm „Wiederherstellung erforderlich“ geben Sie den Notfall-Schlüssel oben ein.
+ Klicken Sie auf „Wiederherstellen“. thera legt den Schlüssel wieder im Schlüsselbund ab und öffnet Ihre Daten wie gewohnt.

Wird der Schlüssel abgelehnt, prüfen Sie, ob er vom *neuesten* Notfall-Kit stammt und ob die Datei `master.wrapped` zu diesem Datenordner gehört.

= Wichtig
- *Ohne diesen Schlüssel und ohne den Schlüsselbund sind die Daten unwiederbringlich verloren.* Niemand, auch nicht der Hersteller, kann sie dann noch öffnen.
- Wer diesen Schlüssel *und* den Datenordner besitzt, kann alle Patientendaten lesen. Bewahren Sie das Blatt deshalb wie eine Patientenakte auf: ausgedruckt, getrennt vom Computer und von Datensicherungen, zum Beispiel im Praxistresor.
- Nicht fotografieren, nicht per E-Mail versenden, nicht in einer Cloud ablegen. Löschen Sie die PDF-Datei nach dem Ausdrucken.
- Unter *Einstellungen → Sicherheit → Neuen Notfall-Schlüssel erstellen* erzeugen Sie jederzeit einen neuen Schlüssel. Ältere Notfall-Kits sind danach ungültig und sollten vernichtet werden.
