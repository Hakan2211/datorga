# thera — polished plan + build start

## Context

The master plan lives at `~/.opencode/plan/therapy-ai-plan.md` (Tauri 2 local-first app, Rust `core` crate, SQLCipher, 5-stage deterministic pipeline, phases 0–4). It is a strong base, and its decisions D1–D10 stay. The user supplied the real report guideline, **KBV Muster PTV 3 (4.2017)** (https://psydie.de/downloads/LeitfadenNEU.pdf). They also want the therapist to be able to **upload her own style, older Berichte and further templates later** so the app improves over time. `D:\Datorga` is empty and not a git repo. The toolchain has Rust 1.96, Node 25, npm 11 and git; there's no pnpm or typst CLI (we use the typst crate, so the CLI isn't needed).

This plan (1) corrects the master plan where PTV 3 and internal inconsistencies require it, (2) adds the Templates & Style subsystem, and (3) defines the first build slice to execute now in `D:\Datorga`. On approval, the merged master plan is written to `D:\Datorga\docs\PLAN.md` so it lives with the code.

---

## 1. Corrections to the master plan

| # | Issue in current plan | Fix |
|---|---|---|
| C1 | JTBD says "Bericht after each 50-min session". PTV 3 Berichte are **per application** (Erst-/Umwandlungs-/Fortführungsantrag) and draw on several sessions. | Two document types. **Sitzungsnotiz** (per-session Verlaufsdokumentation, §630f BGB) is produced by the pipeline after each session. **Bericht an den Gutachter** is assembled from accumulated, cited facts across the episode's sessions plus structured entries. The pitch becomes "notes in 2 min per session, Bericht in 20 min instead of 3 h". |
| C2 | No concept of a treatment episode. | Add `episode` (Behandlungsfall: method VT/TP/AP, age group adult/KJP, setting, stage probatorik/KZT1/KZT2/LZT) and `application` (kind, sequence number for Fortführungsberichte, which PTV 3 requires numbering consecutively; status; report_id). |
| C3 | Section 5 (ICD-10 diagnoses) and section 3 (medication) are in the report, which conflicts with the MDR boundary. | These are **therapist-entered structured fields** (`diagnosis_list`: ICD-10 code + Sicherheit G/V/Z/A; `medication_list`). The AI never fills them. It may only flag "mentioned in session X, not yet recorded". |
| C4 | Encryption is in Phase 3, but the pilot uses real patient data from Phase 1. | Move SQLCipher, the keychain master key and audio encryption into **Phase 1**. |
| C5 | No recovery if the OS keychain is lost, which means total data loss. | At first run, generate a recovery key (printable "Notfall-Kit" PDF). The master key is wrapped by both the keychain and the recovery key. Restore-from-backup also needs it. |
| C6 | Phase 1 uses "cloud STT via proxy", but the proxy only arrives in Phase 4. | Superseded by **A4**: Phase 1 transcribes locally, so no proxy is needed. *(Built as a whisper.cpp sidecar, not whisper-rs; cloud STT came in slice 17 directly through the therapist's own OpenRouter key, only for her own dictations, see HANDOFF.)* |
| C7 | The audit hash chain conflicts with hard erasure if `meta_json` holds PII. | Rule: audit meta holds IDs, counts and hashes only, never names or text. Erasure then leaves the chain intact. |
| C8 | The Gutachter receives the report pseudonymized. | Add a **Chiffre** field and a checklist gate: no patient name, address or birth date in the report body (string scan against patient record). The PDF header carries the Chiffre only. *(Verify the exact Chiffre format with the therapist.)* |
| C9 | Phase 0 exit says €99/mo; the pricing table says €79/€129. | Align on the table. |
| C10 | The Gutachterverfahren reform (move toward QS procedures) could change or retire PTV 3. | Risk row. Templates are data, so a new form means a new template pack, not a release. |
| C11 | No test/CI section. | `cargo test` for core (domain, store, pipeline with a mocked provider), Vitest for the UI, and a GitHub Actions matrix (win/mac) once the repo is pushed. *(Status 2026-10-29: tests exist, CI isn't built yet; the user moved it to later.)* |

### 1b. Merged from the architecture doc (27 Oct, "Therapist Documentation App – Architecture Plan")

| # | Adopted | Effect on master plan |
|---|---|---|
| A1 | **Consent flag per patient gates input mode.** `consent_recording=true` enables Record; otherwise only "Zusammenfassung diktieren" (3–5 min after the session). | The consent table stays (history/hash), plus a derived `patient.consent_recording` flag the UI reads. |
| A2 | **Audio is deleted right after the transcript is confirmed** by default (configurable). | Replaces "30 days after sign-off" as the retention default. |
| A3 | **Local pseudonymizer before every cloud call**: names, places, employers and dates become tokens (`[PERSON_1]`, `[ORT_2]`…), restored in the response. | New `core::pseudo` module and a hard invariant in `LlmProvider`: the cloud impl only accepts a `Pseudonymized<T>` type, so raw text can't compile into a cloud request. Dictionary = patient record + therapist-confirmed entities + rule-based NER (dates, places, capitalized-name heuristics). It's reused for the PTV 3 Chiffre check (C8) and for redacting uploaded old Berichte (§3). |
| A4 | **Local STT first (whisper-rs, turbo).** | Replaces C6. Dictation is single-speaker, so Phase 1 needs no diarization and no cloud STT. Cloud STT (diarization) is only an opt-in for consented session recordings, since audio can't be pseudonymized. Needs LLVM/clang + CMake on Windows to build. *(Built as a whisper.cpp sidecar instead of in-process whisper-rs (user decision). Diarization came in slice 27 and always runs locally (§14).)* |
| A5 | **Local LLM only offered at ≥32 GB RAM** (14–32B); 16 GB is "experimental". | Tightens D6 / §8.2. |
| A6 | **OpenRouter/Requesty are US intermediaries**: they stay as options with a warning in settings. ~~Default is an EU provider with a DPA and zero retention (e.g. Mistral; verify before shipping).~~ | Fixes "Requesty (EU)" in the master plan's §4.3/§8.3. *(Default superseded by P1 (the therapist's own OpenRouter/Requesty key, ZDR enforced) and M4 (never Mistral).)* |
| A7 | **TipTap (ProseMirror) inside each section** + **DOCX export** besides PDF. | The editor stays sectioned (D8), but each prose section is a TipTap field (German spellcheck, lists, bold). Citation chips become a TipTap node. DOCX is generated in Rust (`docx-rs`) from the same section model, so she can finish in Word if she wants. |
| A8 | Modules in the sidebar: Patienten · Sitzungen · Berichte · **Vorlagen** · later Kalender, Rechnungen, chat over her own history (RAG). Each module gets its own folder/crate + migration namespace. | Vorlagen (= templates + style library §3) is a first-class module. |
| A9 | Other professions wait until one therapist is live. | Unchanged scope freeze. |
| A10 | Test with **fake patient data only** until Phase 3 trust items are done. | Pilot uses synthetic patients first. Real data only after encryption, recovery kit and backup are verified. |

GPUI stays rejected (same reasons as D2).

## 2. PTV 3 as the built-in template pack (`templates/ptv3/`)

The template is **data** (YAML → validated JSON Schema), never code. It's one schema covering both report kinds, with conditional hints:

- **Erst-/Umwandlungsantrag**, sections 1–7: 1 Soziodemographische Daten · 2 Symptomatik und psychischer Befund · 3 Somatischer Befund/Konsiliarbericht · 4 Behandlungsrelevante Angaben zur Lebensgeschichte… / Funktionales Bedingungsmodell (VT) bzw. Psychodynamik (TP, AP) · 5 Diagnose zum Zeitpunkt der Antragstellung · 6 Behandlungsplan und Prognose · 7 Zusätzliche Angaben bei Umwandlungsantrag (`when: kind=umwandlung`).
- **Fortführungsantrag**, sections 1–3: Verlauf seit letztem Bericht · aktuelle Diagnosen + Befund + Tests · Begründung, Planung, Prognose, Abschluss. The report is numbered per episode.
- Sub-point hints carry `when: {method: [VT]|[TP,AP], age: adult|kjp}` (e.g. Makroanalyse for VT, Abwehr/Konflikt/Struktur for TP/AP, Bezugspersonen for KJP).
- Global rules from the form: target length ≈2 pages (checklist warns), keyword style allowed, sub-points only "bei Relevanz", date and personal signature required (sign-off gate), biography presented inside the Bedingungsmodell/Psychodynamik.
- Field types per section: `prose` (AI-draftable with citations), `diagnosis_list` and `medication_list` (human only), `test_results` (human-entered or uploaded).

## 3. New subsystem: Templates & Style (therapist-uploadable)

**Inputs she can upload at any time** (`/library`):
1. **Old Berichte** (PDF/DOCX/TXT) become style exemplars.
2. **Other templates/forms** (e.g. Abschlussbericht, Beihilfe/PKV, Konsiliar, her own Sitzungsnotiz format) become new template packs.
3. **Style notes** (free text: "kurz, stichwortartig, Ich-Form vermeiden…") become a style profile.

**Pipeline (all local, deterministic):**
- `ingest/docs`: text extraction (`pdf-extract` for text PDFs, `docx-rs` for DOCX). Scanned PDFs show "nicht lesbar" in v1; OCR is deferred.
- **Pseudonymize before storing as an exemplar.** The therapist confirms detected names, dates and places in a redaction review screen. Old Berichte are patient data, so they're encrypted like everything else, and only redacted text may ever go to a cloud model.
- **Section mapping:** split by headings onto the target template's section keys (heading-match first, LLM assist second, therapist confirms).
- **Style profile** (computed + editable): per-section median length, keyword vs prose ratio, recurring phrases/terminology, person/tense. Stored as `style_profile` and injected into stage-4 prompts.
- **Exemplar retrieval:** at draft time, pick the top-k exemplar sections for the same template section, method and age group (keyword/BM25 in SQLite FTS5; no embeddings in v1). They're used as few-shot "write like this" with an explicit "facts only from cited segments" instruction.
- **Learning loop:** each signed report's final text (AI draft vs final diff is already stored) is offered as a new exemplar, opt-in. **No fine-tuning in v1**; improvement comes from exemplars + style profile + eval set.
- **Template builder:** upload a form, then the app proposes a section schema (headings, hints, field types, conditions), the therapist edits it in a template editor, and saves it as a versioned local pack. The PDF uses a generic typst layout parametrized by the template, plus her letterhead/praxis stamp (uploadable logo/address).

**Data model additions:** `episode`, `application`, `clinical_fact(id, patient_id, episode_id, kind, text, source_segment_id, session_id, confirmed)`, `source_document(id, patient_id?, kind, path, sha256, extracted_text, redacted_text, status)`, `style_exemplar(id, source_document_id|report_id, template_key, section_key, method, age_group, text)`, `style_profile(template_key, section_key, json)`, `template` gains `origin: builtin|user`, `parent_key`.

## 4. Revised pipeline

```
per session:  INGEST → SPEECH → EXTRACT (facts w/ citations, per PTV3 section keys) → Sitzungsnotiz → therapist confirms
per application: gather confirmed facts across episode + structured fields + style profile + exemplars
                 → per-section draft (bounded, cited) → checklist (sections, length≈2p, pseudonymization, no AI diagnoses) → sign → PDF
```

## 5. Phase changes (summary)

- **Phase 1 (skeleton, shipped to the pilot):** add C4/C5 encryption + recovery kit; episode/application model; PTV 3 pack as a **manual** sectioned form with hints filtered by method/age; Sitzungsnotiz manual form; document upload + text extraction (no style use yet); direct STT (C6).
- **Phase 2 (drafting):** fact extraction per session, Bericht assembly across sessions, style profile + exemplar retrieval, redaction review screen, Fortführung numbering.
- **Phase 2.5 (template builder, ~4 d):** upload form → proposed schema → editor → user pack.
- Phases 3/4 are unchanged, minus the items moved into Phase 1.

## 6. Build slice to execute now (Phase 1, first increment)

*Historical: this was the first slice's brief. Where it was built differently (migrations runner, `ts-rs`, whisper sidecar), §8 and HANDOFF say so.*

Repo root `D:\Datorga` (git init, `.gitignore` with `fixtures/` and `*.db`).

1. **Workspace:** `Cargo.toml` workspace; `crates/core` (modules: `domain`, `store`, `crypto`, `audit`, `templates`, `docs`); `crates/app` (Tauri 2). Rule enforced: `core` has no tauri dependency.
2. **Store:** `rusqlite` with `bundled-sqlcipher` + `refinery` migrations `V1__spine.sql` (patient, consent, episode, application, session, source_document, template, report, report_section, audit_event, app_setting). Repository traits plus SQLite impls.
3. **Crypto:** master key generated at first run, stored via `keyring`, wrapped copy under a recovery key; SQLCipher keyed from it.
4. **Audit:** append-only, hash-chained (SHA-256 over prev_hash + canonical JSON), PII-free meta (C7). Unit test: tampering is detected.
5. **Templates:** `templates/ptv3/{erstantrag,fortfuehrung}.yaml` encoding §2 exactly; loader + `when` filter (method, age, kind) + schema validation. Unit tests per variant (VT adult, TP KJP, Umwandlung includes §7).
6. **UI** (`ui/`, **TanStack Start in SPA mode** + React + TS, TanStack Query, Tailwind v4 + shadcn/ui, npm, i18n with German default). Tauri serves static files, so Start runs with `spa.enabled` (prerendered shell, client-side routing). **No server functions in the desktop app**: all data goes through Tauri commands, wrapped as TanStack Query hooks. The upside is one stack across the app and `server/` (the control plane is also Start), with shared UI components in `packages/ui` later. Screens: app shell + sidebar; Patients list/detail; Episode create (method/age/setting); Session list; **Bericht form** rendering the filtered PTV 3 sections with hint text, diagnosis-list field, and a ~2-page length meter; Audit view.
7. **Tauri commands:** thin wrappers over core repos; typed with `specta`/`tauri-specta` so the UI gets generated TS types.
8. **PDF + DOCX:** typst crate (via `typst-as-lib`) renders a signed Bericht with the Chiffre header; `docx-rs` exports the same section model.
9. **Editor:** each prose section of the Bericht form is a TipTap field (StarterKit, `spellcheck` + `lang="de"`).

This answers the doc's open question: **schema first** (steps 1–9), then the pseudonymizer as the next increment.

Next increments (still Phase 1), in order:
- (a) `core::pseudo` + `Pseudonymized<T>` + tests.
- (b) Consent flag + dictation recording (cpal, crash-safe chunks) + whisper-rs local STT + transcript viewer + audio auto-delete.
- (c) Sitzungsnotiz form.
- (d) Vorlagen module: document upload/extraction.
- (e) Recovery-kit PDF.

The LLM adapter (OpenAI-compatible, EU default, US-intermediary warning) and the first AI draft follow in Phase 2.

**Defaults chosen (change any before approving):** German-first UI; TanStack Start (SPA) + shadcn/ui + Tailwind + TipTap with npm; local whisper STT in Phase 1 (A4); build the skeleton now while Phase 0 materials (anonymized old Berichte, recordings, laptop specs) are collected. The old Berichte she uploads become both Phase 0 eval fixtures and the first style exemplars.

## 7. Verification

- `cargo test -p thera-core`: migrations apply on a fresh encrypted DB; the DB can't be opened without the key; audit tamper detection; PTV 3 filtering per method/age/kind; pseudonymization scan catches the patient name.
- (increment a) pseudonymizer round-trip test: German sample text with names, places, employer and dates gets tokenized, then restored to the identical text; the cloud provider API doesn't accept non-`Pseudonymized` input (compile-time).
- `npm run build` + `npm run test` (Vitest) in `ui/`.
- `cargo tauri dev`: create patient → episode (VT, adult) → open Erstantrag Bericht. Sections 1–6 show VT hints only; switching to Umwandlung adds §7. Fill, sign and export the PDF; open the PDF and check the Chiffre header, no name, and ≈2 pages. The audit view shows a chained event list.
- Inspect the `.db` file with plain `sqlite3`: it must fail to read (encrypted).

---

## 8. Build status (2026-10-27)

*Historical snapshot after slice 1. The current status and next steps are in `docs/HANDOFF.md`.*

**Slice 1 done** (steps 1–9 of §6), verified end-to-end in the running app via CDP-driven WebView2 test:
patient (Chiffre auto-suggested) → VT episode → Erstantrag (6 sections, VT hints only) → TipTap editing with autosave →
name leak blocks signing → fix → sign (read-only) → PDF + DOCX export → audit chain intact.
`cargo test -p thera-core`: 43 tests; `ui`: typecheck clean, Vitest green.

**Deviations from §6 (deliberate):**
- Migrations: tiny `user_version` runner instead of `refinery` (avoids a second `libsqlite3-sys` link; same versioned-SQL model).
- TS types: `ts-rs` (generated on `cargo test`) instead of `tauri-specta` (rc versions pinned to specific tauri releases).
- Tauri pinned to 2.x stable (crates.io latest is a 3.0 alpha).
- `plain-sqlite` cargo feature = dev-only unencrypted fallback; production default stays `sqlcipher`.

**Blocked on the machine:** the encrypted build needs Strawberry Perl (OpenSSL's Configure rejects Git-Bash perl).
Whisper (increment b) will need LLVM/clang.

**Next:** increment (a) pseudonymizer `core::pseudo` + `Pseudonymized<T>`, then (b) consent flag + dictation + whisper-rs.

---

## 9. Phase 2 decisions (user, 2026-10-27)

| # | Decision | Consequence |
|---|---|---|
| P1 | **Providers: OpenRouter and/or Requesty**, with the therapist choosing the model, plus any OpenAI-compatible endpoint ("Eigener Endpunkt": Mistral, a gateway, a local server). | Both are US intermediaries, so Settings shows a warning (A6). OpenRouter requests always send `provider: {zdr: true, data_collection: "deny"}`: a model without a zero-retention endpoint fails instead of falling back. Requesty defaults to its Frankfurt endpoint `router.eu.requesty.ai/v1`. Inference only stays in the EU with an EU-region model, and per-key logging must be switched off on Requesty's side (self-serve default: logged up to 30 days). |
| P2 | **The therapist's own API key**, kept in the OS keychain (`de.datorga.thera` / `llm-<provider>`), never in the DB or backups. | She is the provider's direct customer (her own DPA). No server for us in the pilot. After a restore on a new machine she enters the key again. |
| P3 | **Text may leave the machine only pseudonymized**, fail-closed (`llm::chat` takes only `Pseudonymized<String>`). Every call is audited with provider, model, token counts and latency, never text. | A redaction preview before drafting is part of the drafting slice. |
| P4 | **Local models**: the app detects RAM/GPU, recommends a model and offers a hash-checked download (like whisper). | A llama.cpp `llama-server` sidecar on 127.0.0.1 behind the same OpenAI-compatible client. A5 still applies (≥32 GB RAM recommended, 16 GB experimental). |
| P5 | **Using an existing ChatGPT/OpenCode subscription**: wanted by the user, **deferred** (see below). | Only if an official route appears. Anything that offers an API key + OpenAI-compatible endpoint already works via "Eigener Endpunkt". |

**Why P5 is deferred:** OpenAI offers no official way for a third-party app to bill against a ChatGPT Plus/Pro subscription. The working tricks reuse the Codex CLI's sign-in token. That is unofficial, can break with any update, and puts the user's account at risk. Consumer subscriptions also run under consumer terms without an AVV/DPA, which isn't acceptable for Art. 9 health data, even pseudonymized. An OpenCode plan that comes with an API key and an OpenAI-compatible URL can be entered as "Eigener Endpunkt" today, after checking its data-processing terms.

**Phase 2 slices:**
- 11 LLM provider layer (done 2026-10-27).
- 12 Local models: hardware probe, GGUF catalog + recommendation, download/import with SHA-256, `llama-server` sidecar, provider `local` (done 2026-10-27: llama.cpp b11932 Vulkan build; Mistral Small 3.2 24B / Qwen3 14B / Qwen3 8B experimental; server on 127.0.0.1 with a per-start key, killed with the app via a job object).
- 13 Per-session fact extraction with citations to transcript segments, plus a redaction preview, producing a Sitzungsnotiz draft (done 2026-10-27: facts are proposals the therapist reviews and applies as bullets; the preview shows the exact outgoing text, and terms she adds there are stored per patient and used for every later request; run provenance = provider, model, prompt version + hash).
- 14 Bericht drafting per section from confirmed facts (later plus style exemplars). Each report stores model/provider/prompt version/hash (PLAN §5). (done 2026-10-27: input = facts the therapist applied to the episode's Sitzungsnotizen; the patient is written "Pat." before pseudonymizing and sentences naming anyone else are dropped, since the Bericht goes to the Gutachter; every sentence cites facts (`report_citation` → `note_fact` → `fact_citation` → segment); drafts are proposals applied per section by replace or append.)
- 15 Eval set (synthetic German sessions → expected facts), `cargo run -p eval`. (done 2026-10-27: suite `de-therapy-v1`, 6 synthetic sessions, 80 expected facts with note and Bericht sections, 3 episodes; deterministic regex scoring for extraction and per-section drafting; Qwen3 8B: extraction 97 % recall / 84 % right section, drafting 50 % precision / 88 % recall after `draft-2026.2`. A per-section source mapping by note section was measured and rejected: it barely separates Bericht sections.)

## 10. Cloud models, reasoning, cost (user, 2026-10-28)

| # | Decision | Consequence |
|---|---|---|
| M1 | **No OpenRouter batch.** | Batch is async (minutes to 24 h) and OpenRouter stores requests and results for 30 days, which breaks zero retention. The saving would be a few dollars per quarter. |
| M2 | **Reasoning is set by the app per task**, never shown to the therapist: extraction `low`, drafts `medium` (agent later `medium`), `exclude: true`. | `llm::Effort` in `ChatOptions`, sent to OpenRouter only. `catalog::effort_for` maps a level a model lacks to the next lower one (DeepSeek has no `medium`). `max_tokens` 8192 for extraction and drafts, since reasoning comes out of it. An empty answer with `finish_reason: length` becomes a clear error. |
| M3 | **Guidance: three tiers + a curated list.** Günstig = Claude Haiku 5.5, Ausgewogen (Empfohlen) = Gemini 3.8 Flash, Beste Qualität = GPT-6.1 Sol Pro; further tested models listed with quality stars; free model id only under "Erweitert". | `core::catalog` (prices of 28.10.2026, eval-measured reasoning share, cost per note / Bericht / month at 20 sessions a week). Settings shows what was billed this month and overall (`Store::llm_spend` over the audit log, which now records `cost_usd` and `reasoning_tokens`). |
| M4 | **Never Mistral.** | Removed from the local catalog (Mistral Small 3.2) and every example. |

Eval 2026-10-28 (`de-therapy-v1`, OpenRouter, ZDR enforced, production reasoning settings; total spend 0.57 $):

| Model | Facts found | Right section | Draft precision | Draft recall | Forbidden | Errors | USD / full suite |
|---|---|---|---|---|---|---|---|
| Gemini 3.8 Flash | 100 % | 99 % | 86 % | 100 % | 0 | 0 | 0.101 |
| Claude Haiku 5.5 | 100 % | 99 % | 72 % | 98 % | 0 | 0 | 0.022 |
| DeepSeek V4.1 Flash | 100 % | 99 % | 74 % | 94 % | 0 | 1 (tokens, before effort mapping) | 0.024 |
| GPT-6 Luna Pro | 91 % | 100 % | 72 % | 98 % | 0 | 0 | 0.024 |
| MiniMax M3.1 Flash | 100 % | 96 % | 64 % | 88 % | 4 | 1 (invalid JSON) | 0.013 |
| Qwen3.8 27B | 97 % | 100 % | 70 % | 100 % | 2 | 0 | 0.100 |
| Mini suite only (vt-panik-1, tp-depression-1, episode tp-depression): GPT-6.1 Sol Pro | 97 % | 100 % | 90 % | 100 % | 0 | 0 | ~0.37 (×3) |
| … Gemini 4 Argon | 100 % | 100 % | 73 % | 100 % | 0 | 0 | ~0.14 (×3) |
| … Claude Sonnet 5.5 | 97 % | 100 % | 70 % | 100 % | 1 | 0 | ~0.25 (×3) |
| Local Qwen3 8B (earlier, for comparison) | | | 52 % | 93 % | 12 | | 0 |

After slice 19 (`extract-2026.2` / `draft-2026.3`, tag-filtered drafts, 2026-10-28): Gemini 3.8 Flash draft precision 93 %, recall 96 % (0.098 $); Claude Haiku 5.5 97 % / 96 % (0.023 $); local Qwen3 8B 83 % / 75 % (3 runs). Forbidden 0 for all three. Haiku is now the cheapest and the most precise of the measured models.

Not offered: Qwen3.8 Flash and Omni (no zero-retention endpoint), Claude Opus 5.5 (not measured to save cost). The suite has only 3 Bericht episodes: precision differences of a few points are noise.

## 11. Calendar decisions (slice 22, 2026-10-28; made while building, reviewed by the user 2026-10-29)

*Approved as is, except K1, which was changed (slice 28).*

| # | Decision | Why |
|---|---|---|
| K1 | ~~**A session with a recording or a note keeps its time**: it can't be moved, cancelled or deleted.~~ **Changed 2026-10-29 (slice 28):** a session with a recording or a note **can be moved until its note is signed** (and not while a recording runs); it still can't be cancelled or deleted. The unsigned note's date follows the session. The assistant still moves only untouched sessions (A3). | The Sitzungsnotiz takes its date from the session, and a signed note must not change. But before signing, the likely mistake is documenting into the wrong day's session, and "cancel and re-record" isn't a way out. The audit entry records `documented: true` for such a move. |
| K2 | **Cancel instead of delete.** "Absagen" keeps the session (status `cancelled`, optional reason, shown struck through, excluded from the session numbering). Delete is only for an empty mistake (no recording, note or prep). | Ausfälle stay visible for the record and for billing questions. |
| K3 | **The cancel reason, task titles and the day note stay out of the audit log**; it records only that something changed. | Free text can name people; the log must survive erasure without PII. |
| K4 | **Series without RRULE**: N sessions (1–25 in the dialog, ≤ 52 in core), weekly or every two weeks, dates computed in local time. | The plan's "Serie anlegen" helper; DST keeps the wall-clock time. |
| K5 | **.ics = Chiffre + "Sitzung n" only**, three months from the start of the view, cancelled ones marked `STATUS:CANCELLED`. | A file that leaves the app; names and reasons never go into it. |
| K6 | **"Fällig"** = sessions of the last 60 days that weren't cancelled and have no signed note; Berichte for an application in draft/review; tomorrow's planned sessions with nothing done yet. | Today's sessions are already on the timeline with their next step. |
| K7 | **Times are typed (`HH:MM`)**, not picked in quarter hours. | 50-minute rhythms start at 8:50, 9:40, …; the native time input showed AM/PM. |

## 12. Voice mode + Werkstatt decisions (slice 24, 2026-10-28; made while building, reviewed by the user 2026-10-29)

*Approved as is.*

| # | Decision | Why |
|---|---|---|
| W1 | **Live dictation is the therapist's voice only** and may use the cloud whenever cloud transcription is set up (Settings choose local/cloud like for dictations). No audio is ever written to disk; pieces are wiped after transcription. | Same rule as slice 17 for dictations; session audio never goes this way. |
| W2 | **~3.5 s per sentence locally** (encoder context = piece + 512 frames) rather than ~2.3 s. | The faster setting invented text on short or reverberant pieces; wrong words in clinical notes cost more than a second. |
| W3 | **The Werkstatt text is stored** per note/Bericht (encrypted DB, removed with the patient), not the audio. | She can come back to it; a draft can be made later. |
| W4 | **Werkstatt proposals go through the same review** as fact drafts (replace/append/reject) and never touch diagnoses; in a Bericht a sentence naming another person is dropped, in a Sitzungsnotiz names are restored. | MDR rule and the Gutachter-Chiffre rule from slice 14; the note stays in the practice. |
| W5 | **One dictation at a time**, and none while a recording runs. | One microphone; a recording must not lose audio to a dictation. |

## 13. Assistant decisions (slice 25, 2026-10-28; made while building, reviewed by the user 2026-10-29)

*Approved as is. A1 is the one place where something other than placeholders leaves the machine in a cloud request (yearless appointment times); the user approved that explicitly.*

| # | Decision | Why |
|---|---|---|
| A1 | **Appointment times go to the model as weekday + day.month** ("Fr 30.10., 10:00"; another year in brackets), not as tokens. Birth dates, and dates with a year in note text, stay `[DATUM_n]`. | The assistant has to plan ("übermorgen", "nächste Woche"); a token can't be counted with. A practice time without a year and without a name is low risk, and the pseudonymizer already treats yearless dates as non-identifying. |
| A2 | **One dictionary of all patients** (archived too) and their terms for every thread, also scoped ones. | The question may name anyone; a scoped thread still mustn't leak a name typed by mistake. |
| A3 | **Proposal kinds:** new session, reschedule (only untouched sessions), task, text for a section of a note or Bericht (append, or replace for a Bericht section), text for a session's note (creates the note on apply), day plan (appended). No cancel/delete, no task edits. | Smallest useful set; cancelling and deleting are easy to do by hand and hard to undo. |
| A4 | **Reading leaves out diagnoses**, and text proposals refuse ICD codes and diagnoses sections. | MDR boundary; the assistant has no reason to see them. |
| A5 | **A scoped thread** (opened on a patient or session page) sees only that patient; other sessions in the calendar show as "belegt (andere Person)". | Plan: "tools limited to that patient", but planning needs to know when she is busy. |
| A6 | **Erasing a patient deletes every thread that touched them** (scope, tool reads, proposals, their name among the thread's tokens), not just the scoped ones. | Stored chat text names them; Art. 17. |
| A7 | **Reasoning medium, 4096 tokens, at most 8 requests per question**; tool results capped at 12 000 characters, older ones at 2 000, 40 messages of history. | Cost and latency bound; a question that needs more is better asked narrower. |
| A8 | **Read-aloud** replaces placeholders with a neutral word ("Person", "Ort") instead of names. | Names never leave the machine as audio-to-be either. |

## 14. Speaker separation decisions (slice 27, 2026-10-28; made while building, reviewed by the user 2026-10-29)

*Approved, except S3, which was changed, and S5, which got a consent addition (slice 28).*

| # | Decision | Why |
|---|---|---|
| S1 | **sherpa-onnx v2.0.1** (k2-fsa, Apache-2.0) through its C API, with **pyannote segmentation 3.0** and **3D-Speaker ERes2Net (VoxCeleb)** embeddings, ~50 MB bundled like whisper (`scripts/fetch-sherpa.ps1`, hash-pinned). It always runs locally, also when the speech itself was transcribed in the cloud. | WeSpeaker ResNet34 (the first pick) merged the two voices of the German test audio (cross-speaker similarity 0.75) and of one of sherpa's English samples; ERes2Net separated both (0.19 / 0.22). |
| S2 | **The app starts itself again with `--diarize`** instead of loading the library in-process or shipping another exe. | sherpa's diarization call can't be interrupted; killing the process is the cancel. Same kill-on-close job object as the other sidecars; nothing extra to bundle. |
| S3 | ~~**Always two speakers** (therapist and patient).~~ **Changed 2026-10-29 (slice 28):** two by default; for a session with a parent or partner she picks **2, 3 or 4 people** on the draft transcript and clicks "Neu trennen". A `diarize` job then separates the speakers again from the still-existing audio. The text stays as it is, and hand-set labels are reset (they referred to the old speakers). The count is stored per recording (`session_source.speakers`), so a re-transcription uses it too. With more than one other speaker, nobody is guessed to be the patient: the others stay "Sprecher n", and "Sprecher tauschen" is only offered with two speakers. | Two is still the most robust clustering. But KJP sessions often include a parent, and forcing two speakers merges parent and child into one label. No re-transcription is needed, so a correction costs only the separation time. |
| S4 | **Her voice profile** = mean of her newest 20 voice prints: from every dictation with ≥ 8 s of speech (automatic) and from "… bin ich" in a session. A speaker counts as her at cosine ≥ 0.5 and ≥ 0.15 ahead of the other; otherwise "Sprecher 1/2" and she picks. | No extra enrolment step. Thresholds are set from TTS voices (same voice 0.98, other 0.17); real rooms and microphones will score lower, so they may need tuning with her recordings. |
| S5 | **Prints are biometric data:** hers stay in the encrypted DB until she deletes the profile (Settings → Sprechertrennung). The patient's print of a recording is deleted when the transcript is confirmed (with the audio) or the patient is erased. "Learn my voice" and the separation itself can be switched off. **Added 2026-10-29:** the recording consent (`recording-de-2026-10b-entwurf`) now says that a voice print is computed locally only to tell the speakers apart, never leaves the machine and is deleted with the audio, and that other people in the session (parents, partners) must agree too. Consents granted on the earlier text keep their version and hash. | Data minimisation (Art. 9 DSGVO): the patient's print is only needed for "… bin ich" while the draft is open. The consent text is still a draft for the legal review. |
| S6 | **A failed diarization doesn't fail the transcription**: the transcript just has no speakers. | The text is what matters; speakers are a help. |
| S7 | **Extraction (`extract-2026.3`) writes `T:` / `P:`** before session segments; unknown speakers get no prefix. `SourceSegment.kind` now marks dictations explicitly: a session segment spoken by her is not a "Diktat" (and doesn't prefill the note). | Plan; fixes attributing her questions and interpretations to the patient. |

## 15. Patient profile + card view decisions (slice 29, 2026-10-29; made while building, for the user to review)

| # | Decision | Why |
|---|---|---|
| P1 | **The photo is a small JPEG inside the encrypted database** (`patient_photo`, 256×256, centre-cropped, EXIF orientation applied, metadata dropped). It does not use a separate encrypted file under `media/` as PLAN-PHASE4 said. | A thumbnail is ~10–20 kB. In the SQLCipher DB it is encrypted, in every backup and removed by the erasure cascade with no extra code. A file store with its own keys is only worth it for large files such as audio. The original upload is never stored. |
| P2 | **Gender** `w / m / d` or keine Angabe (NULL); it is shown on the card and in the header and is not sent to any model (the agent tools don't include it). | Matches the mockup chips; "keine Angabe" is the default. |
| P3 | **Working diagnoses live on the Behandlungsfall** (`episode.diagnoses_json`) and only the therapist sets them (`set_episode_diagnoses`; no agent/extraction path). A **new** Bericht copies them into its Diagnose section. Later edits on either side don't sync back. | User decision 2026-10-29. A Bericht is a snapshot for one application. Syncing would silently change a signed one. |
| P4 | **Card facts come from the newest open Behandlungsfall** (else the newest closed one): method, age group, diagnoses. The **KJP filter** uses the episode's age group and otherwise the age (< 21, as in the Psychotherapie-Richtlinie). | A patient without an episode still filters sensibly. |
| P5 | **"n offen"** = past, not cancelled sessions without a signed note + Berichte in draft/review. Same rule as "Fällig" on Heute, but over all time. | One definition of "open" across the app. |
| P6 | **ICD chips show only the chapters present** (F0–F9, "Andere"), and the method chips likewise. Default sort is "Nächster Termin" (then "Zuletzt gesehen", "Name"), and view and sort are remembered per computer. | The mockup's fixed chip list would show empty filters in a small practice. |

## 16. Session summary + timeline decisions (slice 30, 2026-10-29; made while building, for the user to review)

| # | Decision | Why |
|---|---|---|
| Z1 | **In this slice, "Zusammenfassen" goes through the redaction preview like "Fakten vorschlagen"** (preview, then send; `summary_run` checks the digest). Slice 31's autopilot will run it without a preview each time, as the user decided, with the sent text viewable afterwards. | The manual run stays consistent with extraction and drafting. Autopilot is the planned exception. |
| Z2 | **Input:** the confirmed transcript plus the note's AI-allowed prose sections (never Diagnosen). The note goes with the first chunk. A long session gets `6 / chunks` points per chunk, so the total stays ≤ 6. A note-only or dictation-only session works. | Same chunking as extraction (16k local context). One extra merge call for very long sessions isn't worth the cost. |
| Z3 | **Checks on every point:** at least one citation (`S…` transcript or `N…` note section) inside what that chunk showed; no ICD code; no dose or dosing talk (`50 mg`, "Dosis", "aufdosieren"); no unknown token; a claim about prognosis/motivation/goals/setting only if a cited source says so (slice 19's grounding check, now shared via `draft::unsupported`). The patient is written "Pat." (before pseudonymizing and after restoring). Other names are restored, since the summary stays local. | The same safety rules as facts and drafts. The MDR boundary covers medication as well as diagnoses. |
| Z4 | **One summary per session, replaced by each run, including an edited one.** Editing is in place (text per point, remove, add; ≤ 12 points); the citations stay with their point. A signed note doesn't lock the summary. | A summary is a working aid for the timeline, not a document of record. "Neu zusammenfassen" is a deliberate click. |
| Z5 | **The timeline shows held sessions only:** past ones, plus later ones once they have a recording or note. Cancelled sessions show without a number. Sessions are numbered per Behandlungsfall ("Sitzung 2"). The **"/12 KZT 1" contingent and the stage are left out** until slice 31 counts the contingent (`episode.stage` is never updated today, so "Probatorik" would be wrong). | Showing a number now that slice 31 would then change is worse than showing none. |
| Z6 | **Consents go into the Behandlungsfall that was running at that time** (else "Ohne Behandlungsfall"); sessions without a case go there too. | Consents belong to the patient, not to a case. Placing them by time keeps one timeline. |
| Z7 | **The voice prep brief uses the key points instead of the note's Verlauf** for the last 3 sessions ("Kernpunkte (KI-Zusammenfassung)"). It still takes "Vereinbarungen" from the signed note, and it also includes sessions that have key points but no signed note yet. The brief's existing filter still drops sentences naming third parties. The assistant gets a read tool `get_session_summaries` (last 3, max 10). | Shorter context and cheaper. The label makes clear that the points are AI-written. |
| Z8 | **The trigger "again after signing if the note changed a lot" is left to slice 31** (autopilot), together with the trigger after transcript confirmation. | Both are autopilot triggers. Slice 30 has the manual button only. |

## 17. Autopilot + contingent decisions (slice 31, 2026-10-29; made while building, for the user to review)

| # | Decision | Why |
|---|---|---|
| A1 | **The autopilot runs in the existing job worker** as one job kind `autopilot` per session (`job.session_id`) with its steps in `autopilot_step`, one after the other and after any transcription. A restart continues with the unfinished steps. | Local whisper and a local model never load at the same time (RAM). One queue, one place to cancel. |
| A2 | **Transcription starts when a recording stops**, with the engine Settings choose (the cloud rules of slice 17 are unchanged: a session recording goes to the cloud only with consent). Only when something was recorded. The per-click Lokal/Cloud toggle is gone; the ⋯ menu next to "Transkribieren" overrides it for one recording. | Plan §0: the engine is a setting, the override stays reachable. |
| A3 | **"Was wurde gesendet?" is stored per step**: the pseudonymized requests plus the local token table (the same view as the redaction preview), in the encrypted DB, gone with the session or patient. It shows on the autopilot card and on the fact card when the autopilot proposed the facts. | The user decided: no preview each time, but viewable afterwards. |
| A4 | **No AI provider set up → the AI steps are "übersprungen", not failed.** "Erneut versuchen" runs failed and skipped steps again (a provider set up meanwhile counts); finished steps stay. A failed step never stops the next one (the summary runs even if the facts failed). | A missing setup isn't an error. Failures must not block the note. |
| A5 | **The autopilot never overwrites a summary she edited**, and it skips facts once the note is signed. A second confirmed recording of the same session re-extracts over all transcripts and replaces the open proposals (applied facts stay), like "Erneut vorschlagen". | Her edits win over automation. |
| A6 | **Summary again after signing** only if there is none yet, or it wasn't edited and the note's text changed by ≥ 200 characters and ≥ 30 % of what the summary read (`session_summary.note_chars`). | "Changed a lot" (Z8) needs a number; small corrections shouldn't cost a request. |
| A7 | **The phase is set by the therapist** ("Phase ändern" on the Behandlungsfall: Probatorik / KZT 1 / KZT 2 / LZT) together with the start of the Richtlinientherapie (first session after the Probatorik). The app can't know approvals, and `episode.stage` was never updated before. Sessions held from that date count; KZT sessions count into the LZT. | Counting needs a start; guessing from application dates would be wrong (created ≠ approved). |
| A8 | **Contingents (Einzeltherapie):** KZT 12 / 24; LZT VT 60 → 80, TP 60 → 100, AP 160 → 300; KJP VT 60 → 80, TP/AP Kinder 70 → 150, Jugendliche (≥ 14, from the birth date; unknown = Kinder) 90 → 180. Gruppe/Kombination use the same numbers for now. Probatorik shows "n probatorische Sitzungen (höchstens 4/6)" without a due item. | Psychotherapie-Richtlinie §§ 28–30. The smaller contingent warns earlier when unsure. |
| A9 | **"Antrag vorbereiten" 3 sessions before the limit:** in KZT 1/2 the Umwandlungsantrag (KZT 2 needs no Bericht; if she goes that way she sets KZT 2 and the reminder moves to 24), in LZT the Fortführungsantrag once (then the limit is the maximum). The item disappears once that application exists. It shows on Heute and on the Behandlungsfall. | One reminder, gone as soon as she acted. |
| A10 | **One click "Antrag anlegen"** creates the application and its Bericht (diagnoses from the Behandlungsfall, P3) and opens it; with autopilot on it drafts all empty AI-allowed sections in one cancelable run without the preview, as proposals. The run is driven from the Bericht page; leaving the page doesn't stop it (the backend finishes and stores the drafts). | Uses the existing `draft_run`. Nothing is applied without her. |
| A11 | **A finished transcription announces itself** (toast with "Prüfen") and stays on Heute as "Transkript prüfen" until confirmed. Confirming stays a human step (it deletes the audio). | Plan step 2. |
| A12 | **The timeline numbers sessions in the contingent** ("Sitzung 11/12 · KZT 1", with the current phase's limit) and the ones before the therapy start as "Probatorik n". The Heute card keeps its plain session count. | Closes Z5. |
| A13 | **Cleanup:** Protokoll and Vorlagen left the sidebar and the Strg+K list; `/audit` stays, linked from Einstellungen → Sicherheit. The raw status column in Sitzungen is gone. | Plan §0. |

## 18. Work queue decisions (slice 32, 2026-10-29; made while building, for the user to review)

| # | Decision | Why |
|---|---|---|
| W1 | **Each session waits for one thing at a time**, in this order: "Transkript prüfen" (a recording without a confirmed transcript), "Notiz offen" (took place, no note yet), "Unsigniert" (note started, not signed), "Neu exportieren" (Nachtrag after the export). "Notiz offen" + "Unsigniert" are Heute's "Notiz offen". A past session without a note counts as held, so a no-show she didn't cancel stays open until she cancels it. | Disjoint chips with counts that add up; the order is the order she works in. |
| W2 | **"Bereit zur Prüfung" = a draft whose sections all have text** (or `in_review`, which nothing sets yet). No extra "Zur Prüfung geben" click. Dragging cards between columns isn't offered: signing and export have their own checks. | With autopilot drafting, "everything written" is the moment she reviews. |
| W3 | **Deadline on a Bericht card** only while the case is still in the stage the application leads out of (Erstantrag: Probatorik, Umwandlung: KZT 1/2, Fortführung: the first LZT step). It shows "fällig ca. <date>" of the planned session that uses up the contingent, else "noch n Sitzungen", or "Kontingent ausgeschöpft". | The app knows sessions, not the Gutachter's dates; the planned calendar is the best estimate. |
| W4 | **Groups: Heute / Diese Woche (Mon–Sun) / Später / Älter.** "Später" was added for planned sessions after this week; older ones show newest first, 30 at a time. Default filter is "Alle". | The mockup has no place for next week's sessions. |
| W5 | **The summary's first point shows in the row** after the todo label (full text on hover), not only on hover. | The mockup shows it inline; it's the quickest reminder of the session. |
| W6 | **The "bald fällig" banner** on Berichte lists the same cases as Heute's "Antrag vorbereiten" and uses the same one-click action. | One rule, two places. |
| W7 | **Search** matches name and Chiffre. | Same as the patients page. |

## 19. AI costs + key guide decisions (slice 33, 2026-10-29; made while building, for the user to review)

| # | Decision | Why |
|---|---|---|
| K1 | **Requesty: link only.** Its balance is in the management API (`GET /v1/manage/org` → `balance`), which needs an admin key with manage permissions. Like OpenAI, thera doesn't ask for one; the tile links to Settings → Billing. | Same reasoning as OpenAI's admin key: too much power for a balance figure. |
| K2 | **OpenRouter without a management key** shows what the normal key can see: this month's usage of the key, and "übrig bis zum Limit" if she set a key limit. The account balance is only shown with the optional management key (own keychain entry `llm-openrouter-mgmt`, folded away under "Guthaben hier anzeigen…", with a warning that it can manage keys). | `/credits` refuses normal keys (403); a key limit is the safe alternative the guide suggests. |
| K3 | **The low-balance banner** uses the account balance, else the key limit's remainder; with neither it never shows. Threshold setting `balance_warn_usd` (default 2 $, 0 = off), "Später" hides it until the next start like the backup banner. It needs a request to OpenRouter on app start (only when an OpenRouter key is stored; no patient data), cached 5 min in the UI. | A warning that fires before requests start failing. |
| K4 | **Audit `billing.balance`** with provider, ok and whether the account balance was read, never amounts. It doesn't count as an AI request in the spend. | Plan asked; amounts aren't PII but stay out anyway. |
| K5 | **Spend breakdown** from the audit log, tasks: Fakten, Bericht, Zusammenfassung, Diktat-Werkstatt, Assistent, Vorbereitung (sheet + GPT-Live), Spracherkennung (cloud transcription + dictation), Vorlesen, Tests/Sonstiges. Period toggle "Dieser Monat / Gesamt"; the sparkline is always the last 30 days. Days are UTC (as stored). Requests without a reported cost (local, Requesty, custom) count as requests and show "—". | Every audited AI action has one bucket; nothing is estimated. |
| K6 | **"Verbindung testen" in the guide checks the key, not a model**: OpenRouter `GET /key` (models are listed without auth there, so that wouldn't check anything), Requesty/OpenAI `GET /models`. No text is sent; it also tells her when the OpenRouter account has no credit yet (`is_free_tier`). | The guide comes before choosing a model. |
| K7 | **Guide links**: OpenRouter sign-up / settings/credits / settings/keys (+ settings/privacy mentioned for account-wide ZDR); Requesty sign-up / settings/billing / api-keys; OpenAI signup / settings/organization/billing/overview / api-keys. The links live on `ProviderInfo` (`signup_url`, `credits_url`, `key_url`). | One place to update when a provider moves a page. |
| K8 | **Where the guide opens**: "Schritt-für-Schritt-Anleitung" next to the key fields (KI-Entwürfe for OpenRouter/Requesty, Stimme for OpenAI) and "… einrichten" links in the costs card for providers without a key. Hilfe follows in slice 34. Breakdown and model table are stacked, not side by side as in mockup 15 (model ids were cut off in the 2xl card). | |

## 20. Hilfe, Rundgang and Demo-Praxis decisions (slice 34, 2026-10-29; made while building, for the user to review)

| # | Decision | Why |
|---|---|---|
| H1 | **The Demo-Praxis is a second start of thera** (`--demo`) with its own data folder (`<data>/demo`), keychain entry (`demo-master-key`) and WebView profile (`<data>/demo-webview`: own tour state, theme, view choices). "Mit Beispieldaten ausprobieren" and "Demo verlassen" restart the app. | A window only ever opens one database; nothing swaps the store at runtime, so demo and real data can't meet. |
| H2 | **Every entry starts fresh**: the demo folder and key are deleted and re-seeded, dated relative to today. Closing thera while in the demo means the next start is the real practice. | Predictable for the tour and the video; no forgotten demo session. |
| H3 | **The demo has no Notfall-Kit, no backup reminder and its own AI keys** (none set). Without a provider the autopilot only creates notes there. Open question: should the demo reuse her real OpenRouter key, so she can try the autopilot on fictional text? | Keys are per practice like everything else; reusing them would be one exception. |
| H4 | **Demo content**: three fictional people (Lena Sommer VT/KZT 1 with an Umwandlungsbericht draft, Jonas Albrecht TP/Jugendlicher in Probatorik with a transcript to review, Miriam Keller VT/KZT 1 with the Umwandlungsantrag due), German dialogue excerpts of about 12 lines, facts and key points written by me (provider "demo"), silent audio for the unreviewed recording. No photos (no fictional face assets). | Shows each work-queue state once: Transkript prüfen, Notiz offen, Unsigniert, Antrag fällig, Bericht in progress. |
| H5 | **Hilfe articles are Markdown inside the app** (`ui/src/content/hilfe`), rendered by a small built-in parser (no Markdown library, no raw HTML). Links point to other articles (`hilfe:…`), app pages or the web. No screenshots in the articles yet (mockup 18 has one); they can come with slice 35's recordings. | Offline, reviewable as text, no new dependency. |
| H6 | **Search** matches every word in title, keywords, summary and body, ignoring case and accents ("schlusselbund" finds "Schlüsselbund"; "ue" for "ü" is not matched). Strg+K lists the articles by title, summary and keywords. | Enough for nine articles. |
| H7 | **The Rundgang starts once per WebView profile** after the setup and the Notfall-Kit. That includes existing installations: the pilot practice sees it once after this update. It blocks clicks on the page while it runs; Esc or "Rundgang beenden" end it (stored as "skipped"), "Fertig" as "done". A step whose element is missing points at the sidebar entry or shows in the middle. | The plan's trigger; a skipped tour doesn't come back. |
| H8 | **"?" links** sit on the cards that have an article; the video card says "erscheint in Kürze" until slice 35 sets `VIDEO_URL`. The key guides also open from the KI-Anbieter article. | Plan slices 33/34. |
| H9 | **Pages now open at the top**: the shell's scroll area used to keep its position across pages (e.g. Hilfe opened half-way down after Einstellungen). Back/forward restore the position; `#…` links still jump to their target. | Found in the tour E2E. |

## 21. Video explainer decisions (slice 35, 2026-10-29; V1 and V3 by the user, the rest made while building, for the user to review)

| # | Decision | Why |
|---|---|---|
| V1 | **Recorded by a script, voiced by OpenAI TTS** (user's choice): `scripts/video/record.mjs` drives the Demo-Praxis over CDP, captures it with the CDP screencast (1280 × 720 CSS px at 1.5× = 1920 × 1080), and ffmpeg builds the MP4. Voice `coral`, `gpt-4o-mini-tts`, key from `OPENAI_API_KEY` or thera's own OpenAI key in the keychain; only the script text is sent. | Re-recordable after every UI change without anyone at the microphone. |
| V2 | **One source for the words**: the recorder reads the voice-over from `docs/video/skript.md`; each shot lasts as long as its clip. The clicks per shot live in `shots.mjs`. Clips are cached by text, so only changed lines are re-voiced. | Script and video can't drift apart. |
| V3 | **Hosted on bunny.net** (user): the URL goes into `VIDEO_URL` (`ui/src/lib/help.ts`). Until then the Hilfe card says "erscheint in Kürze" and the tour shows no link; with a URL the Hilfe card and the tour's last step link to it. The `.de.vtt` subtitles belong with the upload. | |
| V4 | **The AI steps in the video answer from a local mock** (`mock-llm.mjs`, set as "Eigener Endpunkt" in the demo before filming): facts and key points for Jonas Albrecht, Bericht sentences built from the facts thera sends. The redaction preview and "Was wurde gesendet?" show the real pseudonymized requests. The settings page with the endpoint is not filmed; the costs card shows "thera-demo" with "—" as the cost. | Same result every take, no AI account needed, no cost. |
| V5 | **4:38 instead of ~5 min**, 9 chapters along the Hilfe articles (Tastenkürzel folded into the Hilfe chapter), title and end card, chapter captions, a drawn mouse pointer with click ripples. | 20 short shots read better than long ones. |
| V6 | **The recording shot uses the default microphone** (silent on this machine), so the page shows its "Kein Signal?" hint for a moment. Re-record with a real microphone, or accept it. | No way to feed the native capture a test signal from the script. |
| V7 | **The Demo-Praxis now shares the speech and local AI models** with the real practice (`<data>/models`); before, the demo looked in `<data>/demo/models` and always said "Kein Sprachmodell installiert". | Models hold no patient data; found while filming. |

Fixed while filming: after signing a note or Bericht, every editor turning read-only fired an update and tried to save, which ended in the toast "signed reports are read-only" (TipTap's `setEditable` emits an update by default; now `setEditable(…, false)`).

## 22. UI quick-fix decisions (slice 36, 2026-10-29; U1 by the user, the rest made while building, for the user to review)

| # | Decision | Why |
|---|---|---|
| U1 | **Name on top, Chiffre small underneath** (user) in Heute → Fällig and on the Berichte board cards, via one shared `PatientLabel`. No avatar there: `DueItem` carries no photo flag, and the rows stay compact. | The name is what she recognizes; the Chiffre is still visible for the Gutachter context. |
| U2 | **The photo preview is the stored result**: `patient_photo_preview` runs the same normalization as saving (EXIF rotation, centre square, 256 px JPEG) and returns a `data:` URL without storing anything. A file that can't be read shows its error under the picker right away. | What she sees is exactly what gets saved. |
| U3 | **Back only, no forward button**: an arrow at the left of the header (disabled when there's nowhere to go) plus Alt+←. The mouse's back button is left to the webview's own handling (not tested). | Forward is rarely needed and would crowd the header. |
| U4 | **Board columns keep at least 14 rem and the board scrolls sideways** when the space is too narrow (e.g. Assistent open), instead of squeezing four columns. Column titles truncate, card text wraps. The Demo-Praxis banner wraps its buttons below the text for the same reason. | The usual kanban behaviour; nothing overlaps. |

## 23. Bericht-Lotse start decisions (slice 37, 2026-10-29; made while building, for the user to review)

| # | Decision | Why |
|---|---|---|
| L1 | **The Antragsart is suggested by the case, not asked**: the application that is due, else the one leading out of the current phase (Probatorik → Erstantrag, KZT → Umwandlung, LZT → Fortführung). "Neuer Bericht" and the patient page create it right away; the start step shows the three kinds with "Empfohlen" and lets her switch **while the Bericht is still empty** (no text, no AI draft). Switching rebuilds the sections and keeps the diagnoses. | One click instead of choosing between three always-visible buttons; a wrong choice costs nothing while nothing is written. |
| L2 | **One open draft per kind**: if the case already has an unsigned Bericht of the suggested kind, "Neuer Bericht" and the patient page open it instead ("Entwurf öffnen"). | No duplicate Erstanträge by accident. |
| L3 | **Steps for now: Start · Abschnitte · Prüfen · Fertig.** "Fragen" joins in slice 38. Until slices 39–40, Abschnitte/Prüfen/Fertig are the existing editor page (Prüfen and Fertig jump to the checklist and export). An untouched Bericht opens on Start, anything else on Abschnitte. The step is in the URL (`?step=`), so back/forward work. | Ships a working path now without a half-built screen. |
| L4 | **Bereitschaft** counts, per section, the applied facts a draft would use by default (since the previous application, tag-filtered for Erst-/Umwandlung): 3+ full, 1–2 half, 0 empty. Text already there, an open proposal, the diagnoses and "nicht relevant" have their own labels. The open questions join in slice 38. | Shows at a glance where the AI has material and where she will have to write herself. |
| L5 | **"Nicht relevant" only for Somatik** (template `optional: true`), the one PTV 3 section that is often empty. The section is left out of the export (numbers of the others stay), counts as done on the board, is skipped by the checklist and by drafting; its text is kept in case she switches back. | PTV 3: "Unterpunkte nur bei Relevanz"; the other sections are expected in every Bericht. Easy to extend in the YAML. |
| L6 | **Gaps are fixed on the start step**: diagnoses on the Behandlungsfall that differ from the Bericht ("Übernehmen", keeps the notes), a missing Chiffre (patient dialog), a phase still on Probatorik for an Umwandlung/Fortführung (link to the patient page), no AI model (link to settings). | She doesn't discover them at signing time. |

## 24. Bericht-Lotse "Fragen" decisions (slice 38, 2026-10-29; made while building, for the user to review)

| # | Decision | Why |
|---|---|---|
| F1 | **Question catalogue** (`asks:` in `templates/ptv3/*.yaml`, drafted by me from the PTV 3 hints, **for the therapist to review**): §1 Alter/Geschlecht, Beruf, Familienstand (KJP: Schule, Geschwister/Eltern); §2 psychischer Befund, Testverfahren, Krankheitsverständnis; §3 somatische Befunde/Konsil/Sucht, Psychopharmaka, Vorbehandlungen; §4 Biografie, Beginn/Verlauf der Beschwerden, Bedingungsmodell (VT) bzw. Psychodynamik (TP/AP); §6 Ziele, Methoden, Setting/Frequenz, Bezugspersonen (KJP), Kooperation, Prognose; §7 Verlauf der KZT, Zielerreichung, Begründung; Fortführung: Verlauf, Zielerreichung, Begründung, Planung, Prognose, Abschluss. Each with quick chips ("keine", "nicht bekannt" …). **No question on a diagnoses section** (the template validator rejects it). | The gaps the AI used to leave silently (Medikation, Vorbehandlungen, Ziele, Setting, Prognose) are asked up front. Wording and chips are data; she can change them without code. |
| F2 | **Answers are her own statements**: they go into the draft as "Angaben der Therapeutin", numbered and cited like facts, only for their own section, and the sentence shows **"Ihre Angabe"** (question + answer on hover). The grounding check (prognosis/goals/setting/motivation) counts **her question and answer** as the source, instead of exempting answers completely: "günstig" to the prognosis question grounds a prognosis sentence, but an answer about medication can't ground an invented goal. ICD, name and length checks still apply. Medication is reproduced as she wrote it (prompt rule). Prompt `draft-2026.4`. | Plan said "exempt from GROUNDED"; this keeps the intent (her answers aren't dropped) without letting a model pin anything on any answer. |
| F3 | **Chips fill, extend or toggle** an answer (parts joined with "; "). The age/gender question is answered by a **record chip** computed from birth date + Geschlecht ("35 Jahre, weiblich"); she confirms it with one click. It is never sent to the AI check. | One click for the common answers; nothing from the record lands in the Bericht without her choosing it. |
| F4 | **AI coverage check only on request** ("Mit KI prüfen", with the usual redaction preview), not automatically. One request per Bericht with the open, unanswered questions and the facts since the previous application; a question counts as answered only if a cited fact is one the draft of **its section** would see (tags). Covered questions fold into "Aus den Sitzungen beantwortet (n)" with their facts and can be answered anyway. The result is stored per Bericht with a digest of the facts: when facts change, the step says so ("erneut prüfen"). Active model, medium effort, own spend task **"Fragen prüfen"** (`llm.gaps`). Without a model all questions are shown. | Matches the manual-run rule (preview before sending, slice 30 Z-decisions); the autopilot doesn't run it. |
| F5 | Questions of a section marked **"nicht relevant"** are hidden and their answers not used. Switching the Antragsart (only while the Bericht is empty) **keeps the answers** whose section + question still exist (Erst ↔ Umwandlung share them) and drops the check. | No lost typing; the check was over the other template's questions. |
| F6 | **Flow**: Start's one button becomes "Weiter: n Fragen beantworten" while questions are open; Fragen's is "Weiter: Entwurf schreiben lassen" (opens today's draft dialog for the empty sections until slice 39 replaces it), with a quiet **"Später"** next to it. Answers alone are enough to draft a section without facts (e.g. Somatik). The Bereitschaft list shows "3 Fakten · 2 offene Fragen"; a section counts as full only with 3+ facts/answers and no open question. | One primary button per step; the questions never block. |
| F7 | **"Lieber frei diktieren"** opens the existing Diktat-Werkstatt (it already splits free dictation into sections). | Plan: the Werkstatt becomes an option inside step 2, not a separate concept. |
| F8 | **Fix: built-in templates now update in existing databases** when the compiled-in YAML changes (same version, `origin = builtin` only; user packs untouched). Before, `INSERT OR IGNORE` kept the first stored definition, so slice 37's Somatik "nicht relevant" toggle never reached databases created before it. | Found while adding `asks:`; without it the questions wouldn't appear in her existing data either. |
| F9 | **Questions of a section that already has text don't count as open**: they fold into "Abschnitte, die schon Text haben (n Fragen, freiwillig)" and don't show in the Start button or Bereitschaft. The Fragen step always offers "Entwurf schreiben lassen" for the empty sections, also in a half-written Bericht. "Fragen" counts as done when nothing is open or drafting has started. | Found in the E2E on the demo's half-written Umwandlungsbericht: 20 "open" questions, mostly for finished sections, was exactly the chaos the plan wants to avoid. |
