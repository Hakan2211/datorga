# Video: thera in 5 Minuten

Erklärvideo für die Hilfe (`VIDEO_URL` in `ui/src/lib/help.ts`) und den letzten Schritt des Rundgangs. Aufgenommen wird nur in der Demo-Praxis: alle Personen und Inhalte sind erfunden.

**Aufnahme:** `scripts/video/record.mjs` (siehe `scripts/video/README.md`) liest die Sprechertexte unten, lässt sie per OpenAI-Sprachausgabe sprechen, klickt die Demo-Praxis über CDP durch und baut daraus `scripts/video/out/thera-in-5-minuten.mp4` (1920 × 1080) und Untertitel (`.vtt`). Die Länge jeder Einstellung richtet sich nach ihrem Sprechertext. Wer einen Text ändert, nimmt neu auf; nur geänderte Texte werden neu vertont.

**Format dieser Datei** (das Skript liest es): `## Kapitel` gruppiert, `### <id> · Titel` ist eine Einstellung, Zeilen mit `> ` sind der Sprechertext, `Bild:` beschreibt, was zu sehen ist. Die Kapitel folgen den Hilfe-Artikeln.

Stimme: `coral` (warm, weiblich), Modell `gpt-4o-mini-tts`, ruhiges Tempo. Gesamtlänge etwa 4½ Minuten.

## 1 · Erste Schritte

### intro · Willkommen
Bild: Titelkarte „thera in 5 Minuten“, dann „Heute“ der Demo-Praxis mit dem gelben Demo-Hinweis.
> thera hilft Ihnen, Sitzungen zu dokumentieren und Berichte an die Gutachterin oder den Gutachter zu schreiben. Was Sie hier sehen, ist die Demo-Praxis: Alle Personen sind erfunden. Ihre echten Daten liegen verschlüsselt auf Ihrem Rechner.

### heute · Heute
Bild: Maus über den Termin des Tages, die Aufgaben und die Karte „Fällig“.
> Ihr Tag beginnt mit „Heute“: den Terminen, Ihren Aufgaben und allem, was fällig ist. Zum Beispiel eine Abschrift, die noch geprüft werden will, oder ein Antrag, der bald gestellt werden sollte.

### kalender · Kalender
Bild: Kalender in der Wochenansicht, Maus über einen geplanten Termin, Wechsel zur Monatsansicht.
> Im Kalender planen Sie einzelne Sitzungen oder ganze Serien. Einen Termin verschieben Sie einfach mit der Maus.

### patienten · Patient:innen
Bild: Karten der drei Demo-Personen, Filter „VT“ an und wieder aus, Klick auf Lena Sommer.
> Unter Patient:innen sehen Sie alle Behandlungsfälle als Karten, mit Verfahren, Diagnose und dem nächsten Termin. Filter helfen beim Suchen.

### verlauf · Verlauf
Bild: Reiter „Verlauf“ von Lena Sommer, langsam nach unten scrollen.
> Der Verlauf zeigt jede Sitzung mit einer kurzen Zusammenfassung, dazu Anträge und Einwilligungen. Jeder Punkt nennt seine Quellen in der Abschrift.

## 2 · Sitzung aufnehmen

### aufnahme · Aufnahme starten
Bild: Heutige Sitzung von Lena Sommer öffnen, „Weiter zur Aufnahme“, „Einwilligung liegt vor“, Klick auf „Sitzung aufnehmen“; die Aufnahme läuft.
> Zur Sitzung öffnen Sie den Termin. Liegt die Einwilligung zur Tonaufnahme vor, starten Sie die Aufnahme mit einem Klick. Das Audio wird nur verschlüsselt gespeichert.

### stopp · Aufnahme beenden
Bild: Klick auf „Beenden“, die Abschrift startet von selbst.
> Nach dem Gespräch beenden Sie die Aufnahme. Die Abschrift entsteht auf Ihrem Rechner. Aufnahmen mit der Stimme Ihrer Patientinnen und Patienten verlassen das Gerät nie.

## 3 · Autopilot

### pruefen · Abschrift prüfen
Bild: Sitzung von Jonas Albrecht von gestern, Abschrift mit Therapeut/in und Patient/in, langsam scrollen.
> Danach übernimmt der Autopilot. Ihre Aufgabe ist nur, die Abschrift zu prüfen. thera trennt die Sprecher, und Sie korrigieren, wo nötig.

### bestaetigen · Bestätigen
Bild: Klick auf „Abschrift bestätigen“; die Schritte des Autopiloten haken sich nacheinander ab.
> Sobald Sie bestätigen, legt thera die Sitzungsnotiz an, schlägt Fakten vor und schreibt eine kurze Zusammenfassung. Das läuft im Hintergrund, Sie können weiterarbeiten.

## 4 · Notiz & Fakten

### fakten · Fakten übernehmen
Bild: Schritt „Notiz“, die vorgeschlagenen Fakten mit ihren Quellen; einen Fakt übernehmen.
> In der Notiz stehen die Vorschläge der KI. Jeder Fakt zeigt, aus welcher Stelle der Abschrift er stammt. Sie übernehmen ihn, ändern ihn oder verwerfen ihn.

### signieren · Signieren
Bild: Alle übernehmen, nach unten zu „Signieren“.
> Wenn alles stimmt, signieren Sie die Notiz. Danach ist sie fest; Ergänzungen kommen als Nachtrag dazu.

## 5 · Bericht

### board · Berichte
Bild: „Berichte“ als Board, Hinweis „bald fällig“ oben, die Karte von Lena Sommer.
> Unter Berichte sehen Sie alle Anträge als Board, vom Entwurf bis zum Export. thera rechnet mit, wann das Kontingent endet, und erinnert Sie rechtzeitig.

### entwurf · KI-Entwurf
Bild: Umwandlungsantrag von Lena Sommer öffnen (Schritt „Abschnitte“), „Leere Abschnitte entwerfen“, die Vorschau des ausgehenden Texts, „Senden und entwerfen“, zwei Sätze übernehmen.
> Der Bericht-Lotse führt Sie Schritt für Schritt. Die KI entwirft leere Abschnitte aus Ihren signierten Notizen; vorher zeigt thera, welcher Text den Rechner verlässt. Sie gehen die Vorschläge Satz für Satz durch. Diagnosen und Medikation trägt die KI nie ein.

### gutachter · Prüfen und Export
Bild: Schritt „Prüfen“, „Mit KI prüfen“, „Bericht prüfen lassen“, ein Hinweis „Ein Gutachter würde fragen …“, dann „Weiter: Unterschreiben“, Maus über den PDF-Export.
> Beim Prüfen liest die KI den Bericht wie ein Gutachter und nennt, was nachgefragt würde. Den Text ändert sie nicht. Zum Schluss sehen Sie den Bericht im Format des Musters PTV 3, unterschreiben und exportieren als PDF oder Word.

## 6 · Datenschutz

### gesendet · Was wurde gesendet?
Bild: Zurück zur Sitzung von Jonas Albrecht, „Was wurde gesendet?“ öffnen, der pseudonymisierte Text.
> Bevor ein Text an einen KI-Dienst geht, ersetzt thera Namen, Orte und Daten durch Platzhalter. Unter „Was wurde gesendet?“ sehen Sie genau, was Ihren Rechner verlassen hat.

## 7 · KI-Anbieter & Kosten

### anbieter · Eigener Schlüssel
Bild: Hilfe-Artikel „KI-Anbieter & Kosten“, die Anleitung „OpenRouter einrichten“ mit fünf Schritten.
> Den KI-Anbieter wählen Sie selbst, etwa OpenRouter ohne Datenspeicherung beim Anbieter, oder ein lokales Modell ganz ohne Internet. Eine Anleitung führt Sie in fünf Schritten zum eigenen Schlüssel.

### kosten · Kosten
Bild: Einstellungen, Karte „KI-Kosten & Guthaben“.
> Was die KI kostet, sehen Sie in den Einstellungen, nach Aufgabe und nach Modell. Wird das Guthaben knapp, warnt thera rechtzeitig.

## 8 · Datensicherung & Notfall-Kit

### sicherung · Datensicherung
Bild: Einstellungen, Karte „Datensicherung“, Maus über „Sicherung erstellen“.
> Sichern Sie Ihre Daten regelmäßig. Die Sicherung ist verschlüsselt, und Ihr Notfall-Kit vom ersten Start ist der Schlüssel dazu. Bewahren Sie es gut auf.

## 9 · Hilfe

### hilfe · Hilfe und Suche
Bild: „Hilfe“ öffnen, „Schlüssel“ suchen, dann Strg + K mit der Befehlssuche.
> Fragen beantwortet die Hilfe, auch ohne Internet. Mit Strg und K finden Sie jede Seite und jeden Befehl.

### ende · Los geht's
Bild: Zurück zu „Heute“, Maus auf „Demo verlassen“, Abschlusskarte.
> Probieren Sie alles in Ruhe in der Demo-Praxis aus. Wenn Sie bereit sind, klicken Sie auf „Demo verlassen“ und legen Ihre erste echte Sitzung an.
