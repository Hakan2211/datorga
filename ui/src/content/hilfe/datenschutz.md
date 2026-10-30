---
title: Datenschutz & Pseudonymisierung
summary: Was den Computer verlässt und was nicht
order: 6
keywords: datenschutz, dsgvo, pseudonym, platzhalter, verschlüsselung, einwilligung, protokoll, schweigepflicht, cloud
---
Theralo ist so gebaut, dass Patientendaten auf diesem Computer bleiben.

## Verschlüsselt gespeichert

Die Datenbank und alle Aufnahmen sind verschlüsselt (AES-256). Der Schlüssel liegt im Schlüsselbund von Windows. Für den Notfall gibt es den Notfall-Schlüssel, siehe [Datensicherung & Notfall-Kit](hilfe:datensicherung).

## Pseudonymisierung vor jeder KI-Anfrage

Bevor ein Text an einen Cloud-Anbieter geht, ersetzt Theralo Namen, Orte, Arbeitgeber und Datumsangaben durch Platzhalter wie `[PERSON_1]`. Die Patientin bzw. der Patient erscheint als „Pat.“. Die Antwort wird auf diesem Computer zurückübersetzt. Die Zuordnung der Platzhalter verlässt den Computer nie.

- Bei manuellen Anfragen sehen Sie vorher die **Vorschau** des ausgehenden Texts. Steht dort noch ein Name im Klartext, markieren Sie ihn; er wird als eigener Begriff für diese Person gespeichert und künftig ersetzt.
- Beim [Autopilot](hilfe:autopilot) sehen Sie das Gesendete danach unter „Was wurde gesendet?“.
- Mit einem lokalen Modell verlässt gar nichts den Computer.

## Was nie an die KI geht

- Tonaufnahmen von Sitzungen, außer Sie haben die Cloud-Transkription mit Einwilligung gewählt
- Diagnosen und Medikation: Die KI schlägt sie nicht vor
- die Zuordnung der Platzhalter

## Einwilligungen

Für Tonaufnahmen und für die Cloud-Transkription erfassen Sie die Einwilligung auf der Patientenseite. Der Text ist versioniert. Wird eine Einwilligung widerrufen, stoppt eine laufende Aufnahme sofort.

## Protokoll

Jede Änderung und jede KI-Anfrage wird ohne Inhalte in einer manipulationssicheren Kette protokolliert. Sie öffnen es unter [Einstellungen → Protokoll](/settings/protokoll).
