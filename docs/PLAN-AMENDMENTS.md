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
| C6 | Phase 1 uses "cloud STT via proxy", but the proxy only arrives in Phase 4. | Superseded by **A4**: Phase 1 uses local whisper-rs, so no proxy is needed. Opt-in cloud STT later goes through the proxy. |
| C7 | The audit hash chain conflicts with hard erasure if `meta_json` holds PII. | Rule: audit meta holds IDs, counts and hashes only, never names or text. Erasure then leaves the chain intact. |
| C8 | The Gutachter receives the report pseudonymized. | Add a **Chiffre** field and a checklist gate: no patient name, address or birth date in the report body (string scan against patient record). The PDF header carries the Chiffre only. *(Verify the exact Chiffre format with the therapist.)* |
| C9 | Phase 0 exit says €99/mo; the pricing table says €79/€129. | Align on the table. |
| C10 | The Gutachterverfahren reform (move toward QS procedures) could change or retire PTV 3. | Risk row. Templates are data, so a new form means a new template pack, not a release. |
| C11 | No test/CI section. | `cargo test` for core (domain, store, pipeline with a mocked provider), Vitest for the UI, and a GitHub Actions matrix (win/mac) once the repo is pushed. |

### 1b. Merged from the architecture doc (27 Oct, "Therapist Documentation App – Architecture Plan")

| # | Adopted | Effect on master plan |
|---|---|---|
| A1 | **Consent flag per patient gates input mode.** `consent_recording=true` enables Record; otherwise only "Zusammenfassung diktieren" (3–5 min after the session). | The consent table stays (history/hash), plus a derived `patient.consent_recording` flag the UI reads. |
| A2 | **Audio is deleted right after the transcript is confirmed** by default (configurable). | Replaces "30 days after sign-off" as the retention default. |
| A3 | **Local pseudonymizer before every cloud call**: names, places, employers and dates become tokens (`[PERSON_1]`, `[ORT_2]`…), restored in the response. | New `core::pseudo` module and a hard invariant in `LlmProvider`: the cloud impl only accepts a `Pseudonymized<T>` type, so raw text can't compile into a cloud request. Dictionary = patient record + therapist-confirmed entities + rule-based NER (dates, places, capitalized-name heuristics). It's reused for the PTV 3 Chiffre check (C8) and for redacting uploaded old Berichte (§3). |
| A4 | **Local STT first (whisper-rs, turbo).** | Replaces C6. Dictation is single-speaker, so Phase 1 needs no diarization and no cloud STT. Cloud STT (diarization) is only an opt-in for consented session recordings, since audio can't be pseudonymized. Needs LLVM/clang + CMake on Windows to build. |
| A5 | **Local LLM only offered at ≥32 GB RAM** (14–32B); 16 GB is "experimental". | Tightens D6 / §8.2. |
| A6 | **OpenRouter/Requesty are US intermediaries**: they stay as options with a warning in settings. Default is an EU provider with a DPA and zero retention (e.g. Mistral; verify before shipping). | Fixes "Requesty (EU)" in the master plan's §4.3/§8.3. |
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
- 14 Bericht drafting per section from confirmed facts (later plus style exemplars). Each report stores model/provider/prompt version/hash (PLAN §5).
- 15 Eval set (synthetic German sessions → expected facts), `cargo run -p eval`.
