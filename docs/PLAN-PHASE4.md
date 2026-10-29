# Phase 4 plan (approved 2026-10-29): patient cards, timeline + AI summaries, autopilot workflow, onboarding/docs, AI costs

## Context

Phase 3 (slices 16–28) is done. The user's review of the app found these problems:
- Patienten, Sitzungen and Berichte are plain tables. They have no filters, no sorting, no grouping and bare empty states (`ui/src/routes/patients/index.tsx`, `sessions/index.tsx`, `components/reports-table.tsx`).
- There is no visual patient overview.
- There is no per-session summary.
- Every step after a session is a manual button: stop, choose local/cloud, Transkribieren, confirm, Notiz anlegen, Vorschlagen, redaction preview, apply facts, sign. That is about 9 clicks across 4 cards.
- There is no onboarding or help.
- Cost is only shown as a locally summed audit total in the model picker.

Goals:
1. A therapist sees her caseload at a glance.
2. She sees each patient's course over time.
3. The AI does the busy work automatically, and she only reviews, accepts and signs.
4. The app explains itself.
5. She can see what the AI costs and how to top up her balance.

This file is the plan. Build slice by slice; after each slice: commit, push main, `npm run app:build`.

User decisions (2026-10-29):
- **Diagnoses** are entered by the therapist on the Behandlungsfall, and new Berichte take them over.
- **Autopilot is on by default**, including pseudonymized cloud calls without a preview each time. The preview can be viewed afterwards, and a setting turns autopilot off.
- **The video** is hosted online and linked from the app.

## §0 Answers to the review questions

- **Are tables OK?** Partly. For Sitzungen and Berichte, a table is the right form for scanning a work queue. Their real flaw is that they show records instead of *what needs doing*. They become filtered work queues (below). For Patienten, a card grid fits better: a solo practice has about 30–80 active patients, and faces plus status help recognition. A table toggle stays for sorting and export-like scanning.
- **What to delegate to the AI**:
  - transcription start
  - creating the note
  - fact suggestions
  - the session summary
  - "Bericht fällig" detection
  - the first full Bericht draft
- **What stays human**:
  - transcript confirmation (it deletes the audio)
  - accepting facts and proposals
  - diagnoses
  - signing and export
- **What to remove or move**:
  - Audit leaves the main nav and goes under Einstellungen → Sicherheit.
  - The "Vorlagen" placeholder is hidden until the module exists.
  - The raw "Status" column in Sitzungen is removed.
  - Werkstatt is reached from session and Bericht pages only, not as its own concept.
  - The Local/Cloud choice per "Transkribieren" click becomes a default in Settings, with an override in the ⋯ menu.

## Slices

### Step 0: Mockups (user)
The user generates mockups from the appendix prompts into `docs/mockups/10-…` and following. They're needed before slices 29 and 32. Slices 31 and 33 don't depend on them and can go first.

**Done (2026-10-29):** `docs/mockups/10-…` to `18-…`. How to read them:
- Take the content area of each screen.
- Keep the existing shell from slice 21 (sidebar, top bar, Strg+K) and the tokens from slice 20.
- Ignore the sidebar entries the mockups invent (Statistik, Abrechnung, Wissen, Papierkorb, Integrationen, user profile footer).
- Ignore the mockups' placeholder model names. The real list is the curated catalog, never Mistral.
- `14-berichte-board-verfehlt.jpg` missed the prompt: it shows a note document, not a board. Slice 32 builds the board from the prompt text and the column/card style of #13.
- #16 has garbled text in its privacy box. Use the plan wording.

### Slice 29: Patient profile + card view
- Migration V-next on `patient`:
  - `gender` (`w | m | d | x`/keine Angabe)
  - `photo_asset_id` (nullable)
- New `episode_diagnosis` table: episode_id, code, label, certainty G/V/Z/A, entered by the therapist only. It reuses the `Diagnosis` type from `crates/core/src/domain.rs:430`.
- On `create_report` for an application, the `diagnoses` section is prefilled from the episode's diagnoses. Templates keep `ai_allowed: false` on diagnoses (`templates.rs:140`).
- **Photo**:
  - Stored encrypted with the audio pattern from `crates/core/src/audio.rs`: a per-file DEK in a new `media_asset` table, the file at `data_dir/media/<id>.enc`.
  - It's in the backup (extend `backup.rs` entries to `media/…`) and gone on `erase_patient`.
  - It never goes to any LLM.
  - Upload goes through the native open dialog. It's resized to 512 px in Rust (`image` crate) before encryption.
  - Display is a decrypted data URL via a command.
  - Initials avatar as fallback.
  - The consent text gets a line about the photo (optional, revocable). Ask the therapist whether she wants a separate photo consent; default is a checkbox in the patient form.
- **UI**:
  - `/patients` gets a Karten | Tabelle toggle (localStorage).
  - Card contents: photo or avatar, name, Chiffre, age (computed from `birth_date`), Verfahren badge, main diagnosis code, next appointment, open items count (from `due_items`).
  - Filter chips: Altersgruppe (Erw./KJP from episode `age_group`, plus age ranges), Geschlecht, Diagnose (ICD chapter F3x/F4x…), Verfahren, Status (aktiv/archiviert).
  - Sort: Name, nächster Termin, zuletzt gesehen.
  - Patient form (`components/patient-form.tsx`) gets gender and photo. The episode card gets a diagnoses editor (reuse `DiagnosesEditor` from `routes/reports/$reportId.tsx:651`, moved to `components/`).

### Slice 30: Session summary + patient timeline
- New `session_summary` table:
  - session_id unique
  - `points_json` (3–6 bullet key points, each citing segment ids like facts)
  - `status` draft | edited
  - model, created_at
- New `core::summary` reuses the extraction pipeline: pseudonymize, `llm::chat` with `Effort::Low`, restore, and a grounding check like slice 19.
- Input: the confirmed transcript (`source_segments`) plus the note if one exists. A dictation-only session works the same way.
- No diagnoses or medication. The prompt and the post-check filter ICD codes the same way `extract.rs:286` does.
- Audit `llm.summary`, no text.
- **Triggers** (autopilot, slice 31): after the transcript is confirmed, and again after the note is signed if the note changed a lot. Also "Neu zusammenfassen" by hand, and edit in place.
- **Timeline UI** is a new "Verlauf" tab on the patient page, beside the existing cards.
  - It's a vertical timeline, newest on top, grouped by Behandlungsfall.
  - Each session node shows date, number in the contingent (e.g. "Sitzung 9/12 KZT 1"), note status, the key points and a link to the session.
  - Other nodes: application/Bericht (status, signed/exported date), consents, cancellations.
  - Filter: nur Sitzungen / alles.
- The session page shows the summary in the finish step. The voice prep (slice 26) and assistant tools get the last 3 summaries as context, which replaces raw-note lookups where cheaper.

### Slice 31: Autopilot after the session + workflow cleanup
- Setting `autopilot`: on by default, with a toggle and a short explanation of what runs. Default transcription engine (local/cloud) moves to Settings.
- **Chain**, driven by the existing job worker (`crates/app/src/lib.rs` worker thread) plus a new job kind `post_confirm`:
  1. Recording stops → `enqueue_transcription` automatically in `stop_recording` (`lib.rs:716`), with the default engine. Diarization already follows automatically.
  2. Transcript ready → a notification/toast and a Heute item "Transkript prüfen". This stays a **human step** because confirming deletes the audio.
  3. On confirm, these run in order without clicks:
     - `create_session_note` (idempotent)
     - fact extraction into the draft (proposals, nothing applied)
     - the session summary (slice 30)

     Cloud calls go through the pseudonymizer as always. The redaction preview is stored per run and viewable as "Was wurde gesendet?" on the fact card. Per-patient terms still apply. Failures show on the card with "Erneut versuchen" and never block the note.
  4. Note → she accepts facts, edits and signs (human).
- **Bericht fällig**:
  - Count sessions per episode against the contingent (KZT 12/24, LZT by method). Check whether `episode.stage` and the application kind already give this; if not, add a session counter query.
  - From about 3 sessions before the limit, Heute/`due_items` shows "Umwandlungs-/Fortführungsantrag vorbereiten".
  - One click creates the application + Bericht, prefills diagnoses from the episode, and with autopilot on drafts all AI-allowed sections as proposals (existing `draftRun` per section, sequential, cancelable).
- **Cleanup**:
  - Audit moves into Settings → Sicherheit (the route stays, the nav entry goes; `app-shell.tsx:35-42,151-157`).
  - Vorlagen is hidden.
  - The Sitzungen status column is removed.
  - The session stepper (`SESSION_FLOW`, `lib/calendar.ts:144`) shows autopilot states: "läuft automatisch…" instead of a button.
- Tests:
  - core: the chain with fake whisper + fake llama
  - no extraction before confirm
  - autopilot off = today's behaviour
  - summary grounding

### Slice 32: Sitzungen + Berichte as work queues
- **Sitzungen**:
  - Grouped by day (Heute / diese Woche / älter).
  - Filter chips: "Transkript prüfen", "Notiz offen", "Unsigniert", "Neu exportieren", Alle.
  - Search by patient.
  - The row shows photo/avatar, patient, time, a mini stepper (5 dots from `flowStates`), and the summary's first point on hover.
- **Berichte**:
  - The default view is a board by status: Entwurf / Bereit zur Prüfung / Signiert / Exportiert. A table toggle stays.
  - The card shows patient, Antragsart, Verfahren, section progress (n/m filled) and the deadline when known.
  - Filters: Verfahren, Art.
  - `q.reports(status)` already exists (`reports/index.tsx`).
- Shared empty states with a next action (`EmptyState` in `components/bits.tsx`).

### Slice 33: AI costs, credits and key guides
- **OpenRouter**, `llm::key_info` → `GET /api/v1/key` with the normal key. It returns `usage`, `usage_daily/weekly/monthly`, `limit`, `limit_remaining` and `is_free_tier`.
- **Account balance**: `GET /api/v1/credits` needs a **management key**. Offer an optional "Management-Schlüssel (nur für Guthabenanzeige)" field in its own keychain entry `llm-openrouter-mgmt` → `total_credits - total_usage`. Without it, show "Guthaben im Browser ansehen" → https://openrouter.ai/settings/credits.
- **OpenAI** has no balance endpoint for normal keys. `/v1/organization/costs` needs an Admin key (org owner), and asking for one is a needless power grant, so don't do it. Show app-side spend (audit log) plus a link to https://platform.openai.com/settings/organization/billing/overview.
- **Requesty**: check its API for a balance endpoint during the slice; otherwise link only. → Checked: only the management API (admin key) has it, so link only (PLAN-AMENDMENTS §19 K1).
- **New Settings card "KI-Kosten & Guthaben"**:
  - balance/limit per provider (refresh button, cached 5 min)
  - this month / total
  - a **breakdown per model and per task** (extract/draft/summary/agent/STT/TTS/prep) from the audit log (extend `Store::llm_spend`, `store/mod.rs:295`, with GROUP BY model/action)
  - a sparkline over 30 days
  - a low-balance warning in the banner slot when below a threshold (default 2 $)
- **Key guides**: a step-by-step dialog per provider: Konto anlegen → Guthaben aufladen → Schlüssel erstellen → hier einfügen → Verbindung testen.
  - Links: signup, credits, keys (`key_url` already in `llm.rs:57-109`).
  - It also covers the ZDR note for OpenRouter.
  - It opens from the key field and from Hilfe (slice 34).
- Commands go next to `llm_spend` (`lib.rs:1702`), async via `blocking`, the key from `llm_key`. Audit `llm.balance` without amounts? Amounts aren't PII, but leave them out of the log anyway.

### Slice 34: Hilfe (in-app docs) + Rundgang + demo practice
- **Route `/hilfe`**:
  - Markdown articles bundled in `ui/src/content/hilfe/*.md`, offline.
  - Left nav: Erste Schritte, Sitzung aufnehmen, Autopilot, Notiz & Fakten, Bericht, Datenschutz & Pseudonymisierung, KI-Anbieter & Kosten, Datensicherung & Notfall-Kit, Tastenkürzel.
  - Searchable, and also through the Strg+K palette (`components/command-palette.tsx`).
  - A "?" link from cards to the matching article.
  - It links the online video.
- **Rundgang**: a small in-house coach-mark component (Radix Popover anchored to `data-tour="…"` attributes; no new dependency). It runs after the first setup and the Notfall-Kit, can be skipped, and can be restarted from Hilfe/Settings (`thera.tour` localStorage).
  - Steps: Heute → Kalender → Patient anlegen → Sitzung aufnehmen → Autopilot → Notiz signieren → Bericht → Einstellungen KI-Anbieter.
- **Demo-Praxis**: "Mit Beispieldaten ausprobieren" seeds 3 fictional patients, sessions with transcripts, summaries and a draft Bericht into a separate data dir, so it never mixes with real data. It can be left with one click. The tour and the video use it.

### Slice 35: Video explainer
- The deliverable in the repo is `docs/video/skript.md`: storyboard (~5 min, chapters matching the Hilfe articles), voice-over text in German, shot list. It's recorded on the Demo-Praxis only.
- Recording: the user (OBS) or a scripted playwright run over CDP plus screen capture; decided at the slice.
- Hosting: online (YouTube unlisted or website) and linked from `/hilfe` and the tour's last step. The URL lives in one constant.

## Appendix: GPT-image prompts

Prepend the existing `[style]` block from `docs/PLAN-PHASE3.md:250` (#F7F5F2 background, teal #2F7A78, German UI text, 1440×900, shadcn/Tailwind-realistic). Make 2–3 variants each. Use fictional faces only: illustrated or blurred portrait placeholders.

High-fidelity desktop app UI screenshot, 1440×900, Windows 11 app window, for "thera", a calm, private documentation app for German psychotherapists. Visual style: quiet, warm, trustworthy, clinical but not cold; soft warm-neutral background (#F7F5F2), white cards with subtle 1px borders and very soft shadows, muted teal accent (#2F7A78), a secondary warm sand accent, rounded corners 10px, generous whitespace, Inter-like sans-serif for UI and an elegant serif for document text, small lucide-style line icons. All UI text in German. No stock photos, no illustrations of people, no clutter, no gradients. Realistic and implementable with shadcn/ui + Tailwind.

10. **Patienten (Karten)**: [style] Patient overview "Patienten", toggle "Karten | Tabelle", search "Patient suchen…", button "Neuer Patient". Filter chips row: "Alle", "Erwachsene", "Kinder & Jugendliche", "weiblich", "männlich", "divers", "F3 Affektive Störungen", "F4 Angst-/Belastungsstörungen", "VT", "TP". Grid of 4 columns of white cards: round portrait placeholder (soft illustrated avatar, or initials circle for some), name "Anna K.", Chiffre "K120385" in monospace, "38 J. · weiblich", badges "VT" and "F32.1", line "Nächster Termin: Do 31.10., 10:00", small amber dot "2 offen". Sort select "Nächster Termin".
11. **Patient: Verlauf (timeline)**: [style] Patient page header with portrait, name, Chiffre, tabs "Übersicht | Verlauf | Dokumente". Tab "Verlauf": vertical timeline, left date column, teal line with nodes. Group header "Behandlungsfall VT · KZT 1 · Sitzung 9 von 12". Session node "Di 22.10.2026 · Sitzung 9" with 4 short bullet key points marked "KI-Zusammenfassung" and a small "bearbeiten" link, badge "Notiz signiert". Application node "Umwandlungsantrag · Bericht Entwurf". Consent node "Einwilligung Tonaufnahme erteilt". Filter "Nur Sitzungen".
12. **Nach der Sitzung (Autopilot)**: [style] Session page with stepper "Vorbereiten · Aufnahme · Prüfen · Notiz · Abschluss". Card "Autopilot" with checklist: "Transkription ✓ (lokal, 6 Min.)", "Sprecher getrennt ✓", "Transkript prüfen – Ihre Aufgabe" highlighted with button "Prüfen und bestätigen", greyed next items "Notiz anlegen", "Fakten vorschlagen", "Zusammenfassung" with "läuft automatisch nach Bestätigung". Small link "Was wird gesendet? (pseudonymisiert)".
13. **Sitzungen (Arbeitsliste)**: [style] "Sitzungen" with filter chips "Transkript prüfen 2", "Notiz offen 3", "Unsigniert 1", "Neu exportieren", "Alle". List grouped "Heute", "Diese Woche", "Älter". Rows: avatar, patient name, time, five small step dots (filled teal / outlined), one-line summary in muted text, button "Weiter".
14. **Berichte (Board)**: [style] "Berichte" with toggle "Board | Tabelle". Four columns "Entwurf", "Bereit zur Prüfung", "Signiert", "Exportiert". Cards: patient name, "Umwandlungsantrag KZT→LZT", badge "VT", progress bar "5 von 7 Abschnitten", "fällig ca. 12.11.". A banner card on top: "Umwandlungsantrag für Anna K. bald fällig (Sitzung 10 von 12) – Bericht vorbereiten".
15. **KI-Kosten & Guthaben**: [style] Settings page, card "KI-Kosten & Guthaben". Two provider tiles: "OpenRouter – Guthaben 8,42 $ · diesen Monat 1,37 $" with button "Aufladen ↗", "OpenAI (Sprache) – nur im Browser einsehbar ↗". Bar list "Nach Aufgabe": Fakten 0,41 $, Bericht 0,62 $, Zusammenfassung 0,08 $, Assistent 0,19 $, Sprache 0,07 $. Table "Nach Modell" with requests and cost. Small 30-day sparkline. Warning row "Warnen unter 2,00 $".
16. **Schlüssel einrichten (Guide)**: [style] Dialog "OpenRouter einrichten", vertical stepper with 5 steps: "Konto anlegen ↗", "Guthaben aufladen ↗ (z. B. 10 $)", "API-Schlüssel erstellen ↗", "Schlüssel hier einfügen" with password field, "Verbindung testen" button with green check. Side note box "Datenschutz: nur pseudonymisierte Texte, Zero Data Retention aktiv".
17. **Rundgang**: [style] The "Heute" screen dimmed with a spotlight on the sidebar entry "Kalender" and a white coach-mark popover: "2 von 8 · Kalender – hier planen Sie Sitzungen und Serien.", buttons "Zurück", "Weiter", link "Rundgang beenden".
18. **Hilfe**: [style] "Hilfe" page, left nav with article titles (Erste Schritte, Sitzung aufnehmen, Autopilot, Bericht schreiben, Datenschutz, KI-Anbieter & Kosten, Datensicherung), search "Hilfe durchsuchen…", main article with heading, numbered steps, a screenshot frame and a video card "Video: thera in 5 Minuten ▶".

## Critical files
- core:
  - `crates/core/src/domain.rs`
  - `store/migrations/` (new V-next)
  - `store/mod.rs` (`llm_spend`, `due_items` in `store/calendar.rs`)
  - `audio.rs` (pattern for `media`)
  - `backup.rs`
  - `extract.rs` (pattern for `summary.rs`)
  - `llm.rs` (key_info/credits)
- app: `crates/app/src/lib.rs` (`stop_recording` 716, worker, `llm_spend` 1702, handler list)
- ui:
  - `routes/patients/index.tsx`, `patients/$patientId.tsx`
  - `sessions/index.tsx`, `sessions/$sessionId.tsx`
  - `reports/index.tsx`, `components/reports-table.tsx`
  - `components/app-shell.tsx`, `patient-form.tsx`, `model-picker.tsx`, `llm-settings.tsx`
  - `lib/calendar.ts`, `lib/locales/de.ts`
  - new `routes/hilfe/`, `components/tour.tsx`
- docs: new `docs/PLAN-PHASE4.md`, HANDOFF pointer.

## Verification (per slice)
- `cargo test -p thera-core` (new migration, media encryption round trip + erase + backup, summary grounding, autopilot chain with fake whisper/llama, spend breakdown), `npm run typecheck && npm test`.
- E2E in the debug app with an isolated `THERA_DATA_DIR`/`THERA_KEYRING_USER`, driven over CDP (see memory thera-dev-env):
  - Photo: upload → card shows it, file on disk not readable as an image, backup/restore keeps it, erase removes it.
  - Autopilot: the TTS dictation/recording stops → transcription starts by itself → confirm → note, facts and summary appear. Mock endpoint log shows only pseudonymized text.
  - Credits: a Node mock for `/api/v1/key` and `/api/v1/credits` (403 without a management key → fallback link).
  - Tour runs on a fresh data dir, can be skipped, and restarts from Hilfe.
- After each slice: commit, push main, `npm run app:build`.
