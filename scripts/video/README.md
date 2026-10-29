# Erklärvideo aufnehmen

`record.mjs` nimmt „thera in 5 Minuten“ in der Demo-Praxis auf. Storyboard und Sprechertexte stehen in `docs/video/skript.md`, die Klicks je Einstellung in `shots.mjs`.

```
cd scripts/video
npm install
node record.mjs --dry            # ohne Stimme und Aufnahme: ein Bild je Einstellung in out/dry/
node record.mjs                  # out/thera-in-5-minuten.mp4 + .de.vtt
node record.mjs --voice sage     # andere OpenAI-Stimme
node record.mjs --dry --until fakten   # hält nach der Einstellung an, thera bleibt offen (CDP :9333)
```

Voraussetzungen:
- Debug-Build `target/debug/thera-app.exe` (`cargo build -p thera-app`); nur er kennt `THERA_DATA_DIR`. Die Demo läuft in einem frischen Ordner im Temp-Verzeichnis mit eigenem Schlüsselbund-Eintrag (`video-master-key`, wird danach gelöscht); die Sprachmodelle aus `%APPDATA%\de.datorga.thera\models` werden hart verlinkt.
- Vite auf :3000 (startet sonst selbst), `ffmpeg`/`ffprobe` im PATH.
- OpenAI-Schlüssel für die Stimme: `OPENAI_API_KEY` oder der in thera hinterlegte (Einstellungen → Stimme). Gesendet wird nur der Sprechertext; die Clips liegen in `out/voice/` und werden nur bei geändertem Text neu erzeugt (etwa 0,05 $ für alle).
- Die KI-Schritte in der Demo beantwortet `mock-llm.mjs` lokal (Fakten und Zusammenfassung für Jonas Albrecht, Berichtsentwürfe aus den gesendeten Fakten).

Während der Aufnahme keine Dateien in `ui/` ändern: Vite lädt die Seite sonst neu.

Bild: 1280 × 720 CSS-Pixel mit Faktor 1,5 per CDP-Screencast (1920 × 1080), dazu ein gezeichneter Mauszeiger, Kapitel-Einblendungen und Titelkarten (`director.mjs`). Jede Einstellung dauert so lange wie ihr Sprechertext.

Veröffentlicht wird das MP4 auf bunny.net (Untertitel `.de.vtt` dort hochladen); die Adresse kommt in `VIDEO_URL` in `ui/src/lib/help.ts`.
