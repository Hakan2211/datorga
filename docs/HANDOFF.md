# thera: session handoff (2026-10-27, after slice 3)

Start a new session with: *"Read docs/HANDOFF.md and continue with the next step."*

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

## Current limitations

- ~~Database is unencrypted~~ Encrypted SQLCipher build is live (2026-10-27): Strawberry Perl, LLVM, CMake installed, `LIBCLANG_PATH` set. Old plain DB backed up to `%APPDATA%\de.datorga.thera.plain-backup-20261027` (delete once not needed). Still: fake patients only until the recovery-kit PDF exists.
- Recording/dictation and local transcription work. No diarization (session recordings are one speaker stream, role `unknown`) and no AI drafting yet.
- Transcription is **CPU only**. A 50-min session on a laptop CPU may take ~25–50 min (unmeasured on laptop hardware). The CUDA build is 285 MB+ and NVIDIA-only; Vulkan has no prebuilt Windows release (would need our own build). Decide once the therapist's laptop specs are known.
- The sidecar reads a **plaintext WAV** from `stt-tmp` while it runs (deleted afterwards, wiped on next unlock after a crash). Possible hardening: whisper-cli `-f -` (stdin) + parsing stdout, or a Windows Job Object so a crashed app can't leave the sidecar running.
- There is no audio playback in the transcript viewer, so the therapist reviews text without hearing it.
- The consent text is a **draft**: needs legal review before real patients. After any wording change, bump `RECORDING_VERSION` in `crates/core/src/consent.rs`.
- No printable consent form yet (the therapist notes the signed paper form in the consent note).
- Capture lacks a disk-space guard and keep-awake (laptop sleep during long sessions).
- Vorlagen upload is a placeholder card.

## Next steps (in order)

0. ~~**Install tools + encrypted build**~~ **done**.
1. ~~**Pseudonymizer** `core::pseudo`~~ **done**: patient record + confirmed dictionary + rules (dates with day, `Frau/Herr X`, `GmbH/AG/e.V.`, `Firma X`, PLZ+Ort, street+number) become `[PERSON_n]`/`[ORT_n]`/`[ARBEITGEBER_n]`/`[DATUM_n]`, exact round-trip restore, stable tokens across texts, fail-closed residual check. `Pseudonymized<T>` has a private constructor (compile-fail doctest). `detect()` returns char-offset spans for the future redaction review. Not wired into the UI yet: the first consumer is the Phase 2 LLM adapter / Vorlagen redaction.
2. **Consent + dictation**:
   - ~~consent records and `consent_recording` flag gating Record vs. "Zusammenfassung diktieren"~~ **done**: versioned text in `templates/consent/` (draft, needs legal review), hash stored per grant, patient consent card, revoke stops a running recording.
   - ~~cpal recording (crash-safe chunks)~~ **done**: `crates/capture` (cpal → mono 16 kHz, 5 s chunks), `core::audio` (AES-GCM per chunk, per-recording key in the DB, fsync+rename), migration V2 (`session_source`, `audio_asset`), session page with timer/level meter/device picker, crash recovery marks sources `interrupted`, erasure removes audio. Not yet: disk-space guard, keep-awake.
   - ~~local transcription via a whisper.cpp sidecar~~ **done** (resource folder instead of `externalBin`, see slice 3)
   - ~~transcript viewer~~ **done** (edit per segment, confirm)
   - ~~audio auto-delete after the transcript is confirmed~~ **done**
   - open: disk-space guard + keep-awake for capture; GPU decision; optional audio playback in the viewer; diarization for session recordings (PLAN §4.3)
3. **NEXT → Sitzungsnotiz** form per session (can prefill from `Store::source_segments`).
4. **Vorlagen module**:
   - upload old Berichte/forms (PDF/DOCX → text)
   - redaction review, stored as style exemplars
5. **Recovery-kit PDF** (printable emergency key).
6. **Phase 2:** LLM adapter (OpenAI-compatible, EU default, US-intermediary warning), per-session fact extraction with citations, Bericht drafting from facts + style exemplars, eval set.

## Open questions for the therapist

- Exact **Chiffre** format (app suggests surname initial + DDMMYY).
- 5–10 anonymized old Berichte + her style preferences (Phase 0 eval + first style exemplars).
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
