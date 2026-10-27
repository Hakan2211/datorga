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

## Current limitations

- ~~Database is unencrypted~~ Encrypted SQLCipher build is live (2026-10-27): Strawberry Perl, LLVM, CMake installed, `LIBCLANG_PATH` set. Old plain DB backed up to `%APPDATA%\de.datorga.thera.plain-backup-20261027` (delete once not needed). Still: fake patients only until the recovery-kit PDF exists.
- No recording, dictation, transcription or AI drafting yet.
- Vorlagen upload is a placeholder card.

## Next steps (in order)

0. ~~**Install tools + encrypted build**~~ **done**.
1. ~~**Pseudonymizer** `core::pseudo`~~ **done**: patient record + confirmed dictionary + rules (dates with day, `Frau/Herr X`, `GmbH/AG/e.V.`, `Firma X`, PLZ+Ort, street+number) become `[PERSON_n]`/`[ORT_n]`/`[ARBEITGEBER_n]`/`[DATUM_n]`, exact round-trip restore, stable tokens across texts, fail-closed residual check. `Pseudonymized<T>` has a private constructor (compile-fail doctest). `detect()` returns char-offset spans for the future redaction review. Not wired into the UI yet: the first consumer is the Phase 2 LLM adapter / Vorlagen redaction.
2. **Consent + dictation**:
   - consent records and `consent_recording` flag gating Record vs. "Zusammenfassung diktieren"
   - cpal recording (crash-safe chunks)
   - local transcription via a whisper.cpp **sidecar** (Tauri `externalBin`, separate process), not in-process whisper-rs
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
npm run app:plain                     # dev app (hot reload)
npm run app:build:plain               # release exe used by the desktop icon
npm run typecheck && npm test         # UI checks
cargo test -p thera-core --no-default-features --features plain-sqlite
```
