# thera: session handoff (2026-10-27, after the Nachtrag export hint)

Start a new session with: *"Read docs/HANDOFF.md and continue with the next step."*

**State at end of session (2026-10-27):** the Nachtrag export hint (slice 8) is done, tested, committed and pushed to `main`. The release app (desktop shortcut) and installers were rebuilt from it. The user asked to work through the remaining steps one by one until Phase 2. Next step: **printable consent form** (see Next steps 9).

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

## Current limitations

- ~~Database is unencrypted~~ Encrypted SQLCipher build is live (2026-10-27): Strawberry Perl, LLVM, CMake installed, `LIBCLANG_PATH` set. Old plain DB backed up to `%APPDATA%\de.datorga.thera.plain-backup-20261027` (delete once not needed). A10's technical gates (encryption, recovery kit, verified backup) are done. Before real patients: legal review of the consent text and a restore test on the therapist's own machine.
- Recording/dictation and local transcription work. No diarization (session recordings are one speaker stream, role `unknown`) and no AI drafting yet.
- Transcription is **CPU only**. A 50-min session on a laptop CPU may take ~25–50 min (unmeasured on laptop hardware). The CUDA build is 285 MB+ and NVIDIA-only; Vulkan has no prebuilt Windows release (would need our own build). Decide once the therapist's laptop specs are known.
- The sidecar reads a **plaintext WAV** from `stt-tmp` while it runs (deleted afterwards, wiped on next unlock after a crash). Possible hardening: whisper-cli `-f -` (stdin) + parsing stdout, or a Windows Job Object so a crashed app can't leave the sidecar running.
- There is no audio playback in the transcript viewer, so the therapist reviews text without hearing it.
- The consent text is a **draft**: needs legal review before real patients. After any wording change, bump `RECORDING_VERSION` in `crates/core/src/consent.rs`.
- No printable consent form yet (the therapist notes the signed paper form in the consent note).
- Closing the laptop lid during a recording still sleeps if Windows is configured to. What the audio stream does after resume is untested.
- Backups are manual (no schedule, no reminder yet) and always full. `last_backup` is the only "how old is my backup" signal.
- Vorlagen upload is a placeholder card.
- Sitzungsnotiz: no AI drafting (Phase 2: fact extraction from `source_segments`). A note exported before a Nachtrag lacks it until exported again (now flagged "neu exportieren"). Partial insert works within one segment only, not across segments.

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
9. **NEXT → Printable consent form:** PDF of the current recording-consent text (version + hash) with signature lines, from the patient's consent card.
10. **Backup reminder:** a hint in the app when the last backup is older than N days (schedule later).
11. **Vorlagen module: deferred** until the therapist hands over material. She will supply old **Sitzungen and Berichte** later, and she must be able to **upload them herself in the app** (no developer-side import):
   - upload old Berichte, old session notes and her forms (PDF/DOCX → text)
   - redaction review (pseudonymizer `detect()` spans), stored as style exemplars
   - the "Vorlagen" placeholder card stays until then
12. **Phase 2:** LLM adapter (OpenAI-compatible, EU default, US-intermediary warning), per-session fact extraction with citations, Bericht drafting from facts + style exemplars, eval set.

## Open questions for the therapist

- Exact **Chiffre** format (app suggests surname initial + DDMMYY).
- 5–10 anonymized old Berichte and old Sitzungen + her style preferences (she brings them later and uploads them herself) (Phase 0 eval + first style exemplars).
- Laptop specs (CPU, RAM, GPU) for local models: decides turbo-q5_0 vs. small and CPU vs. GPU build. Which report kinds matter most.

## Handy commands

```sh
cd ui
npm run app                           # dev app, encrypted (hot reload)
npm run app:build                     # release exe used by the desktop icon (+ msi/nsis installers)
npm run typecheck && npm test         # UI checks
cargo test -p thera-core              # also regenerates ui/src/bindings (ts-rs)
cargo test -p thera-capture
powershell -ExecutionPolicy Bypass -File scripts/fetch-whisper.ps1   # once per checkout: whisper.cpp sidecar
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
