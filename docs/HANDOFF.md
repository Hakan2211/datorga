# thera: session handoff (2026-10-27)

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

## Current limitations

- ~~Database is unencrypted~~ Encrypted SQLCipher build is live (2026-10-27): Strawberry Perl, LLVM, CMake installed, `LIBCLANG_PATH` set. Old plain DB backed up to `%APPDATA%\de.datorga.thera.plain-backup-20261027` (delete once not needed). Still: fake patients only until the recovery-kit PDF exists.
- Recording/dictation works (encrypted at rest). No transcription or AI drafting yet.
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
   - **NEXT → local transcription via a whisper.cpp sidecar** (Tauri `externalBin`, separate process), not in-process whisper-rs. Suggested shape:
     - ship `whisper-cli.exe` from a whisper.cpp release as `externalBin` (target-triple suffix in `crates/app/binaries/`); check CPU-only vs. GPU (CUDA/Vulkan) builds against the therapist's laptop
     - model is **not bundled**: download/locate `ggml-large-v3-turbo` (German) into the app data dir, with a settings screen and a SHA-256 check; smaller model as fallback for weak laptops
     - flow: `Store::audio_location(source_id)` → `audio::read_pcm` → `audio::write_wav` to a temp file in the app data dir → run the sidecar (`-l de`, JSON output with segment timestamps) → **always delete the temp WAV** (also on error) → store segments
     - migration V3: `transcript` + `transcript_segment` (see `docs/PLAN.md` §5), plus a `job` row so a long transcription survives an app restart and shows progress
     - keep the PLAN §6 abstraction: everything downstream consumes `SourceSegment { role, start, end, text }`
   - transcript viewer
   - audio auto-delete after the transcript is confirmed
3. **Sitzungsnotiz** form per session.
4. **Vorlagen module**:
   - upload old Berichte/forms (PDF/DOCX → text)
   - redaction review, stored as style exemplars
5. **Recovery-kit PDF** (printable emergency key).
6. **Phase 2:** LLM adapter (OpenAI-compatible, EU default, US-intermediary warning), per-session fact extraction with citations, Bericht drafting from facts + style exemplars, eval set.

## Open questions for the therapist

- Exact **Chiffre** format (app suggests surname initial + DDMMYY).
- 5–10 anonymized old Berichte + her style preferences (Phase 0 eval + first style exemplars).
- Laptop specs (RAM/GPU) for local models; which report kinds matter most.

## Handy commands

```sh
cd ui
npm run app                           # dev app, encrypted (hot reload)
npm run app:build                     # release exe used by the desktop icon (+ msi/nsis installers)
npm run typecheck && npm test         # UI checks
cargo test -p thera-core              # also regenerates ui/src/bindings (ts-rs)
cargo test -p thera-capture
```

Close the running thera window before `app:build` (it locks the exe).

**E2E** (as used for consent/capture):
- Start Vite (`npm run dev`).
- Launch `target\debug\thera-app.exe` with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`. If the release app is open, also set `WEBVIEW2_USER_DATA_FOLDER` to a scratch dir, or the debug instance exits silently.
- Drive it with `playwright-core` `connectOverCDP` (install it in a scratch dir; it's not in `ui/`).
- Mics on the dev machine: the default "J5-AUDIO" is silent; use the "Scarlett 2i2".
