// The actions per shot of docs/video/skript.md, in its order. Each gets the Director (`d`) and
// runs while its voice-over plays; the recorder waits for the voice to end afterwards.

export const SHOTS = {
  async intro(d) {
    await d.card('thera in 5 Minuten', 'Demo-Praxis · alle Personen sind erfunden')
    await d.wait(3500)
    await d.hideCard()
    await d.wait(1200)
    await d.hover(d.page.getByText('Demo-Praxis:'), 2500)
  },

  async heute(d) {
    await d.hover(d.page.getByText('Sitzung 12 · Lena Sommer'), 1800)
    await d.hover(d.page.getByText('Aufgaben', { exact: true }), 1500)
    await d.hover(d.page.getByText(/Transkript prüfen/).first(), 1500)
    await d.hover(d.page.getByText(/Antrag vorbereiten/).first(), 1200)
  },

  async kalender(d) {
    await d.nav('Kalender')
    await d.wait(800)
    await d.hover(d.page.getByText('S140391').first(), 1600)
    await d.click(d.page.getByTestId('calendar-view').getByText('Monat', { exact: true }))
    await d.wait(1000)
  },

  async patienten(d) {
    await d.nav('Patient:innen')
    await d.wait(800)
    await d.click(d.page.getByRole('button', { name: 'VT', exact: true }))
    await d.wait(900)
    await d.click(d.page.getByRole('button', { name: 'Alle', exact: true }))
    await d.untilVoiceEnds(1.2)
    await d.click(d.page.getByText('Lena Sommer').first())
  },

  async verlauf(d) {
    await d.click(d.page.getByRole('tab', { name: 'Verlauf' }))
    await d.hover(d.page.getByText('KI-Zusammenfassung').first(), 800)
    await d.scroll(500, 2500)
    await d.hover(d.page.getByText(/\d Quellen?/).first(), 1200)
  },

  async aufnahme(d) {
    await d.nav('Heute')
    await d.click(d.page.getByRole('button', { name: 'Vorbereiten' }).first())
    await d.click(d.page.getByRole('button', { name: 'Weiter zur Aufnahme' }))
    await d.hover(d.page.getByText('Einwilligung liegt vor'), 1200)
    await d.click(d.page.getByRole('button', { name: 'Sitzung aufnehmen' }))
    await d.wait(2000)
  },

  async stopp(d) {
    await d.wait(1500)
    await d.click(d.page.getByRole('button', { name: /Beenden|Aufnahme beenden/ }).first())
    await d.wait(2500)
  },

  async pruefen(d) {
    await d.nav('Sitzungen')
    await d.wait(700)
    await d.click(d.page.getByTestId('session-row').filter({ hasText: 'Jonas' }).first().getByRole('link', { name: 'Weiter' }))
    await d.wait(800)
    await d.point(d.page.getByText('Sprecher tauschen'))
    await d.scroll(450, 2500)
    await d.hover(d.page.getByText('Patient/in').nth(1), 1200)
  },

  async bestaetigen(d) {
    await d.click(d.page.getByRole('button', { name: 'Abschrift bestätigen' }))
    await d.click(d.page.getByRole('button', { name: 'Bestätigen und Audio löschen' }))
    await d.wait(800)
    await d.hover(d.page.getByTestId('autopilot-steps'), 3000)
    await d.hover(d.page.getByText('KI-Zusammenfassung').first(), 1500)
  },

  async fakten(d) {
    await d.click(d.page.getByRole('link', { name: 'Notiz bearbeiten' }).last())
    await d.wait(1000)
    await d.hover(d.page.getByText('Mathearbeit').first(), 1500)
    await d.click(d.page.getByText('00:12', { exact: true }).first())
    await d.hover(d.page.getByText('Abschrift', { exact: true }).last(), 1200)
    await d.hover(d.page.getByText('Abwertende Reaktion').first(), 1000)
  },

  async signieren(d) {
    await d.click(d.page.getByRole('button', { name: /Punkte übernehmen/ }))
    await d.wait(1200)
    await d.click(d.page.getByRole('button', { name: 'Unterschreiben' }).first())
    await d.wait(1500)
  },

  async board(d) {
    await d.nav('Berichte')
    await d.wait(800)
    await d.hover(d.page.getByText(/bald fällig/).first(), 1800)
    await d.hover(d.page.getByText('Lena Sommer').first(), 1500)
  },

  async entwurf(d) {
    // The Bericht-Lotse opens a half-written Bericht on "Abschnitte", one section at a time.
    await d.click(d.page.getByText('Lena Sommer').first())
    await d.wait(800)
    await d.click(d.page.getByTestId('lotse-draft-empty'))
    await d.wait(1500)
    await d.hover(d.page.getByText('Platzhalter (nur lokal)', { exact: false }).first(), 1200)
    await d.click(d.page.getByRole('button', { name: 'Senden und entwerfen' }))
    await d.wait(3000)
    // Somatik has no facts in the demo (its proposal is empty): show the Umwandlung section.
    await d.click(d.page.locator('[data-testid="focus-rail"] [data-section="umwandlung"]'))
    await d.hover(d.page.locator('[data-testid="sentence"][data-state="current"]').first(), 1000)
    await d.click(d.page.getByTestId('sentence-accept').first(), { before: 800 })
    await d.wait(700)
    await d.press('Enter')
    await d.wait(700)
  },

  async gutachter(d) {
    await d.click(d.page.getByRole('button', { name: /Prüfen$/ }).first())
    await d.wait(1200)
    await d.click(d.page.getByTestId('check-run'))
    await d.wait(800)
    await d.click(d.page.getByRole('button', { name: 'Bericht prüfen lassen' }))
    await d.wait(2500)
    await d.hover(d.page.getByTestId('check-entry').filter({ hasText: 'Gutachter würde fragen' }).first(), 1500)
    // The toast sits over the bottom bar at video size: on via the stepper.
    await d.click(d.page.getByRole('button', { name: /Fertig$/ }).first())
    await d.wait(1000)
    await d.hover(d.page.getByTestId('finish-pdf'), 1200)
  },

  async gesendet(d) {
    await d.press('Escape')
    await d.nav('Sitzungen')
    await d.click(d.page.getByTestId('session-row').filter({ hasText: 'Jonas' }).first().getByRole('link', { name: 'Weiter' }))
    // The signed note moved the stepper on; the autopilot card sits on the "Notiz" step.
    await d.click(d.page.getByRole('button', { name: /^\d*\s*Notiz$/ }).first())
    await d.click(d.page.getByRole('button', { name: 'Was wurde gesendet?' }).first())
    await d.wait(1500)
    await d.hover(d.page.getByTestId('sent-outgoing').first(), 2500)
  },

  async anbieter(d) {
    await d.press('Escape')
    await d.nav('Hilfe')
    await d.click(d.page.getByRole('link', { name: /KI-Anbieter/ }).first())
    await d.click(d.page.getByRole('button', { name: 'OpenRouter einrichten' }))
    await d.wait(2500)
  },

  async kosten(d) {
    await d.press('Escape')
    await d.nav('Einstellungen')
    await d.page.evaluate(() => document.getElementById('costs')?.scrollIntoView({ behavior: 'smooth', block: 'start' }))
    await d.wait(1500)
    await d.hover(d.page.locator('#costs'), 2000, { dy: 0.3 })
  },

  async sicherung(d) {
    await d.hover(d.page.getByRole('button', { name: /Sicherung erstellen|Jetzt sichern/ }).first(), 2500)
  },

  async hilfe(d) {
    await d.nav('Hilfe')
    await d.click(d.page.getByPlaceholder('Hilfe durchsuchen…'))
    await d.type('Schlüssel')
    await d.wait(1800)
    await d.press('Control+k')
    await d.wait(600)
    await d.type('Bericht')
    await d.wait(1800)
    await d.press('Escape')
  },

  async ende(d) {
    await d.nav('Heute')
    await d.hover(d.page.getByRole('button', { name: 'Demo verlassen' }), 1500)
    await d.untilVoiceEnds(0)
    await d.card('thera', 'Hilfe und Rundgang finden Sie jederzeit in der App.')
    await d.wait(3000)
  },
}
