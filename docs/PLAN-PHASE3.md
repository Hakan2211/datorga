# thera Phase 3 plan (approved 2026-10-28): cloud, calendar, new UX, document editor, agent, voice prep

## Context

Phase 1 (slices 1–10) and Phase 2 (slices 11–15) are done: encrypted local store, capture, local whisper, Sitzungsnotiz, fact extraction, Bericht drafting and the eval set. The user now wants six things:
1. use OpenRouter/Requesty for real, with a cloud transcription fallback
2. a calendar
3. a new UI/UX
4. a Word-like editor with preview and a voice mode
5. voice session prep with context from old sessions
6. an agentic chat that works across the app

What already exists (from exploration):
- **LLM** (`crates/core/src/llm.rs`): plain functions with blocking `ureq`. `chat()` takes only `Pseudonymized<String>`. No streaming, no tool calling. Only tested against a mock.
- **STT** (`crates/core/src/stt.rs`): hard-wired to `WhisperCli`, batch only, no engine abstraction. `transcript.engine`/`transcript.model` columns already exist.
- **Sessions**: `session.scheduled_at` and `status` (default `planned`) exist. No date-range query, no index, no update/reschedule command.
- **Editor**: TipTap is already used, one `RichField` per section (`ui/src/components/rich-field.tsx`), in `ui/src/routes/reports/$reportId.tsx`. PDF/DOCX export is in the backend (`render/`). There's no preview.
- **UI**: shadcn new-york, Tailwind v4 oklch tokens in `ui/src/styles.css`, light only. The shell is `components/app-shell.tsx` with a module registry, and a greyed-out "Kalender" is already there. State lives in TanStack Query with polling; there's no `listen()`.
- **Context sources** for prep and agent: `Store::episode_facts`, applied `note_fact`, signed notes, Berichte. Pseudonymizer: `Pseudonymizer::with_map` keeps tokens stable across texts. The "Pat." rule comes from slice 14 (`core::draft`).

User decisions (2026-10-28):
- ~~**Cloud STT only for the therapist's own voice**~~ **Changed by the user (2026-10-28):** dictations may use the cloud; **session recordings too, but only with a separate patient consent "Cloud-Transkription"** (own versioned text and form; the recording consent still promises local processing and stays unchanged). The engine is chosen per recording in the session, preselected from Settings.
- **Voice prep uses OpenAI Realtime directly**: a separate OpenAI key, WebRTC.
- **The agent proposes, the therapist confirms**: it never signs, exports or sets diagnoses.
- **The calendar is in-app only**, with optional .ics export.

`docs/OPEN-ITEMS.md` (loose ends after Phase 2) is folded in. Each item is marked **[OI]** where it lands, and the table at the end maps every item to its slice.

Each slice below follows the house pattern: core logic plus tests in `crates/core`, thin commands in `crates/app/src/lib.rs`, ts-rs bindings, `ui/src/lib/api.ts` wrappers, an E2E in the real app over CDP, and HANDOFF.md updated.

---

## Phase A: cloud providers for real (slices 16–18)

**16. Real-key test + streaming**
- **[OI]** Run OpenRouter and Requesty once with real keys: ping, model list, one extraction and one draft, with egress checked in the audit log. **[OI]** Run one cloud model in the eval (`THERA_EVAL_API_KEY=… cargo run -p eval -- --provider openrouter --model <id>`). This is the cloud baseline for slice 19. Note the results in HANDOFF.
- `llm.rs`: add `chat_stream()` over SSE (`stream: true`, a line reader on ureq, a callback per delta, cancel flag), keeping the same `Pseudonymized` input type. Phases D and E need it.
- **[OI] Cancel button for long extractions**: extraction and drafting run through the same cancel flag (per chunk, and mid-stream). The UI gets "Abbrechen" in `fact-draft.tsx`/`report-draft.tsx`, and partial facts from finished chunks are kept as proposals.
- App: stream deltas to the UI with Tauri events (`app.emit("llm:delta", {request_id, text})`). This is the first `listen()` in `api.ts`. Polling stays everywhere else.
- Tests: extend `crates/core/tests/llm_http.rs` with an SSE mock (deltas joined, cancel, error mid-stream).

**17. STT engine abstraction + cloud dictation fallback**
- `core::stt`: `enum SttEngine { Local(WhisperCli), Cloud(CloudStt) }` behind one `transcribe(...) -> Vec<Segment>`. The silence gate and hallucination filter run for both.
- `CloudStt`: OpenAI-compatible `POST {base}/audio/transcriptions`, multipart WAV, `language=de`, `response_format=verbose_json` for segments (falls back to a single segment). Providers:
  - OpenRouter: `/api/v1/audio/transcriptions`, models filtered by `output_modalities=transcription` (e.g. `openai/whisper-large-v3`, `openai/gpt-4o-transcribe`)
  - Requesty: only if their router offers it (check at build time); otherwise hide it
  - a custom endpoint
  - The key is reused from the LLM keychain entry.
- Split long WAVs at silence into pieces under 20 MB.
- **Enforcement point** in `transcribe_job` (app lib.rs ~l.783): cloud is allowed only when `session_source.kind = dictation` (and for the new voice-mode sources from slice 24). For `session_audio` cloud is unreachable: a type-level or explicit check, plus a test.
- Settings: in the "Transkription" card, an engine choice "Lokal" / "Cloud (nur eigene Diktate)" / "Lokal, Cloud als Ausweichlösung". It explains plainly that the audio leaves the machine and can't be pseudonymized. "Ausweichlösung" triggers when there's no local model or the hardware probe says it's too slow (reuse `local.rs` hardware info).
- Audit: `transcription.cloud {provider, model, seconds, ok}` with no text. `transcript.engine = "cloud:<provider>"`.
- **[OI] whisper-cli into the job object**: reuse the kill-on-close job object from `local.rs` (llama-server) for whisper-cli, and later for whisper-server (slice 24), so a crashed app can't leave it running.
- **[OI] Her laptop specs**: the "Ausweichlösung" uses the hardware probe. Once she sends specs, choose the default engine and the GPU vs. CPU whisper build. Until then, CPU build + local default.

**18. Cloud TTS + OpenAI provider entry** (done 2026-10-28, see HANDOFF)
- `ProviderId::Openai` with a keychain entry `llm-openai`, only for Realtime (slice 26) and optionally TTS. Settings shows a US-provider warning plus a hint to use an EU data-residency project.
- `core::tts` (small): OpenRouter `/audio/speech` for reading answers aloud in the agent chat (slice 25). Optional and off by default.

## Phase A2: drafting quality (slice 19), measured with the eval

**19. Better drafts** (done 2026-10-28, see HANDOFF "Built (slice 19)": local 8B draft precision 52 % → 83 %, forbidden 12 → 0, recall 93 % → 75 %; the cloud model is not re-measured yet. Deviations: tags use the Erst-/Umwandlungsbericht sections only, Fortführung sections get all facts of the chosen sessions; the fallback is per fact (untagged facts go to every section, a section without facts is skipped) plus an "Alle Fakten anbieten" switch; the post-check drops prognosis/goal/setting sentences not backed by a cited fact, in every section.) (all **[OI]**; measured before and after with `cargo run -p eval`, local 8B + the cloud model from slice 16; the user re-runs the 14B themselves)
- **Per-fact Bericht section tags**: extraction also returns 1–3 Bericht section keys per fact (template section keys, validated against the template). Migration: `note_fact_section(fact_id, section_key, source: ai|therapist)`. In the fact review (`fact-draft.tsx`) tags appear as chips she can add or remove. Drafting a section uses only facts tagged for it (fallback: all facts, with a warning). The eval gets a section-precision metric per tag (the golden set already has Bericht sections per fact).
- **Existing section text as context on "Anhängen"**: `draft_run` with mode append sends the section's current text (pseudonymized, "Pat." rule) with the instruction "nicht wiederholen, nur ergänzen".
- **Choose sessions for a Fortführungsbericht**: `draft_preview`/`draft_run` take `session_ids`. The draft dialog shows a session checklist (default: sessions since the last application), and the selection is stored in the draft provenance.
- **Merge duplicate facts across chunks**: after extraction, a local merge pass (normalized text similarity plus overlapping citations) folds duplicates into one fact that keeps all citations. It's deterministic, not an LLM call, and tested with eval fixtures that have repeated statements.
- **Invented prognoses/goals** (8B): add a prompt rule plus a post-check that drops sentences without a fact citation in the sections Prognose/Behandlungsziele, with an eval case for it.

## Phase B: design system + new shell (slices 20–21)

**Step 0 (user, now):** generate screens from the prompts in the appendix with GPT image. Pick a direction; I then derive tokens from it.

**20. Design system**
- `ui/src/styles.css`:
  - new tokens from the chosen image (colour, radius, shadow scale)
  - real `.dark` values plus a theme toggle (setting `theme`: hell/dunkel/system)
  - a self-hosted font (e.g. Inter or Geist via `@fontsource`, offline), with Libertinus kept for document text
- Add the missing shadcn components: tabs, dropdown-menu, popover, calendar, sheet, progress, switch, command, resizable, toggle-group, kbd, sonner already exists.
- Shared patterns in `components/bits.tsx`: EmptyState, StatusPill, SectionCard, StickyActionBar.

**21. New shell + workflow guidance**
- `app-shell.tsx`:
  - a collapsible icon sidebar
  - a top bar with global search / command palette (`⌘K`/`Strg+K`: patients, sessions, actions such as "Neue Sitzung", "Diktieren")
  - a right-side panel slot (used later by the agent chat)
  - the recording status as a persistent pill
- **Session as a guided flow**: the session page becomes a stepper "Vorbereiten → Aufnehmen/Diktieren → Abschrift prüfen → Notiz → Abschließen". Each step shows the next action as the only primary button, and the existing cards move into steps. Same for a Bericht: "Fakten prüfen → Entwurf → Bearbeiten → Prüfen → Exportieren".
- **[OI] Audio playback in the transcript viewer** (step "Abschrift prüfen"): a new command `source_audio_wav(source_id, from_ms, to_ms)` decrypts the requested range in memory and returns WAV bytes, played as a blob URL. Nothing is written to disk. Clicking a segment's timestamp plays that segment, with play/pause in the viewer. It only works until the transcript is confirmed, because the audio is deleted then.
- The dashboard is replaced by **"Heute"** (slice 22).

## Phase C: calendar (slice 22)

**22. Calendar + "Heute"**
- Migration (calendar):
  - `session.planned_minutes`, `session.cancelled_reason`, index on `COALESCE(scheduled_at, started_at)`
  - a `task` table (id, date, time NULL, title, done, patient_id NULL, session_id NULL, created_at) for day planning and to-dos
  - optional `day_note` (date PK, body: free planning text)
- Store: `sessions_between(from, to)` (joined with patient Chiffre, note status, transcript state), `update_session` (reschedule, duration, cancel), task CRUD, and `due_items(from, to)` = derived items:
  - unsigned notes older than N days
  - planned sessions without prep
  - Berichte in draft for an application
- Commands: `calendar_range`, `session_update`, `session_cancel`, `task_*`, `day_note_get/set`, `calendar_export_ics(path)` (Chiffre only, no names).
- UI: `routes/calendar.tsx`:
  - Day / Week / Month (custom grid on date-fns; no heavy calendar lib)
  - drag to reschedule in the week view
  - click a free slot to create a session
  - colour by state (geplant / durchgeführt / Notiz offen / signiert)
- `routes/index.tsx` becomes **"Heute"**:
  - a timeline of today's sessions, each with its next step (from the slice 21 stepper)
  - the to-do list
  - due items
  - the day's planning note
  - "Tag vorbereiten" (later opens the agent with a day brief)
- Recurring appointments: a "Serie anlegen" helper that creates N sessions (weekly / every two weeks). No RRULE engine.

## Phase D: document editor, preview, voice mode (slices 23–24)

**23. Document mode + preview** (done 2026-10-28, see HANDOFF "Built (slice 23)". Deviation: the preview shows the compiled pages as PNG (`typst-render`) instead of a blob PDF in WebView2's viewer, so re-renders don't flicker or jump back to page 1. Justified is now the default alignment in the DOCX too, matching the PDF.)
- A toggle in the editor: "Abschnitte" (today's view) | "Dokument" (Word-like).
- Document mode: one TipTap editor over the whole report, with a custom `section` node per template section (heading not deletable, body editable). It is mapped back to `report_section` rows on save, so storage, drafting, citations and sign-off stay unchanged.
- An A4 page look (width, margins, print font), plus more toolbar items:
  - headings inside a section
  - underline
  - undo/redo
  - text alignment, only where `render/richtext.rs` + the DOCX exporter support it (extend both together)
- Preview: a split view that renders through the existing typst pipeline into bytes (new command `report_preview_pdf(id) -> Vec<u8>`), shown as a blob URL in WebView2's PDF viewer, debounced re-render. What you see is exactly the export.

**24. Voice mode + Diktat-Werkstatt** (done 2026-10-28, see HANDOFF "Built (slice 24)". Deviations: no `voice_note` source row (the Werkstatt also exists per Bericht; the `dictation.*` audit entry records it); ~3.5 s per sentence on CPU instead of 1–3 s (shorter encoder context hallucinated); the newest piece shows as a grey "…" until its text arrives, not as interim text.)
- **Near-live dictation** (therapist voice only):
  - Local: a whisper.cpp `whisper-server` sidecar (same release as whisper-cli, add it to `fetch-whisper.ps1`). It's fed utterance-sized pieces split at silence from the capture crate (VAD on the level meter: about 600 ms of silence ends a piece).
  - Cloud: the same pieces go to `CloudStt` (slice 17), if that's allowed.
  - Text appears within about 1–3 s per piece.
- New `session_source.kind = 'voice_note'`: audio is never written to disk (in memory only), and pieces are dropped after transcription.
- Editor voice mode: a mic button inserts text at the cursor in the active section. The newest piece is shown in grey until it's final, and it stays editable.
- **Diktat-Werkstatt** (`/sessions/$id/werkstatt` and per Bericht): a big free-text area where she thinks out loud and the text appears live. Then "Entwurf erstellen" sends the pseudonymized text plus the template sections to the LLM (streaming), which returns a proposal per section. Proposals are applied with the existing proposal UI (`components/report-draft.tsx` replace/append). The MDR rule stays: no diagnoses.

## Phase E: agentic chat (slice 25)

**25. Agent with tools, proposals only**
- `llm.rs`:
  - `tools` in the request
  - parse `tool_calls`
  - a `Role::Tool` message
  - streaming tool-call deltas
- Tool-calling capability per model comes from `/models` (`supported_parameters`); the UI warns for models without it.
- `core::agent`:
  - The loop runs in Rust (max N steps).
  - **Read tools** (results pseudonymized with one `TokenMap` per chat thread through `Pseudonymizer::with_map`, dictionary = all patients + `pseudo_term`):
    - `search_patients`, `get_patient_overview`, `list_sessions(range)`, `get_session_note`, `get_facts(episode)`, `get_report`, `calendar_day`, `due_items`
  - **Write tools** never write. Each one creates an `agent_proposal` row (kind, target, payload JSON, status), e.g.:
    - `propose_session`, `propose_reschedule`, `propose_task`, `propose_section_text`, `propose_note_text`, `propose_day_plan`
  - Hard deny-list: sign, export, erase, consent, diagnoses fields, settings.
  - Answers are restored locally (`TokenMap::restore`) before display.
- Migration (chat): `chat_thread` (id, scope: global | patient | session, ref_id, created), `chat_message` (role, content restored and stored encrypted in the DB like everything else, tool name/args), `agent_proposal`.
- UI:
  - a right-side panel (slot from slice 21) plus a full page `/assistant`
  - streaming answer
  - "Was ich getan habe" steps (tool calls as collapsible lines in German)
  - proposal cards with Übernehmen / Verwerfen; applying runs the normal store command, and the audit records `agent.apply {kind}`
  - Voice input with the slice 24 dictation; optional read-aloud with slice 18 TTS.
- Scope: when opened from a patient or session page, the thread is scoped there and the tools are limited to that patient.

## Phase F: voice session prep via OpenAI Realtime (slice 26)

**26. Vorbereitung per Sprache** (done 2026-10-28, ahead of 19–25, on **OpenAI GPT-Live** `gpt-live-1` instead of `gpt-realtime`: Rust posts the SDP offer to `/v1/live/sessions`, no ephemeral secret; see HANDOFF)
- Context builder `core::context::prep_brief(patient_id, session_id)`, shared with the agent:
  - the last N signed Sitzungsnotizen (sections Verlauf/Vereinbarungen)
  - applied facts of the episode
  - the latest Bericht summary
  - open Vereinbarungen
  - The patient is written as "Pat.", sentences naming third parties are dropped (slice 14 rule), and the result is pseudonymized and length-capped. The therapist sees the exact text before starting (same preview pattern as `extract_preview`).
- Rust mints an **ephemeral client secret** (`POST https://api.openai.com/v1/realtime/client_secrets`, model `gpt-realtime` current version, German instructions plus the brief, voice, server VAD). The real key never reaches JS.
- UI `components/voice-prep.tsx`: WebRTC in WebView2 (`RTCPeerConnection`, `getUserMedia` on the chosen mic), live captions of both sides from the data channel, interrupt, end.
- Result: on end, the transcript of both sides (restored locally) goes to the LLM, which returns a short prep sheet: themes, open Vereinbarungen, questions for today. It's stored in a new `session_prep` table (new migration), shown on the session page and in "Heute", and editable.
- Consent/egress: opt-in in Settings with a clear note that her voice and the pseudonymized brief go to OpenAI (US; EU residency project recommended). Audit `prep.realtime {model, seconds}`. Never available inside a session recording.

## Phase G: speaker separation (slice 27, can move up)

**27. [OI] Who is speaking: therapist or patient**
- Local diarization of session recordings, as a sidecar or ONNX in Rust (e.g. sherpa-onnx speaker segmentation + embedding models, hash-checked download like whisper). It runs as part of the transcription job and fills the existing `transcript_segment.speaker_label`.
- Therapist voice profile: an embedding enrolled from her confirmed dictations, stored encrypted in the DB. Segments get labelled "Therapeut/in" / "Patient/in", and an unsure label stays "Sprecher 2".
- Transcript viewer: speaker chips, "Sprecher tauschen" per recording, and correcting a single segment.
- `source_segments` returns the role per segment. Extraction gets "T:"/"P:" prefixes, which fixes the gender and attribution errors noted in OPEN-ITEMS. The eval gets a two-speaker fixture.
- It's placed last because it's the riskiest (model quality on German, laptop CPU). If the slice 19 eval shows attribution errors dominate, do it right after slice 19.

---

## Order and rationale

A (16→17→18) → A2 (19) → B (20→21) → C (22) → D (23→24) → E (25) → F (26) → G (27).
- A first: the real-key test unblocks everything, and streaming is needed by D, E and F.
- A2 early: better drafts feed everything after it (Werkstatt, agent proposals, prep context), and the eval gives a before/after number.
- B before C: the calendar becomes the new home and should be built once, in the new style.
- E before F: F reuses the context builder and the proposal pattern.

## OPEN-ITEMS.md mapping

| Open item | Where |
|---|---|
| Re-run Qwen3 14B | user, before slice 19 (baseline) |
| Measure one cloud model; real-key OpenRouter/Requesty test | slice 16 |
| Per-fact Bericht section tags | slice 19 |
| Existing text as context on "Anhängen" | slice 19 |
| Choose sessions for a Fortführungsbericht | slice 19 |
| Merge duplicate facts across chunks | slice 19 |
| Cancel button for long extractions | slice 16 |
| Audio playback in the transcript viewer | slice 21 |
| whisper-cli into the job object | slice 17 |
| Speaker separation | slice 27 |
| Laptop specs (GPU/CPU, local vs. cloud default) | waiting on her; used in slice 17 |
| Vorlagen module (old Berichte/Sitzungen upload) | stays deferred until her material arrives; the new shell keeps a "Vorlagen" slot |
| Chiffre format | waiting on her; a one-line change in the Chiffre suggestion |
| Legal review of consent text | before real patients, **now also covering**: cloud dictation (her voice only), OpenAI Realtime prep, US intermediaries. Add these to the processing description (Verarbeitungsverzeichnis) she keeps. |
| Restore test on her machine | before real patients (unchanged) |
| Open decision "drafting quality vs. Vorlagen" | resolved by this plan: drafting quality now (slice 19), Vorlagen when the material is there |

## Critical files

- `crates/core/src/llm.rs`, `stt.rs`, new `tts.rs`, `agent.rs`, `context.rs`
- `crates/core/src/extract.rs`, `draft.rs`, `crates/eval` (slice 19)
- `crates/core/src/store/mod.rs` + new migrations (section tags, calendar, chat/proposals, prep)
- `crates/core/src/render/richtext.rs`, `render/mod.rs` (alignment/headings, preview bytes)
- `crates/capture/src/lib.rs` (utterance splitting for live dictation)
- `crates/app/src/lib.rs` (commands, events, STT enforcement point in `transcribe_job`)
- `scripts/fetch-whisper.ps1` (+ whisper-server)
- `ui/src/styles.css`, `ui/src/components/app-shell.tsx`, `rich-field.tsx`, `report-draft.tsx`, `llm-settings.tsx`
- `ui/src/routes/index.tsx` (→ Heute), new `calendar.tsx`, `assistant.tsx`, `sessions/$sessionId.tsx` (stepper), `reports/$reportId.tsx` (document mode)

## Verification (per slice, house pattern)

- `cargo test -p thera-core` (new unit tests: SSE parsing, cloud STT mock with multipart check, **a test that `session_audio` can never reach CloudStt**, calendar range queries, agent loop against a scripted mock with tool calls, proposals never mutating data, deny-list), plus `npm run typecheck && npm test` in `ui/`.
- E2E in the real debug app over CDP (isolated `THERA_DATA_DIR`/`THERA_KEYRING_USER`):
  - real-key OpenRouter run with the audit checked for text-free entries
  - TTS dictation through cloud STT
  - calendar drag-reschedule persisted
  - document-mode edit equals section rows, and the preview PDF matches the export (`pdftotext`)
  - live dictation into the editor
  - agent proposal applied and audited
  - Realtime prep against a real key (manual, with the user)
- Egress check for every cloud feature: capture the outgoing body with the mock/local proxy and assert that no name or birth date appears in it. The exception is audio by design, and only for therapist-voice kinds.
- Update `docs/HANDOFF.md` and `docs/PLAN-AMENDMENTS.md` (§10 Phase 3 decisions) after each slice.

---

## Appendix: prompts for UI images (GPT image generation)

Use them one by one, same style block each time. Generate 2–3 variants per screen and pick one direction before slice 20.

**Shared style block (prepend to every prompt):**
> High-fidelity desktop app UI screenshot, 1440×900, Windows 11 app window, for "thera", a calm, private documentation app for German psychotherapists. Visual style: quiet, warm, trustworthy, clinical but not cold; soft warm-neutral background (#F7F5F2), white cards with subtle 1px borders and very soft shadows, muted teal accent (#2F7A78), a secondary warm sand accent, rounded corners 10px, generous whitespace, Inter-like sans-serif for UI and an elegant serif for document text, small lucide-style line icons. All UI text in German. No stock photos, no illustrations of people, no clutter, no gradients. Realistic and implementable with shadcn/ui + Tailwind.

1. **Heute (home)**
> [style] Screen "Heute, Mittwoch 28. Oktober". Left: a slim collapsible icon sidebar (Heute, Kalender, Patienten, Sitzungen, Berichte, Assistent; bottom: Einstellungen, a small lock badge "Verschlüsselt"). Top bar with a search field "Suchen oder Befehl… Strg K". Main area: a vertical timeline of today's sessions (08:00–17:00) as cards with patient code (e.g. "M150385"), session number "Sitzung 12", a status pill (Geplant / Notiz offen / Signiert) and one clear next-step button per card ("Vorbereiten", "Notiz schreiben"). Right column: "Aufgaben" checklist, "Fällig" list (2 Notizen unsigniert, Bericht Fortführung in Arbeit), and a "Tagesplanung" text card with a microphone button.

2. **Kalender (week)**
> [style] Calendar week view "KW 44, 26.–30. Oktober", toggles Tag / Woche / Monat, button "Neue Sitzung". Time grid 07:00–19:00, session blocks coloured by status (teal = durchgeführt, outline = geplant, amber dot = Notiz offen), patient codes only (no names). A small popover on one block showing "Sitzung 12 · 50 Min · Vorbereitung vorhanden" with buttons "Öffnen" and "Verschieben". Today's column subtly highlighted.

3. **Sitzung as guided flow**
> [style] Session page header "Sitzung 12 · M150385 · 28.10.2026 10:00". Below: a horizontal stepper with 5 steps (Vorbereiten ✓, Aufnehmen, Abschrift prüfen, Notiz, Abschließen), step 2 active. Large central recording card: big round record button, timer 00:42:13, live level meter, microphone picker, a consent badge "Einwilligung liegt vor". Side panel "Vorbereitung" with 3 bullet themes and open agreements from last session. A single primary button at the bottom right "Aufnahme beenden".

4. **Bericht editor, document mode with preview**
> [style] Report editor "Bericht an den Gutachter – Fortführungsantrag". Toggle at top "Abschnitte | Dokument" (Dokument active) and "Vorschau" switch on. Left half: Word-like editor on an A4 white page with serif text, section headings (1. Relevante soziodemographische Daten, 2. Symptomatik und psychischer Befund, …), a slim formatting toolbar (Fett, Kursiv, Unterstrichen, Überschrift, Listen, Rückgängig) and a floating microphone pill "Diktieren". Right half: PDF preview of the same page. Bottom bar: length meter "1,8 von 2 Seiten", "Alle Änderungen gespeichert", button "Zur Prüfung".

5. **Diktat-Werkstatt (voice mode)**
> [style] Focused full-width writing space "Diktat-Werkstatt – Sitzung 12". A large calm text area where spoken text appears live; the last sentence shown in lighter grey (still being recognised). Bottom center: big microphone button with a soft pulsing ring and waveform, label "Ich höre zu…". Right panel "Entwurf erstellen" listing the note sections (Inhalt und Verlauf, Interventionen, Vereinbarungen) with generated proposal cards and buttons "Übernehmen" / "Verwerfen". Small privacy hint: "Nur Ihre Stimme · Text wird pseudonymisiert".

6. **Assistent (agent chat panel)**
> [style] Main app on the left (calendar day view) and a docked right panel "Assistent" (420px). Chat in German: user asks "Bereite meinen Tag vor und plane die Notizen von gestern ein." Assistant reply with a collapsible list "Was ich getan habe" (3 steps with small icons: Kalender gelesen, 2 Notizen gefunden, Tagesplan erstellt) and two proposal cards: "Aufgabe anlegen: Notiz M150385 abschließen, 12:30" and "Tagesplan übernehmen", each with "Übernehmen" and "Verwerfen". Input at bottom with microphone and send buttons, scope chip "Alle Patienten".

7. **Vorbereitung per Sprache (voice prep)**
> [style] Calm full-screen voice mode "Vorbereitung · M150385 · Sitzung 12". Center: a large soft animated orb in teal indicating the AI is speaking, live captions below in two styles (Therapeutin / Assistent). Left card "Kontext aus früheren Sitzungen" with 4 short bullets and a link "Gesendeten Text ansehen". Bottom controls: Mikrofon stumm, Unterbrechen, Beenden (red outline). Top hint "OpenAI Realtime · Ihre Stimme und pseudonymisierter Kontext verlassen das Gerät".

8. **Dark mode variant** (after picking a direction)
> Same as prompt 1 but in a dark theme: deep warm charcoal background (#1C1B1A), cards #252321, teal accent slightly brighter, same layout and German text.
