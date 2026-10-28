# thera: session handoff (2026-10-27, Phase 2: slices 11–15 done)

Start a new session with: *"Read docs/HANDOFF.md and continue with the next step."*

**Phase 3 plan approved (2026-10-28):** `docs/PLAN-PHASE3.md`, slices 16–27: cloud for real + cloud dictation STT, drafting quality (folds in docs/OPEN-ITEMS.md), design system + new shell, calendar/"Heute", document editor + voice mode, agent chat (proposals only), voice prep via OpenAI Realtime, speaker separation. **Slice 16 in progress (2026-10-28):** real OpenRouter key tested via the eval (9 models, 0.57 $; results and the decisions no batch / reasoning per task / tier picker / no Mistral in PLAN-AMENDMENTS §10). Done: `llm::Effort` + per-model mapping (`core::catalog`), `max_tokens` 8192, cost + reasoning tokens in `Completion` and the audit log, `eval --max-cost`, Settings tier picker (`components/model-picker.tsx`, E2E-checked), spend display, Mistral removed. Still open in slice 16: SSE streaming, cancel for extraction/drafting, a Requesty real-key test (no key yet), an E2E extraction + draft in the app against the real key.

**State at end of session (2026-10-27):** Phase 1 is done (slices 1–10). Phase 2 has started. The user decided (PLAN-AMENDMENTS §9): OpenRouter/Requesty + own endpoint, chosen model, the therapist's own key, only pseudonymized text leaves the machine, local models with hardware-based recommendation; ChatGPT/OpenCode subscription deferred. **Slice 11 (LLM provider layer) is done** and verified in the real app against a mock endpoint. It isn't tested against a real OpenRouter/Requesty key yet (none on the dev box). **Slice 12 (local models) is done** and verified with the real llama-server on the dev box GPU. **Slice 13 (fact extraction → Sitzungsnotiz draft) is done**, verified in the real app against a mock endpoint (egress checked) and the real Qwen3 8B. **Slice 14 (Bericht drafting per section from applied facts) is done**, verified in the real app against a mock endpoint (egress checked) and the real Qwen3 8B. **Slice 15 (eval set) is done**: `cargo run -p eval`, Qwen3 8B measured (14B only on the old draft prompt, no cloud model yet). Next step: see Next steps 12 (14B re-run + cloud model, then per-fact Bericht section tags).

On the next start of the dev-box app you'll see **"Notfall-Kit fehlt"**: the DB predates the kit, so it was never confirmed. Create a new key there and save/print the kit.

Test data in the dev DB: a session with a TTS dictation, a signed note ("E2E Test") and two Nachträge. Nachträge can't be deleted; only erasing the test patient removes them. The 12:03 session's two recordings belong to the user and are untranscribed: don't confirm transcripts there (that deletes the audio).

## Accomplished

**Planning**
- Merged three sources into one plan: the opencode master plan (`docs/PLAN.md`), the architecture doc (27 Oct), and the KBV **PTV 3** guideline.
- Key correction: a Bericht an den Gutachter is written **per application** (Erst-/Umwandlungs-/Fortführungsantrag) from several sessions. Two document types: Sitzungsnotiz (per session) + Bericht.
- All corrections, decisions, and the Templates & Style subsystem (upload old Berichte, own forms, style notes) are in `docs/PLAN-AMENDMENTS.md`.

**Built (slice 1, Phase 1)**
- `crates/core` (Rust, no Tauri):
  - SQLCipher store with migrations
  - master key in the OS keychain + printable recovery key
  - hash-chained audit log (PII-free, tamper test)
  - PTV 3 templates as YAML, filtered by VT/TP/AP, adult/KJP and application kind
  - PII/Chiffre checklist
  - PDF (typst) + DOCX export
  - 51 tests + compile-fail doctest
- `crates/app`: Tauri 2 shell, thin commands over core.
- `ui/`: TanStack Start (SPA) + shadcn/ui + Tailwind + TipTap, German UI:
  - patients (Chiffre suggestion), treatment cases, applications, sessions list
  - **Bericht editor**: guideline hints, manual diagnoses, autosave, length meter, sign-off gate, PDF/DOCX export
  - audit log, settings, template view
- Verified end-to-end in the real app (CDP-driven test). Fixed an autosave bug that test found.
- Desktop shortcut `thera` → `target\release\thera-app.exe`. Repo pushed to github.com/Hakan2211/datorga (`main`).

**Built (slice 2, 2026-10-27)**
- Encrypted SQLCipher build is the default.
- `core::pseudo`: local pseudonymizer (see step 1 below).
- **Consent** (`core::consent`, `38e286d`): versioned recording-consent text in `templates/consent/recording.de.md`, SHA-256 of the shown text stored per grant, one active consent per kind, `patient.consent_recording` derived, audit without free text. UI: consent card on the patient page.
- **Capture** (`3aad9d8`):
  - `crates/capture`: cpal on its own thread → mono 16 kHz i16 → 5 s chunks to a sink.
  - `core::audio`: AES-256-GCM per chunk (chunk index as AAD), per-recording key (`audio_asset.dek`) only in the encrypted DB, temp+fsync+rename. `read_pcm()` decrypts, `write_wav()` writes a plain WAV for a sidecar.
  - Migration V2: `session_source` (kind `session_audio` | `dictation`, consent link, status `recording` | `recorded` | `interrupted`, `end_reason`) and `audio_asset`.
  - Store: `start_source` (session recording fails closed without consent; one recording at a time), `finish_source`, `recover_interrupted_sources` (run on unlock), `audio_location`; `erase_patient` also removes audio dirs.
  - App: `recording_start/status/stop`, `audio_devices`, `session_get`. Revoking consent cuts a running session recording (`end_reason = consent_revoked`). Window exit finishes the recording.
  - UI: `/sessions/$sessionId` with record/dictate buttons, timer, level meter, remembered mic (`audio_device` setting), recordings list. Sidebar shows "Aufnahme läuft".
- Tests: 65 core, 3 capture, UI typecheck + tests. E2E in the real app: dictation, consent-revoke cut, hard-kill recovery, erasure.

**Built (slice 3, 2026-10-27): local transcription**
- Sidecar: whisper.cpp **b5454** (v1.9.5) CPU x64 build, fetched by `scripts/fetch-whisper.ps1` (pinned SHA-256) into `crates/app/whisper/` (gitignored, only `.gitkeep` is tracked). Bundled as a Tauri **resource folder** (`bundle.resources`: `whisper/*`), not `externalBin`: whisper-cli needs its DLLs (`whisper.dll`, `ggml*.dll`, `ggml-cpu-*.dll` picked at runtime by CPU) next to it, and `externalBin` only ships one exe. Run with `std::process` from the app, `CREATE_NO_WINDOW`.
- `core::stt`: model catalog (`large-v3-turbo-q5_0` default 574 MB, `large-v3-turbo` 1.6 GB, `small-q5_1` 190 MB; HF SHA-256s), `WhisperCli::run` (`-l de -oj -pp`, progress from stderr, cancel kills the process), `transcribe()` = decrypt → WAV in a per-job work dir → sidecar → parse → **work dir always removed**. Lossy UTF-8 decode (whisper can split umlauts across tokens).
- Hallucination guards: **silence gate** (a segment whose loudest 100 ms stays below −50 dBFS is dropped; found in E2E: a silent mic produced "Untertitelung des ZDF, 2020" + a made-up quote) and a filter for German subtitle-credit phrases.
- Migration V3: `transcript` (one per source, `draft` | `confirmed`), `transcript_segment` (`speaker_label` NULL until diarization, `edited`), `job` (queued/running/done/failed/cancelled, progress, attempts; FK to source, cascades on erasure).
- Store: `enqueue_transcription`, `claim_next_job`, `requeue_running_jobs` (on unlock), `finish_transcription` (re-run replaces a draft), `edit_segment`, `confirm_transcript` (**deletes the audio dir and wipes the DEK in the same transaction**), `source_segments(session_id)` → `SourceSegment { source_id, role, start_ms, end_ms, text }` from confirmed transcripts only (dictation = `therapist`, session audio = `unknown`). Audit: `transcription.enqueue/end`, `transcript.create/confirm`, `audio.delete`, no text.
- App: one worker thread (condvar-woken), never holds the store lock while the sidecar runs. Unlock wipes `%APPDATA%/de.datorga.thera/stt-tmp` (crash leftovers) and re-queues `running` jobs. Exit kills the sidecar and leaves the job for re-queue. Erasing a patient cancels their running job. Model download (ureq → `.part` → SHA-256 → rename) and import from a local file (same hash check), progress via polling `stt_overview`.
- UI: session page shows Transkribieren / progress + Abbrechen / error / Erneut transkribieren, transcript viewer (timestamps, per-segment edit on blur, "bearbeitet"), confirm dialog ("Bestätigen und Audio löschen"). Settings: "Transkription (lokal)" card (models, size, download/import/remove, active model).
- Tests: 80 core unit, 4 sidecar tests against a fake `whisper-cli` (`thera-fake-whisper` bin in core: args, progress, cancel, silence, cleanup), ignored `real_whisper` test. E2E in the real app: hash-checked import (+ wrong file rejected), real HF download (small-q5_1, 18 s), Scarlett dictation of TTS German → 3 correct segments, edit, confirm → audio dir gone, audit text-free and chain intact, hard kill mid-job → WAV wiped on relaunch, job resumed (attempt 2) and finished.
- Speed (dev box i7-12700K, CPU, 8 threads, turbo-q5_0): 15.7 s audio in 9.2 s incl. model load.

**Built (slice 4, 2026-10-27): Sitzungsnotiz**
- Template `templates/notes/sitzungsnotiz.yaml` (key `sitzungsnotiz`, Verlaufsdokumentation § 630f BGB). It has five sections:
  - Inhalt und Verlauf (required)
  - Aktueller psychischer Befund
  - Interventionen
  - Risikoeinschätzung
  - Vereinbarungen und Planung

  Hints are filtered by method and age (VT methods/Hausaufgaben vs. TP/AP Übertragung/Abwehr, KJP Bezugspersonen/Kindeswohl).
- Template schema: `report_kinds` defaults to empty. `SectionDef.required` (default true; only required sections block signing) and `SectionDef.dictation` (receives confirmed dictation on creation). `Context.kind` is now `Option` (`None` = note, kind-restricted entries dropped).
- Migration V4: unique index, one `report` of kind `sitzungsnotiz` per `session_id`. `Report.session_id`, `Session.note_id`/`note_status` (LEFT JOIN).
- Store: `create_session_note(session_id)` is idempotent. It pre-fills `dictation` sections from **confirmed dictation** segments only (escaped `<p>` per segment). Session recordings are not copied in, since they contain the patient's words; the therapist picks from them. Audit `report.create` holds kind/session/prefilled, no text. `list_reports` returns Berichte only.
- Export: `build_doc(..., session, ...)` titles a note "Sitzungsnotiz / Sitzung vom …" and omits empty optional sections.
- App: `session_note_create`. `report_get` returns `session` + `segments` (confirmed transcript) for notes.
- UI:
  - Session page: note card (anlegen/öffnen + status, hint if transcripts are unconfirmed).
  - Notes open in the existing editor (`/reports/$id`), which shows a back-link to the session and highlights the active section. The sidebar has an "Abschrift" picker: choose a target section (defaults to the last focused one), then ↵ appends the segment.
  - `RichField` now syncs outside value changes.
  - Note status badge in the sessions list and on the patient page.
- Tests: 83 core (+ note lifecycle, hint filter, note render), Vitest 4. E2E in the real app covered:
  - real TTS dictation via Scarlett → transcribe → confirm → create note → Verlauf pre-filled
  - insert segment into Interventionen, typing still works, persisted after reload
  - sign, then read-only with the picker hidden
  - PDF/DOCX export
  - fresh TP note: TP hints, empty Verlauf blocks signing, idempotent create
  - note not in Berichte queue, audit chain intact

**Built (slice 4 follow-ups, 2026-10-27)**
- **Nachtrag** (§ 630f BGB): migration V5 `report_addendum` (id, report_id cascade, `seq`, body, created_at = signing time, signed_by; an UPDATE trigger makes it append-only, no DELETE trigger so erasure cascades). `Store::add_addendum` only on `sitzungsnotiz` reports that are signed/exported, empty body/signer rejected, report row untouched, audit `report.addendum` with ids only. `Report.addenda` (by seq). Command `report_add_addendum`. Export prints each Nachtrag after the original and its signature line ("Nachtrag vom 27.10.2026, 14:05 – Name", German local time via a built-in CET/CEST rule). UI `components/addenda.tsx` below a signed note: read-only list + form with confirm dialog; signer prefilled from `therapist_name`, else the note's signer. Berichte take no addenda (they go to the Gutachter once).
- Sidebar marks "Sitzungen" while a note is open (`app-shell.tsx` reads the open report's kind; `NavLink` takes an `active` override).
- Transcript picker: text selected inside one segment turns its button into "Markierten Text einfügen" and inserts only that part.
- Tests: 87 core. E2E in the real app: TTS dictation came back as one long segment, partial selection inserted only the selected words, sign, two Nachträge (form clears, headings with time and signer, DB seq 1/2, audit text-free), PDF shows original + signature + both Nachträge.

**Built (slice 5, 2026-10-27): Recovery-kit PDF ("Notfall-Kit", C5)**
- `core::crypto`: `rewrap(key, path)` issues a new `RecoveryKey` and replaces `master.wrapped` atomically (temp + fsync + rename; first run uses the same writer now). `check_recovery(path, key)` parses the key and verifies that it opens `master.wrapped`.
- `core::render`: `RecoveryKit` + `recovery_kit_pdf()` (Typst, `render/notfall.typ`, shares the engine with `pdf()` via `typst_pdf_from`). The one-page A4 kit holds:
  - the key in 16 groups (DejaVu Sans Mono)
  - date/time (German local), reason (Ersteinrichtung / Neuer Schlüssel), computer name, app version, data folder, and a blank line for "Aufbewahrungsort"
  - when it is needed, how to restore, and the warnings (key + data folder = full access; no photos/cloud/e-mail; delete the PDF after printing; old kits stop working after rotation)
  - no QR code (Typst packages would need network)
- Store:
  - `rotate_recovery_key` writes audit `recovery_key.rotate` in the same transaction as the file swap. A failed swap leaves no event. A failed commit is recoverable, because the keychain still has the master key, so rotating again works.
  - setting `recovery_kit_confirmed`: absent = unconfirmed; rotation sets it to false.
  - `confirm_recovery_kit` (audit `recovery_kit.confirm`) and `log_recovery_kit_export` (audit `recovery_kit.export {first_run}`).
  - No key material in any audit event (tested).
- App:
  - `recovery_kit_export(recovery_key, rotated, path)` refuses a key that doesn't open `master.wrapped`.
  - `recovery_key_rotate` loads the master key from the keychain and returns the new key once.
  - `recovery_kit_confirm`.
  - `app_init` returns `recovery_confirmed`.
  - **Debug-only** isolation: `THERA_DATA_DIR` and `THERA_KEYRING_USER` override the data dir and keychain user (`cfg(debug_assertions)`, ignored in release).
- UI: `components/recovery-kit.tsx` (`RecoveryKitPanel`) shows the key groups, "Notfall-Kit als PDF speichern" (save dialog) and the confirm checkbox; continuing persists the confirmation. It's used in three places:
  - the first-run screen
  - the new **"Notfall-Kit fehlt"** gate in `unlock-gate.tsx`: an unconfirmed key (window reload or crash before confirming, or an old install) forces a new key at the next start
  - Settings → Sicherheit → "Neuen Notfall-Schlüssel erstellen": a confirm dialog explains that old kits stop working, then a dialog that won't close until the confirmed "Fertig"
- Tests: 91 core (+ rewrap/old key dead/typed-key parsing, rotation audit without key, failed rotation without event, kit PDF render). E2E in the real debug app (isolated data dir + `e2e-master-key` keychain entry, real entry untouched):
  - first-run screen: continue blocked until the checkbox is ticked
  - reload during first run → "Notfall-Kit fehlt" → new key → kit PDF saved (key in the PDF = key on screen)
  - Settings rotation: Esc and outside click don't close the dialog; kit saved; audit chain intact
  - keychain entry deleted → "Wiederherstellung erforderlich": the old key is rejected; the new key, typed in lower case with spaces, opens the DB
- E2E trick: the native save dialog can't be driven over CDP, and overriding `__TAURI_INTERNALS__.invoke` doesn't take. Use `page.route(/ipc\.localhost\/plugin%3Adialog%7Csave/, …)` and fulfill it with a JSON path (header `Tauri-Response: ok`).

**Built (slice 6, 2026-10-27): encrypted backup + restore (A10, PLAN 3.2)**
- `core::backup`, the `.therabak` format:
  - layout: `THERABK\x01 | header_len | header JSON | frames`
  - header: format, created_at, app_version, `key_id` (non-secret fingerprint of the master key), `wrapped` (master key under the recovery key current at backup time), salt, nonce prefix
  - frames: 1 MiB AES-256-GCM chunks. The payload key is SHA-256 of domain + salt + master key. The nonce binds chunk index + last flag, and the AAD binds the fixed header fields, so a flipped bit, truncation or appended data all fail.
  - plaintext: entries `thera.db` (SQLCipher snapshot via `VACUUM INTO`, still encrypted) + `audio/<source>/<chunk>` (already encrypted)
- **The Notfall-Schlüssel is the backup key (user decision).** On a new machine the printed kit opens the backup. On the same machine the keychain key is enough (`key_id` match).
- `create` (temp + fsync + rename), `inspect`, `unlock` (keychain first, else recovery key), `extract` (paths limited to `thera.db` / `audio/…`), `verify` (decrypt into scratch, open DB, walk audit chain, scratch always removed).
- `rewrap_file` swaps only the header (temp + rename). Key rotation re-wraps every backup of this database in the backup folder (toast with count; failures reported). Copies elsewhere keep the old key, and the kit and confirm dialog say so.
- `install`:
  - extracts into `restore-staging`, opens and verifies the DB there, and rewrites `audio_asset.path` to `<data_dir>/audio/<source>` (`Store::relocate_audio`; paths were absolute)
  - then moves the current DB, `audio` and (when restoring with the recovery key) `master.wrapped` to `vor-wiederherstellung-<German local time>/` (moved, never deleted)
  - moves the restored files in and rolls back on error
- Store: `snapshot_to`, `relocate_audio`, `log_backup` (audit `backup.create/restore/rekey` with counts only).
- `render::file_stamp` gives German local time for file/folder names.
- App:
  - `app_init` returns status **`new`** when there's neither a keychain entry nor `master.wrapped`; `app_setup` then creates the keys and DB ("Neu einrichten").
  - async `backup_create`: setting `backup_dir`; snapshot → archive → verify → audit; setting `last_backup`.
  - `backup_inspect`: date, version, `keychain_matches`.
  - async `backup_restore`: refuses during a recording or transcription; closes the store, installs, stores the master key in the keychain when restored with the recovery key, reopens, audits and sets `last_backup`; reopens the old store on failure.
  - `recovery_key_rotate` is async and returns `{recovery_key, rekeyed, rekey_failed}`.
- UI:
  - `components/backup-restore.tsx` (`RestoreBackup`: pick file, info, key field only if needed, restore; keeps the `init` query and drops the rest)
  - welcome screen "Willkommen bei thera" (Neu einrichten / Aus Datensicherung wiederherstellen)
  - restore option on the "Wiederherstellung erforderlich" screen
  - Settings card "Datensicherung" (folder, Jetzt sichern, last backup, restore with a warning about where the current data goes)
  - The Notfall-Kit PDF now covers backups and still fits on one page (10.5 pt).
- Tests: 97 core (+ backup round trip into another folder with the recovery key incl. audio decrypt after relocation, restore over existing data moves it aside, tamper/truncate/append/wrong key fail, re-wrap, path traversal, file_stamp). E2E in the real app with isolated data dirs and keychain users (`e2e-machine-a/b/c`, all removed afterwards):
  - welcome → setup → kit → patient → backup written and verified (archive contains neither the name nor an SQLite header)
  - second patient → same-machine restore without a key (newer data moved aside) → rotation re-keyed 1 backup
  - new machine: key field required, old key rejected, new key typed in lower case with spaces restored → patient back, chain intact, `last_backup` shown
- Found in E2E and fixed: dropping all queries after a restore also dropped `init`, so the gate stayed on the welcome screen.

**Built (slice 7, 2026-10-27): capture guards**
- `thera_capture::power`: `free_space(path)` (nearest existing ancestor; Windows `GetDiskFreeSpaceExW`) and `KeepAwake` (`SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`; the flag belongs to the calling thread, so the guard is `!Send`; the display may still turn off). Plain FFI to kernel32, no new crate; non-Windows builds compile to no-ops.
- App:
  - `recording_start` refuses when less than 500 MB is free (message gives MB free and needed) before any source row exists.
  - A guard thread per recording holds `KeepAwake`, measures free space every 2 s and stops the recording cleanly below 200 MB (`end_reason = disk_full`; what was recorded stays).
  - `recording_status.minutes_left` is set only when fewer than 60 minutes remain before that stop (16 kHz mono = 32 kB/s ≈ 115 MB/h, so a fixed GB threshold would warn ~17 h early).
  - Debug-only `THERA_DISK_START_MB` / `THERA_DISK_FLOOR_MB` overrides for E2E.
- UI: low-space warning under the recording controls ("noch etwa N Minuten" / "gleich automatisch beendet"); reason text for `disk_full` in the recordings list.
- Fix found on the way (from slice 2): the recording timer only counted samples already written in 5-s chunks, so it showed 00:00 for 5 s and then jumped. `Recorder::duration_ms` now counts captured samples; the stored duration is unchanged.
- Tests: capture 5 (+ free space, keep-awake checked via `CallNtPowerInformation(SystemExecutionState)` without admin rights; another process can hold the same flag, so this shows the call works, not exclusivity). E2E in the real app (isolated data dir, `e2e-cap` keychain user, removed afterwards):
  - start minimum above actual free space → refused with the message, no recording
  - floor above free space → warning shown, auto-stop after ~2 s with `disk_full`, reason on the page
  - normal limits → no warning, live timer (2970 ms after 3 s), clean stop
- Not covered: lid close still sleeps if Windows is set to (no app can prevent that); `powercfg /requests` needs admin, so the request wasn't checked from outside.

**Built (slice 8, 2026-10-27): Nachtrag export hint**
- Migration V6: `report.exported_at`, `report.exported_addenda` (Nachträge contained in the last export; NULL for exports before V6 counts as 0, so the hint errs towards "export again").
- `mark_exported` sets both on every export of a signed or exported report (re-exports too).
- `Report.exported_at` + `Report.export_outdated` (exported and more Nachträge than the last export); `Session.note_export_outdated` (same rule in `SESSION_SELECT`).
- UI: warning on the note's export card ("Seit dem letzten Export (…) kam ein Nachtrag hinzu …") and an `ExportOutdatedBadge` ("neu exportieren") next to the note status in the sessions list, on the patient page and on the session page.
- Tests: 98 core (+ lifecycle: Nachtrag before the first export doesn't count, Nachtrag after export → outdated on report and session list, re-export clears it). E2E in the real app: export → no hint; Nachtrag → hint and all three badges; export again → hint and badges gone; first PDF lacks the Nachtrag, second contains it.

**Built (slice 9, 2026-10-27): printable consent form**
- `render::ConsentForm` + `consent_form_pdf()` (Typst, `render/einwilligung.typ`, one A4 page):
  - practice header, title and paragraphs taken unchanged from `consent::current()` (the text the app records)
  - patient name and birth date: the form stays in the practice file and never goes to the Gutachter
  - therapist name, and signature lines with room to sign
  - two extra lines for Sorgeberechtigte when the patient has a KJP episode
  - a "Widerruf" section to cut off at the bottom
  - footer: text version + first 16 hex of the same SHA-256 that a consent grant stores
  - while the version contains `entwurf`, a box says the text isn't legally reviewed yet
- Store `log_consent_form` (audit `consent.form_export` with kind + version only). App `consent_form_export(patient_id, kind, path)`.
- UI: "Formular drucken (PDF)" on the consent card (file name `Einwilligung-Tonaufnahme.pdf`, no patient name in the file name); the grant dialog suggests printing, signing, then recording the consent.
- Tests: 99 core (+ form: title/paragraphs equal the recorded text, German birth date, draft flag, hash prefix, PDF with and without guardian lines). E2E in the real app: KJP patient → PDF with guardian lines and birth date; adult → none; draft box, practice and therapist printed; audit entries carry only kind/version; grant dialog shows the hint.

**Built (slice 10, 2026-10-27): backup reminder**
- `ui/src/lib/backup.ts` `backupDue(last, days, hasData, now)`: no reminder without patients or when off (0); `null` = never backed up; else the age in days once it reaches the interval. Vitest covers it (3 tests).
- `components/backup-reminder.tsx`: banner at the top of every page (`role="status"`). "Jetzt sichern" runs the backup when a folder is set, otherwise "Sicherung einrichten" links to Settings. "Später" hides it until the next start.
- Settings → Datensicherung: "Erinnern, wenn die letzte Sicherung älter ist als" 1/3/7/14/30 days or "nie erinnern" (setting `backup_reminder_days`, default 7).
- E2E in the real app (fresh data dir):
  - no patients → no banner
  - patient → "noch keine Datensicherung" with "Sicherung einrichten"
  - folder set → "Jetzt sichern" → backup → banner gone
  - backup 8 days old → "8 Tage alt"; "Später" hides it across pages
  - reminder off → gone

**Built (slice 11, 2026-10-27): LLM provider layer (Phase 2)**
- `core::llm` (ureq, blocking):
  - `ProviderId` `openrouter` | `requesty` | `custom`
  - `providers()` with default base URL, key link and `us_intermediary`
  - `LlmConfig` (setting `llm_config`: active provider + per-provider model/base URL)
  - `Endpoint` (Debug hides the key)
  - `list_models` (`GET /models`)
  - `chat(ep, model, &[(Role, &Pseudonymized<String>)], opts)`: the egress gate, since raw text doesn't type-check
  - `ping` (fixed prompt through the pseudonymizer)
  - The URL must be https (http only for localhost/127.0.0.1/[::1]). OpenRouter bodies carry `provider: {zdr: true, data_collection: "deny"}`. API errors become short German messages (401/402/404/429/5xx + provider message, 200-with-error too), never echoing the request.
- Store: `llm_config`/`set_llm_config`, `log_llm_call(action, provider, model, Ok(&Completion)|Err)` → audit `llm.*` with provider/model/ok/tokens/latency only.
- App:
  - API key in the OS keychain (`llm-<provider>`; debug `THERA_KEYRING_USER=x` → `x-llm-<provider>`)
  - commands `llm_overview` (config + providers + `has_key`), `llm_config_set`, `llm_key_set` (empty = delete), async `llm_models`, async `llm_test` (audit `llm.test`)
  - The store lock isn't held during HTTP.
- UI: `components/llm-settings.tsx` (`LlmCard`, Settings → "KI-Entwürfe"):
  - provider select (Aus / OpenRouter / Requesty / Eigener Endpunkt), the pseudonymization note, and a US-intermediary warning per provider
  - key field (password, "hinterlegt" badge, Entfernen, "Schlüssel beim Anbieter erstellen" via `@tauri-apps/plugin-opener`, newly added npm dep)
  - endpoint override, model input with a `<datalist>` filled by "Modellliste laden", "Verbindung testen"
- Tests: 109 core unit (+5 llm) and `tests/llm_http.rs` (4, one-shot local HTTP server): the body holds `[PERSON_1]` and not the name, answer restored, Bearer header, models sorted, 401 and 200-with-error mapped, unreachable endpoint.
- E2E in the real debug app (isolated data dir + keychain user `e2e-llm`, removed afterwards), against a Node mock at 127.0.0.1:8765:
  - `http://example.com` refused before any request
  - bad key → "API-Schlüssel ungültig … (HTTP 401): Invalid API key"
  - good key → "2 Modelle geladen", model saved → "Verbindung steht"
  - config survives reload; audit `llm.test` without text, chain intact
  - key not in the DB file, removing the key → `has_key` false
  - The mock log shows only the fixed test prompt.
- E2E harness note: sonner toasts are read with a MutationObserver that records every added `[data-sonner-toast]` (also inside newly mounted lists); reading the last visible toast picks up stale ones.
- Not done: no drafting yet (no consumer of `chat` besides the test), no streaming, no real-provider test (needs the therapist's key).

**Built (slice 12, 2026-10-27): local models (Phase 2)**
- Sidecar: llama.cpp **b11932** Windows **Vulkan** x64 build, fetched by `scripts/fetch-llama.ps1` (pinned SHA-256) into `crates/app/llama/` (gitignored, `.gitkeep` tracked), bundled as resource folder `llama/*`. Only `llama-server.exe` + the DLLs it loads (~75 MB; `ggml-rpc.dll` left out because ggml loads every `ggml-*.dll` as a backend). Vulkan runs on NVIDIA/AMD/Intel GPUs and falls back to the CPU (`ggml-cpu-*.dll` picked at runtime); `-ngl auto` + `--fit on` (defaults) offload as many layers as fit in VRAM.
- `core::models`: `ModelInfo` + `sha256_file` moved out of `stt` (re-exported there) and shared by both catalogs.
- `core::local`:
  - catalog, biggest first, Q4_K_M, HF URL + SHA-256 + size pinned: **Mistral Small 3.2 24B** (unsloth, 14.3 GB, `cpu_ok: false`), **Qwen3 14B** (official, 9.0 GB), **Qwen3 8B** (official, 5.0 GB, `experimental`). ctx 16k each; KV size per model in the catalog.
  - `Hardware` (RAM, CPU threads, GPUs with dedicated VRAM), `fit()` → `gpu` | `partial` | `cpu` | `too_big` (6 GiB RAM headroom, 1 GiB VRAM margin, partial from 4 GiB VRAM), `recommend()`: the biggest model that fits the GPU entirely, else the biggest `cpu_ok` one that fits RAM; experimental below 30 GiB RAM unless on the GPU (A5) or for the 8B.
  - `LlamaServer`: `spawn` on 127.0.0.1 with a free port and a new random API key per start (env `LLAMA_API_KEY`, inherited `LLAMA_*` vars removed), flags `--no-webui --no-slots --offline --cache-ram 0 --parallel 1 --sleep-idle-seconds 300` (+ `--reasoning-budget 0` for Qwen3's thinking mode); stderr drained into an 8-line in-memory tail (no log file); `wait_ready` polls `/health` (cancel, timeout, early exit → error with the log tail); `Drop` kills.
- `llm`: provider `local` (no key, no base URL, not a US intermediary). `LlmConfig.providers.local.model` = catalog id. Requests go through the same `llm::chat` egress gate.
- `thera_capture::system`: `system_info()` (RAM via `GlobalMemoryStatusEx`, GPUs via DXGI `EnumAdapters1`/`GetDesc1`, raw COM vtable FFI, software adapters skipped) and `ProcessJob` (job object with `KILL_ON_JOB_CLOSE`): sidecars assigned to it die with the app, also on a crash or hard kill. Plain kernel32/dxgi FFI, no new crate.
- App:
  - `local_endpoint` starts the server lazily on the first request (serialized by a start lock, load timeout 10 min, `exiting` cancels), reuses it while alive and the model matches. `llm_endpoint`/`llm_test` handle `local`; `llm_models` for `local` lists installed models without loading one.
  - `llm_config_set` stops the server when the provider or local model changes. Window exit stops it; the job object covers crashes.
  - commands `local_overview` (hardware, recommendation, models with fit/installed/transfer, `loaded`, `loading`, engine), `local_model_download/import/delete` (delete unloads first), `local_unload`.
  - downloads/imports share `start_transfer` with the speech models, which now refuses when free space < size + 1 GB.
  - debug-only `THERA_LLAMA_SERVER` points at another server exe (E2E with the fake).
- UI: `components/model-row.tsx` (shared by speech and language models). Settings → KI-Entwürfe → "Lokales Modell (auf diesem Computer)": machine line, recommendation box, model rows with fit/recommended/experimental badges, "Modell laden und testen", loading/loaded status, "Jetzt entladen".
- Tests: 117 core unit (+ catalog, recommendation table, fit levels, server args), `tests/llama_sidecar.rs` (4) against `thera-fake-llama` (start + readiness + chat + 401 without key + gone after drop; failed load shows the log; cancel and timeout kill; missing model refused), capture 8 (+ struct layouts vs. Windows headers, RAM probe, closing the job kills its child).
- E2E in the real debug app (isolated data dir + keychain user `e2e-local`, removed afterwards):
  - dev box detected as 32 GB · RTX 3090 Ti (24 GB) · UHD 770 · 20 threads → recommends Mistral Small 24B, all three "läuft auf der Grafikkarte"
  - real HF download of Qwen3 8B (5 GB, hash-checked, 458 s at ~11 MB/s)
  - with the fake server: test → "Geladen", unload → process gone, provider "Aus" → process gone, **hard kill of the app → server gone** (job object)
  - with the **real llama-server**: load + answer in 4.7 s (answer itself 75 ms, so thinking is off), model fully on the GPU (~6.9 GB VRAM), listens on 127.0.0.1 only, `/v1/models` and `/slots` → 401 without the key, no web UI (404), unload and normal window close both stop it
  - audit `llm.test` with provider `local`, no text
- Not done: Mistral Small and Qwen3 14B were not downloaded or run (only their hashes are pinned); no CPU-only measurement; whisper-cli is not in the job object yet.

**Built (slice 13, 2026-10-27): fact extraction → Sitzungsnotiz draft (Phase 2)**
- `core::extract`:
  - `system_prompt(template)` from the resolved note template (prose sections with `ai_allowed`, their hints), German rules: only what's in the transcript, cite segment numbers, split mixed segments across sections, negated findings count ("Suizidgedanken verneint" → risiko), no diagnoses/ICD/medication, keep placeholders and relationships, JSON only. `PROMPT_VERSION = extract-2026.1`; `prompt_hash` = SHA-256 of version + prompt.
  - `prepare(template, segments, dict, budget)`: one pseudonymizer for the whole session (a name the rules find in one chunk is tokenized in every chunk), segments as `[S12] (Diktat|Sitzung, mm:ss) text`, chunked at `CHUNK_CHARS` = 24k chars (fits a 16k-context local model). `digest` = SHA-256 of all outgoing messages.
  - `parse(answer, chunk range, sections, map)`: strips `<think>` and code fences, accepts `{"facts": [...]}` or a bare array, citations as `3`/`"S3"`/`"[S3]"`. Dropped and counted: unknown section, empty or > 600 chars, ICD-code pattern, no citation or one outside the chunk, tokens the model invented. Duplicates removed, tokens restored locally.
- Migration V7: `pseudo_term` (per patient, kind + text, cascade on erasure), `extraction` (provider, model, prompt version/hash, chunks, dropped), `note_fact` (`proposed`/`rejected`/`applied`, `edited`), `fact_citation` → `transcript_segment.id`. `SourceSegment` now carries `segment_id`.
- Store: `pseudo_terms`/`add_pseudo_term`/`remove_pseudo_term`, `pseudo_dictionary` (patient record + terms), `note_facts` (with cited segment time + text), `save_extraction` (replaces open/rejected proposals, keeps applied ones), `reject_facts`, `apply_facts` (appends `<ul>` bullets per section; empty section → origin `ai`, otherwise `mixed`; signed notes refused). Audit: `pseudo_term.add/remove` (kind only), `note.extract` (provider/model/prompt hash/counts), `note.facts_apply` (counts), `llm.extract` per request. Never text or terms.
- App: `extract_preview` (outgoing messages, local token table, terms, provider/model), `extract_run(report_id, digest)` (refuses if the text changed since the preview; one `llm::chat` per chunk; nothing stored if any request fails; `extract-progress` events: `loading` for the local model, then chunk n of m), `pseudo_term_add/remove`, `note_facts`, `note_facts_reject`, `note_facts_apply`.
- UI (`components/fact-draft.tsx`, in the note editor above the sections while unsigned and a confirmed transcript exists):
  - "KI-Vorschläge aus der Abschrift": no provider → link to Settings
  - "Fakten vorschlagen…" opens "Vorschau: was gesendet wird": target + model, US-intermediary warning, token table (local only), the outgoing text with placeholders highlighted. Selecting a word there fills "Eigene Begriffe für diese Person"; adding one re-runs the preview. Local provider: says the text stays on the machine.
  - proposals grouped by section: checkbox (default on), editable text, section select, ✕ to reject, timestamp chips (tooltip = segment text; click scrolls the transcript sidebar to the line and highlights it). "N Punkte übernehmen" flushes pending autosaves, then applies; "Alle verwerfen". Applied facts stay listed with their sources ("übernommene Punkte mit Quellen").
- `thera-fake-llama` answers extraction prompts (one fact per line + one invented token).
- Tests: 132 core unit (+ extract: prompt has no tokens and resolves method hints, chunking, cross-chunk tokenization + global numbering + stable digest, parse checks, citation range, clock; store: terms + erasure, save/reject/apply/re-run/sign/audit/erasure). UI typecheck + vitest green.
- E2E in the real debug app (isolated data dir + keychain user `e2e-extract`, removed afterwards), TTS dictation through the Scarlett, transcript corrected before confirming:
  - no provider → hint; mock endpoint (custom provider, 127.0.0.1:8765) logging request bodies
  - preview: patient name and date tokenized, "Schwester Gerda" still plain → selecting "Gerda" fills the term field → added → `Schwester [PERSON_2]`
  - mock request body: no Anna/Beispiel/Gerda/birth date/appointment date; tokens + numbered segments present; one request
  - invented `[PERSON_99]` and an "F32.1" fact dropped (toast says 2 dropped); restored names in the facts
  - citation chip highlights the transcript line; uncheck one, edit one, move one to Befund, apply 3 → bullets in the editor without reload, origins `mixed`/`ai`, statuses + `edited` right; audit has no text, chain intact
  - **real Qwen3 8B** (GPU): first prompt version missed the denied suicidality, filed mood under Verlauf and turned "Ihre Schwester [PERSON_2]" into "Frau Gerda". After adding the split/negation/relationship rules: 8 facts, all in the right sections, 11.4 s including model load.
- Not done: no E2E with a session long enough to need several chunks (unit-tested only); no real cloud provider run; the prompt was tuned on one synthetic dictation (slice 15's eval set should cover it); session recordings still have no speaker separation, so the model can't tell patient from therapist there.

**Built (slice 14, 2026-10-27): Bericht drafting per section (Phase 2)**
- `core::draft`:
  - input: the **applied** facts of all Sitzungsnotizen in the Bericht's episode (`Store::episode_facts`, session order, sessions numbered 1…n, no dates), sent as `Sitzung n:` + `[F7] (Aktueller psychischer Befund) text`.
  - the patient's own name is rewritten to **"Pat."** *before* pseudonymizing (`patient_as_pat`: name parts, "Frau/Herr", genitive s), since the Bericht goes to the Gutachter under the Chiffre. Other names/places/employers still become tokens.
  - one request per section (prose + `ai_allowed`; Diagnose never). System prompt per template + frame (kind, method, age group, PTV 3 intro, page target), no patient data; `PROMPT_VERSION = draft-2026.1`, `prompt_hash` like extraction. The user message holds heading + hints + the therapist's optional instruction (max 500 chars, pseudonymized, not stored) + facts, and repeats the section scope at the end (small models follow what comes last). 24k-char fact budget, else an error.
  - `parse`: `{"sentences": [{"text", "facts": [n]}]}` or a bare array, `F3`/`[F3]`/`3`. Dropped and counted: no or out-of-range citation, > 500 chars, ICD pattern, invented tokens, **any PERSON/ORT/ARBEITGEBER token** (a sentence naming someone else must not reach the Gutachter; dates are restored). Inline markers like `[F1]`/`(F1, F2)` are stripped from the text. `html()` joins the sentences into one `<p>`.
  - `extract` shares `json_list`, `citation_number`, `ICD`, `sha256` with it.
- Migration V8: `section_draft` (provider, model, prompt version/hash, `instructed`, dropped, status `proposed`/`applied`/`rejected`/`superseded`, mode `replace`/`append`), `draft_sentence`, `report_citation` (sentence → `note_fact`; the fact → segment trail is `fact_citation`).
- Store: `episode_facts`, `section_drafts` (open + applied, each sentence with its source facts), `save_drafts` (one transaction per run; replaces the section's open proposal), `reject_draft`, `apply_draft(mode)` (empty section → replace; replace → origin `ai` and earlier applied drafts `superseded`; append → after the therapist's text, `mixed`; sets `report.provider/model/prompt_version/prompt_hash`). Signed Berichte refused. Audit `report.draft` (sections, counts, prompt hash, `instructed`), `report.draft_apply`, `llm.draft` per request; never text.
- App: `draft_sources`, `draft_preview(report_id, sections, instruction)`, async `draft_run(…, digest)` (digest must match the preview; nothing stored if any request fails; `draft-progress` events), `report_drafts`, `report_draft_reject`, `report_draft_apply`.
- UI (`components/report-draft.tsx`; `RedactionDialog` in `fact-draft.tsx` is now shared, with labels/progress event/children as props):
  - Bericht editor: card "KI-Entwurf aus den Sitzungsnotizen" (N facts from M sessions, "Leere Abschnitte entwerfen…", hints when there's no provider or no applied facts), "Entwurf…" next to each draftable heading
  - dialog: sections being drafted, optional instruction (preview refreshes after typing pauses; send waits for it), then the usual preview + terms
  - proposal box above the section editor: sentences with "Sitzung n" chips (tooltip: date, note section, fact text; click opens the Sitzungsnotiz), "Übernehmen" (empty section) or "Ersetzen"/"Anhängen", "Verwerfen". Pending autosaves are flushed first. Applied drafts stay listed under the section as "Quellen des KI-Entwurfs".
- `thera-fake-llama` answers draft prompts (one sentence per fact, max 3, plus one with a bad citation).
- Tests: 142 core unit (+ draft: prompt frame, "Pat." rewrite, numbering/tokens/digest/instruction/budget, parse checks; store: sources across sessions and episodes, save/apply replace+append/supersede/reject/sign/audit/erasure), UI typecheck + vitest (+ `isBlank`).
- E2E in the real debug app (isolated data dir + keychain user `e2e-draft`, removed afterwards; DB seeded through the store API by a scratch tool: two Sitzungsnotizen with 6 applied facts, a Gerda term, an Erstantrag with text in section 1), custom provider → Node mock at 127.0.0.1:8765 logging bodies:
  - card "6 übernommene Fakten aus 2 Sitzung(en)"; "Leere Abschnitte entwerfen…" → 4 sections; outgoing text has "Pat. berichtet…", `Schwester [PERSON_1]`, no Anna/Beispiel/Gerda/birth date; instruction shows in the preview
  - toast "4 Abschnitte entworfen. 8 ungültige Sätze … verworfen" (the mock's `[PERSON_1]` sentence and bad citation per section)
  - Übernehmen → `ai`; Verwerfen; single-section draft on section 1 (already filled) offers Ersetzen/Anhängen; text typed just before Anhängen kept → `mixed`; tooltip shows the original fact; survives reload; audit without text, chain intact; mock got 5 requests, none with names, birth date, Chiffre or the therapist's own section text
  - **real Qwen3 8B** (GPU, 4 sections, 12–16 s): the first prompt invented a prognosis and "reflektierte Therapieziele" and put every fact into every section. After the scope/no-evaluation rules: nothing invented, Behandlungsplan empty rather than padded, but **Somatik still gets unrelated facts** and the model wrote `[F1]` into the text (now stripped).
- Not done: section fit with an 8B model is unreliable (the therapist's review catches it; slice 15 should measure it, and a per-section default source mapping in the template YAML, e.g. `sources: [befund, risiko]`, is the obvious next lever); no real cloud model run; existing section text isn't sent as context (so "Anhängen" may repeat what's there); no session selection (all sessions of the episode count, also for a Fortführungsbericht); style exemplars wait for the Vorlagen module.

**Built (slice 15, 2026-10-27): eval set (Phase 2)**
- `crates/eval` (bin `eval`): `cargo run -p eval -- --provider local --model qwen3-8b` (flags: `--check`, `--only extract|draft`, `--case <id>`, `--sources <yaml>`, `--runs n`, `--gguf`, `--server`, `--base-url`; remote key from `THERA_EVAL_API_KEY`). Uses the app's prompts, parsers and request options (`extract::chat_options()` / `draft::chat_options()`, now shared with the app). Local models: spawns its own `LlamaServer` from `crates/app/llama`, GGUF from `target/eval-models/` (gitignored; Qwen3 8B + 14B there, hashes match the catalog) or the app's models folder. Writes every proposal + score to `target/eval/<suite>/<time>-<model>.json`, including the raw answer on a parse error.
- Suite `crates/eval/suites/de-therapy-v1` (synthetic only; real material belongs in the gitignored `fixtures/`, pass it with `--suite <dir>`): 6 cases (VT panic ×3 dictations, TP depression: one session recording with small talk + one dictation, KJP school anxiety with the mother, forced into several chunks via `chunk_chars`), 80 expected facts, 3 Erstantrag episodes. Each expected fact: accepted note section(s), canonical text (the drafting input), match regexes, cited segments, `required`, `bericht` (Bericht sections that should use it) and `bericht_ok` (acceptable). `forbid` regexes per case/episode catch invented content (diagnoses, prognosis, goals/setting not in the facts, names that must not reach the Gutachter). `sources.yaml` is a candidate per-section source mapping.
- Scoring is deterministic (regex, no LLM judge). Extraction: recall, right section, citation hit, precision (proposals matching any expected fact), extras listed for review, forbidden, dropped. Drafting (input = the cases' expected facts, so it doesn't depend on the extraction run): per section precision (cited facts that belong) and recall, padding, forbidden, dropped. `--sources` also prints a model-free filter analysis.
- `cargo test -p eval`: the suite loads, every pattern hits its own canonical text and cited segments, the gold facts score 100 %, scoring counts misses/misfiles/extras.
- Core fixes found by the eval: draft text kept `.,,` when markers came as `[F1], [F2]` (now a marker run + dangling separators are stripped); `json_list` repairs mismatched/missing closing brackets (`…"}}` for `…"}]}`), never an unterminated string; a draft entry with inline `[Fn]` markers and no `facts` (Qwen3 14B's habit) is split into cited sentences; draft prompt says one entry per sentence, numbers only in `facts` (`draft-2026.2`). 144 core unit tests.
- Results (3 runs each, dev box GPU):

  | | extraction recall / section | drafting precision / recall | forbidden |
  |---|---|---|---|
  | Qwen3 8B, draft-2026.1 | 97 % / 83 % | 51 % / 48 % (4 of 23 requests invalid JSON) | 24 |
  | Qwen3 8B, draft-2026.2 | 97 % / 84 % | 50 % / 88 % (no invalid JSON) | 17 |
  | Qwen3 8B, 2026.2 + `sources.yaml` | – | 52 % / 93 % | 12 |
  | Qwen3 14B, draft-2026.1 | 99 % / 93 % | 69 % / 10 % (most entries dropped: inline citations) | 2 |

- Findings: extraction is good; its weak spot is the session recording (8B filed everything under Verlauf and called the male patient "Patientin": no speakers). Drafting now covers nearly everything but puts too much everywhere (Somatik and Lebensgeschichte ~31 % precision) and the 8B still invents prognoses/goals. **Decision: no per-section source mapping by note section**: it doesn't separate Bericht sections (precision +2 points; model-free it removes only 29 of 72 unrelated facts for Somatik, since somatic facts sit in Verlauf with the symptoms). The next lever is per fact (e.g. the extraction or a cheap classification step tags each fact with Bericht sections, the therapist can correct it, drafting sends only tagged facts).
- Not done: Qwen3 14B on `draft-2026.2` (that re-run was stopped when the machine ran low on memory; the user will redo it); no cloud model (no key on the dev box); Mistral Small not downloaded.

## Current limitations

- ~~Database is unencrypted~~ Encrypted SQLCipher build is live (2026-10-27): Strawberry Perl, LLVM, CMake installed, `LIBCLANG_PATH` set. Old plain DB backed up to `%APPDATA%\de.datorga.thera.plain-backup-20261027` (delete once not needed). A10's technical gates (encryption, recovery kit, verified backup) are done. Before real patients: legal review of the consent text and a restore test on the therapist's own machine.
- Recording/dictation and local transcription work. No diarization (session recordings are one speaker stream, role `unknown`). AI providers (cloud and local) propose Sitzungsnotiz facts (slice 13) and draft Bericht sections from applied facts (slice 14).
- The OpenRouter/Requesty paths are tested only against a mock. Before real use: one test with a real key, and check that the chosen model has a ZDR endpoint (OpenRouter) / an EU region with logging off (Requesty).
- Transcription is **CPU only**. A 50-min session on a laptop CPU may take ~25–50 min (unmeasured on laptop hardware). The CUDA build is 285 MB+ and NVIDIA-only; Vulkan has no prebuilt Windows release (would need our own build). Decide once the therapist's laptop specs are known.
- The sidecar reads a **plaintext WAV** from `stt-tmp` while it runs (deleted afterwards, wiped on next unlock after a crash). Possible hardening: whisper-cli `-f -` (stdin) + parsing stdout, and putting whisper-cli into the app's `ProcessJob` (llama-server already is) so a crashed app can't leave it running.
- Local models: only Qwen3 8B was run, on a 24 GB GPU. CPU-only speed (the therapist's laptop may have no usable GPU) is unmeasured; a dense 14B on a laptop CPU is likely ~3–6 tokens/s. Local and transcription can run at the same time and compete for RAM. The server unloads the model after 5 idle minutes, so the next request pays the load time again.
- There is no audio playback in the transcript viewer, so the therapist reviews text without hearing it.
- The consent text is a **draft**: needs legal review before real patients. After any wording change, bump `RECORDING_VERSION` in `crates/core/src/consent.rs`.
- The printable consent form carries the draft text (marked as such) until the legal review.
- Closing the laptop lid during a recording still sleeps if Windows is configured to. What the audio stream does after resume is untested.
- Backups are manual (reminder banner, no automatic schedule) and always full.
- Vorlagen upload is a placeholder card.
- Sitzungsnotiz: AI proposals exist (slice 13), but facts are appended as bullets: merging them into existing prose is up to the therapist. A note exported before a Nachtrag lacks it until exported again (now flagged "neu exportieren"). Partial insert works within one segment only, not across segments.
- Bericht drafts: an 8B local model puts many facts into the wrong section (eval: ~50 % of cited facts belong, Somatik/Lebensgeschichte ~31 %) and sometimes invents a prognosis or goals; the therapist reviews each draft. Facts only, no style exemplars yet, no session selection.
- Fact extraction runs one request per 24k-character chunk without a cancel button; a long session on a CPU-only laptop may take minutes. Facts from different chunks aren't merged, so the same point can come twice.

## Next steps (in order)

0. ~~**Install tools + encrypted build**~~ **done**.
1. ~~**Pseudonymizer** `core::pseudo`~~ **done**: patient record + confirmed dictionary + rules (dates with day, `Frau/Herr X`, `GmbH/AG/e.V.`, `Firma X`, PLZ+Ort, street+number) become `[PERSON_n]`/`[ORT_n]`/`[ARBEITGEBER_n]`/`[DATUM_n]`, exact round-trip restore, stable tokens across texts, fail-closed residual check. `Pseudonymized<T>` has a private constructor (compile-fail doctest). `detect()` returns char-offset spans for the future redaction review. Not wired into the UI yet: the first consumer is the Phase 2 LLM adapter / Vorlagen redaction.
2. **Consent + dictation**:
   - ~~consent records and `consent_recording` flag gating Record vs. "Zusammenfassung diktieren"~~ **done**: versioned text in `templates/consent/` (draft, needs legal review), hash stored per grant, patient consent card, revoke stops a running recording.
   - ~~cpal recording (crash-safe chunks)~~ **done**: `crates/capture` (cpal → mono 16 kHz, 5 s chunks), `core::audio` (AES-GCM per chunk, per-recording key in the DB, fsync+rename), migration V2 (`session_source`, `audio_asset`), session page with timer/level meter/device picker, crash recovery marks sources `interrupted`, erasure removes audio. Not yet: disk-space guard, keep-awake.
   - ~~local transcription via a whisper.cpp sidecar~~ **done** (resource folder instead of `externalBin`, see slice 3)
   - ~~transcript viewer~~ **done** (edit per segment, confirm)
   - ~~audio auto-delete after the transcript is confirmed~~ **done**
   - ~~disk-space guard + keep-awake~~ **done** (slice 7)
   - open: GPU decision; optional audio playback in the viewer; diarization for session recordings (PLAN §4.3)
3. ~~**Sitzungsnotiz** form per session~~ **done** (slice 4).
4. ~~**Sitzungsnotiz follow-ups**~~ **done**: Nachtrag, sidebar highlight, partial transcript insert.
5. ~~**Recovery-kit PDF**~~ **done** (slice 5). The original brief follows for reference.
   Brief ("Notfall-Kit", PLAN-AMENDMENTS C5; gate for real patient data, A10). What exists:
   - `crates/core/src/crypto.rs`: `RecoveryKey` (`generate`/`display`/`parse`), `wrap`/`unwrap` (AES-GCM), `unlock_or_init` writes `master.wrapped` on first run and returns `Unlock::Created { recovery }`; `recover()` restores the keychain entry.
   - `crates/app/src/lib.rs`: `app_init` returns `recovery_key` **once** (first run only), `app_recover` takes it back. The UI shows it in `components/unlock-gate.tsx`. The key is never stored, so it can't be shown again later.
   - To build:
     - a printable PDF (Typst, like `render/`) with the recovery key (+ QR optional), date, device/app hint and instructions for what to do when the keychain is lost, offered on the first-run screen (save/print before continuing, with a "I have stored it safely" confirmation);
     - since the key can't be re-shown: a Settings action "Neuen Notfallschlüssel erstellen" that generates a new recovery key, re-wraps the master key into `master.wrapped` atomically (write temp + rename) and produces a new PDF; the old printed key stops working (say so); audit entry without the key;
     - tests: re-wrap then `recover()` with new key works and old key fails; E2E of first run on the dev box needs isolation: the data dir comes from Tauri `app_data_dir()` (known-folder API, not the `APPDATA` env var) and the keychain entry `de.datorga.thera`/`master-key` is shared, so an existing entry skips first run. Add a debug-only override for both (e.g. `THERA_DATA_DIR` + a different keyring user), never touch the real entry.
   - The dev box's DB was created before this; the "Notfall-Kit fehlt" gate asks for a new kit on next start.
6. ~~**Encrypted backup + restore**~~ **done** (slice 6). Original brief:
   - an `age`-encrypted (or AES-GCM) archive of `thera.db` (consistent snapshot via SQLite backup API / `VACUUM INTO`), `master.wrapped` and `audio/`, written to a path the user chooses
   - restore from within the app, needing the Notfall-Schlüssel when the keychain doesn't match
   - a verified restore test
   - schedule later
   - **Decided (user, 2026-10-27):** the Notfall-Schlüssel is the backup key. No separate backup password. Restoring on a new machine needs the backup archive and the current Notfall-Kit. Consequences to handle: after a key rotation, older backups need the old key, or re-wrap the key inside them. Say so in the UI and on the kit.
7. ~~**Capture guards**~~ **done** (slice 7): disk-space check before and during a recording (warn early, stop cleanly before the disk is full), and keep the laptop awake while recording (Windows `SetThreadExecutionState`).
8. ~~**Nachtrag export hint**~~ **done** (slice 8): mark a note whose newest Nachtrag is newer than its last export ("neu exportieren").
9. ~~**Printable consent form**~~ **done** (slice 9): PDF of the current recording-consent text (version + hash) with signature lines, from the patient's consent card.
10. ~~**Backup reminder**~~ **done** (slice 10): a hint in the app when the last backup is older than N days (schedule later).
11. **Vorlagen module: deferred** until the therapist hands over material. She will supply old **Sitzungen and Berichte** later, and she must be able to **upload them herself in the app** (no developer-side import):
   - upload old Berichte, old session notes and her forms (PDF/DOCX → text)
   - redaction review (pseudonymizer `detect()` spans), stored as style exemplars
   - the "Vorlagen" placeholder card stays until then
12. **Phase 2** (decisions and slices: PLAN-AMENDMENTS §9):
   - ~~slice 11, LLM provider layer~~ **done**
   - ~~slice 12, local models~~ **done** (hardware probe, GGUF catalog + recommendation, hash-checked download/import, llama-server sidecar in a kill-on-close job object, provider `local`)
   - ~~slice 13, fact extraction per session~~ **done** (redaction preview with per-patient terms, cited facts, review + apply into the note)
   - ~~slice 14, Bericht drafting per section~~ **done** (applied facts → cited sentences per section, "Pat." instead of the name, replace/append, provenance on the report)
   - ~~slice 15, eval set~~ **done** (`crates/eval`, suite `de-therapy-v1`; decided against a per-section source mapping by note section)
   - **NEXT**: re-run `cargo run -p eval -- --provider local --model qwen3-14b --runs 3` on `draft-2026.2` (user does it), one cloud model (`THERA_EVAL_API_KEY=… cargo run -p eval -- --provider openrouter --model <id>`), then improve drafting precision per fact (Bericht section tags per fact, see slice 15 findings) and measure with the eval; add the therapist's real material as a `fixtures/` suite when she brings it
   - deferred: using a ChatGPT/OpenCode subscription (reasons in PLAN-AMENDMENTS §9). An endpoint with an API key already works as "Eigener Endpunkt".
## Open questions for the therapist

- Exact **Chiffre** format (app suggests surname initial + DDMMYY).
- 5–10 anonymized old Berichte and old Sitzungen + her style preferences (she brings them later and uploads them herself) (Phase 0 eval + first style exemplars).
- Laptop specs (CPU, RAM, GPU): Settings → KI-Entwürfe → Lokales Modell now shows them and the recommended model. Decides turbo-q5_0 vs. small, CPU vs. GPU whisper build, and whether local drafting is usable. Which report kinds matter most.

## Handy commands

```sh
cd ui
npm run app                           # dev app, encrypted (hot reload)
npm run app:build                     # release exe used by the desktop icon (+ msi/nsis installers)
npm run typecheck && npm test         # UI checks
cargo test -p thera-core              # also regenerates ui/src/bindings (ts-rs)
cargo test -p thera-capture
cargo test -p eval                    # golden-set consistency + scoring
cargo run -p eval -- --check          # list the suite, no model
cargo run -p eval -- --provider local --model qwen3-8b --runs 3   # GGUF in target/eval-models
powershell -ExecutionPolicy Bypass -File scripts/fetch-whisper.ps1   # once per checkout: whisper.cpp sidecar
powershell -ExecutionPolicy Bypass -File scripts/fetch-llama.ps1     # once per checkout: llama-server sidecar
# real sidecar test (needs a model + 16 kHz mono WAV):
THERA_WHISPER_CLI=crates/app/whisper/whisper-cli.exe THERA_WHISPER_MODEL=<ggml.bin> THERA_WHISPER_WAV=<wav>   cargo test -p thera-core --test stt_sidecar real_whisper -- --ignored --nocapture
```

Models live in `%APPDATA%/de.datorga.thera/models` (dev box has turbo-q5_0 + small-q5_1). German test speech: Windows TTS voice "Microsoft Hedda Desktop" via `System.Speech` (16 kHz mono).

Close the running thera window before `app:build` (it locks the exe).

**E2E** (as used for consent/capture):
- Start Vite (`npm run dev`).
- Launch `target\debug\thera-app.exe` with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`. If the release app is open, also set `WEBVIEW2_USER_DATA_FOLDER` to a scratch dir, or the debug instance exits silently.
- Drive it with `playwright-core` `connectOverCDP` (install it in a scratch dir; it's not in `ui/`).
- Mics on the dev machine: the default "J5-AUDIO" is silent; use the "Scarlett 2i2".
- Local models without the real server: `THERA_LLAMA_SERVER=target/debug/thera-fake-llama.exe` (built by `cargo test -p thera-core`). The fake loads in 0.5 s and echoes; model files only need the catalog size to count as installed.
