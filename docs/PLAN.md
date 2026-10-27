# Therapy Documentation Assistant — Project Plan

**Codename:** `thera`
**Status:** Planning / pre-build
**Owner:** you
**Pilot client:** psychotherapist (paying build fee, potential subscription customer)
**Last updated:** 2026-10-25

---

## 1. Decisions locked

| # | Decision | Choice | Why |
|---|---|---|---|
| D1 | v1 platform | **Tauri 2 desktop app, local-first** | Data stays in the practice → no processor role, no hosting, strongest legal position, premium pricing story |
| D2 | UI stack | **Web frontend (TanStack Router) inside Tauri webview** | Rich text/PDF/waveform/charts/i18n all solved; GPUI Kit loses the whole ecosystem and is pre-1.0, tied to Zed |
| D3 | Mobile app | **Not in v1.** Universal audio import instead | Phone is the worst custody location for a 60-min confidential recording; client always has their laptop |
| D4 | Database | **SQLite (SQLCipher) locally = single source of truth** | Cloud holds identity/entitlements only, never patient data |
| D5 | Cloud | **Thin control plane**: auth, license, LLM/STT proxy, template packs, updates, billing | Required so the desktop binary never contains your provider keys |
| D6 | Model access | **Cloud default + local opt-in via hardware probe** | 8B Q4 fits any 16GB laptop; local is a real tier, not a checkbox |
| D7 | "Agentic" | **No.** Fixed deterministic 5-stage pipeline | Clinical documentation needs auditability, reproducibility, human signature |
| D8 | Report editor | **Sectioned form, not a word processor** | Better clinically, forces payer-expected structure, makes the LLM task a fill-in-the-slots job |
| D9 | Commercial | **Product**, not bespoke: landing page, license, subscription, template packs | Client is the pilot, not the market |
| D10 | Scope | **v1 = Sessions & Berichte + Patients spine** | Everything else is a module behind a registry |

---

## 2. Product definition

### 2.1 The job to be done

> "I just finished a 50-minute session. I need a payer-format Bericht in 5 minutes instead of 40, and I need to be able to defend every sentence in it."

### 2.2 What the product actually sells

Not "AI writes your report." The value is:

1. **Time** — 30–45 min saved per session (~13 h/month at 20 sessions).
2. **Consistency** — every Bericht in the same payer-expected structure, every time.
3. **Recall** — the report cites what was actually said, not what the therapist remembers at 6pm.
4. **Defensibility** — traceable from report sentence → transcript segment → audio timestamp.
5. **Privacy** — the audio and transcript never leave the practice.

### 2.3 v1 scope (frozen)

**In:**
- Patient file (spine): identity, contact, status, session history, documents
- Session capture: 4 modes (see §6)
- Consent capture + record
- Transcription with speaker separation
- Structured extraction → template-driven draft
- Section-by-section review & edit, checklist self-review
- Sign-off → PDF → archive
- Encrypted backup / full export
- Retention policy + audio auto-delete
- Audit log
- Cloud model proxy with usage metering
- Local model option with hardware probe

**Out of v1 (module registry, later):**
- Calendar / scheduling
- Billing codes (EBM/GOP, GebüH), invoicing
- Supervision / case conference
- Progress charts, goal tracking (v1.5 — schema prepared)
- Prescribing, medication dosing, interaction checks (see §3.4)
- Multi-therapist practice / team accounts
- Mobile app
- Multi-device sync

---

## 3. Legal & regulatory guardrails

Not legal advice — these are the engineering constraints that follow from the domain. Get a lawyer to review the client contract and the patient-facing consent text.

### 3.1 Data classification

Session audio and transcripts are **Art. 9 GDPR special-category health data**. In DE the therapist is bound by **§203 StGB** (AT: §121 StGB); in CH, Art. 321 StGB + cantonal professional secrecy. Disclosing to a third party is a criminal matter, not just a compliance one.

### 3.2 Consequence for architecture

| Where data lives | Consequence |
|---|---|
| Local machine only | No processor, no AVV, no breach exposure |
| Your EU-hosted server | You become a processor: AVV, subprocessor list, TOMs, backups, deletion, breach runbook |
| US aggregator / consumer chat product | Weakest position; most likely to fail an audit or a Berufsverband question |

→ This is why D1 + D4 exist.

### 3.3 Provider rules

- Cloud LLM/STT is **opt-in per session**, and the UI states plainly what leaves the machine.
- Every cloud provider must offer: EU data residency (or an adequacy decision), a DPA, **no training on submitted data**, and short retention (ideally zero-retention endpoints).
- The proxy (§5.2) must be **stateless with respect to content**: stream through, log token counts and timings only. This is an invariant, not a preference — it's what makes "we never store your sessions" truthful.

### 3.4 MDR boundary — do not cross it

- ✅ "Documentation draft for therapist review and signature."
- ✅ Suggesting structure, summarizing what was said, flagging what's missing from a template.
- ⚠️ Suggesting ICD-10/ICF codes — allowed only as an explicitly human-confirmed *suggestion*, never auto-filled, never framed as diagnosis.
- ❌ Autonomous diagnosis, treatment recommendation, dosing, interaction checking, risk scoring that drives action.

Marketing copy must stay on the right side of that line. Never say "diagnoses," "recommends treatment," or "detects risk."

### 3.5 Consent as a feature

- In-app consent capture: patient reference, scope, template version, timestamp, granted/revoked, hash of the displayed text.
- Mid-session "stop recording" must work instantly and mark the cut point.
- Patient right of access: one-click export of everything held about a patient.
- Patient right to erasure: hard delete + audit entry.
- Modes 2 and 3 (§6) record **no patient voice at all** — the legally cleanest path, and a marketing asset.

---

## 4. Architecture

### 4.1 Two planes

```
┌───────────────────────── PRACTICE MACHINE (data plane) ─────────────────────────┐
│                                                                                 │
│  Tauri shell                                                                    │
│  ├── webview UI (TanStack Router + TS)                                          │
│  └── Rust core                                                                  │
│       ├── capture      (cpal → crash-safe chunked WAV)                          │
│       ├── stt          (whisper-rs  |  cloud STT via proxy)                     │
│       ├── diarize      (ort + speaker embeddings + clustering)                   │
│       ├── pipeline     (segment → extract JSON → render → check)                │
│       ├── render       (typst → PDF)                                            │
│       ├── store        (SQLCipher, migrations, repo traits)                     │
│       ├── crypto       (OS keychain master key, AES-256-GCM per file)           │
│       └── audit        (append-only, hash-chained)                              │
│                                                                                 │
│  Files: /audio (encrypted)  /exports  /backups                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
                    │  TLS, no patient content persisted
                    ▼
┌──────────────────────── CONTROL PLANE (your infra) ─────────────────────────────┐
│  auth · license/entitlement · model proxy (keys live here) · usage metering     │
│  template packs · update feed · opt-in crash reports (no content, ever)         │
│  billing (Stripe)                                                               │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### 4.2 Repo layout

```
thera/
├─ Cargo.toml                # workspace
├─ crates/
│  ├─ core/                  # NO tauri, NO ui — the portable product
│  │   ├─ domain/            # entities, value objects, invariants
│  │   ├─ store/             # repo traits + sqlite impl + migrations
│  │   ├─ capture/           # audio recording, chunking, device handling
│  │   ├─ stt/               # SttProvider trait; whisper-rs, cloud impls
│  │   ├─ diarize/           # embeddings, clustering, label mapping
│  │   ├─ pipeline/          # stages, prompts, schema validation
│  │   ├─ render/            # typst templates → pdf
│  │   ├─ crypto/            # keychain, envelope encryption, backup
│  │   └─ audit/             # hash-chained event log
│  ├─ app/                   # Tauri shell: commands, sidecars, updater, tray
│  └─ eval/                  # golden-set harness (CLI)
├─ ui/                       # TanStack Router + TS + component lib
├─ templates/                # versioned Bericht templates (typst + json schema)
├─ fixtures/                 # anonymized sessions for the eval set (gitignored)
└─ server/                   # TanStack Start control plane (separate deploy)
```

**Rule:** `crates/core` must never depend on `tauri`. That single rule is what makes a GPUI shell, a web app, or a headless CLI possible later without a rewrite.

### 4.3 Key library choices

| Concern | Choice | Note |
|---|---|---|
| App shell | `tauri` v2 | built-in signed updater, sidecars, no Node in renderer |
| DB | `rusqlite` + `bundled-sqlcipher` | SQLCipher support is the reason; keep repo traits so Postgres can be added for a server build |
| Migrations | `refinery` | works with rusqlite, versioned SQL files |
| Recording | `cpal` → chunked WAV (5s chunks, fsync) | **Do not** rely on webview `MediaRecorder` for a 60-min session; a lost session is unacceptable |
| STT local | `whisper-rs` (large-v3-turbo q5) | ~0.9 GB, 0.3–0.5× realtime on CPU |
| Diarization | `ort` + WeSpeaker/3D-Speaker ONNX + agglomerative clustering | two-speaker sessions are tractable; general diarization is not needed |
| LLM | one OpenAI-compatible client + provider config table | Requesty (EU) / OpenRouter / Azure OpenAI EU / local llama.cpp |
| PDF | `typst` crate, templates compiled in | real typography, German hyphenation, per-payer templates |
| Crypto | `keyring` (OS keychain) + `aes-gcm` + `age` for backups | |
| UI | TanStack Router, TanStack Query, a component lib, `wavesurfer.js` | |
| Server | TanStack Start + Postgres (or just SQLite + a small host) | holds no patient data |

---

## 5. Data model (SQLite sketch)

Full DDL lives in `crates/core/store/migrations/`. Shape:

```sql
-- spine
patient(id, external_ref, first_name, last_name, birth_date, pronouns,
        contact_json, status, created_at, archived_at)

consent(id, patient_id, kind, scope, text_version, text_hash,
        granted_at, revoked_at, note)

session(id, patient_id, scheduled_at, started_at, ended_at, duration_s,
        modality, setting, status, consent_id, therapist_note)

-- capture: 4 modes collapse into these two tables
session_source(id, session_id, kind,          -- session_audio|dictation|typed|hybrid
               role_map_json, language, created_at)
audio_asset(id, source_id, path, sha256, bytes, duration_ms, codec,
            encrypted, retention_delete_after, deleted_at)

-- transcription
transcript(id, session_id, source_id, engine, model, model_version,
           language, status, created_at)
transcript_segment(id, transcript_id, idx, start_ms, end_ms,
                   speaker_label, speaker_conf, text, edited)

-- generation
report(id, session_id, template_key, template_version, kind,
       status,                              -- draft|in_review|signed|exported
       model, provider, prompt_version, prompt_hash, params_json,
       created_at, signed_at, signed_by, pdf_path)
report_section(id, report_id, key, heading, body, origin, -- ai|human|mixed
               confidence, edited_at)
report_citation(id, section_id, segment_id)  -- sentence → transcript segment

-- supporting
template(id, key, version, payer, kind, locale, schema_json, typst_path, active)
job(id, kind, entity_id, status, progress, attempts, last_error, created_at)
audit_event(id, at, actor, action, entity_type, entity_id, meta_json,
            prev_hash, hash)                  -- append-only, hash-chained
goal(id, patient_id, session_id, text, status)  -- v1.5, schema ready now
app_setting(key, value_json)
```

**Notes**
- `report_citation` is the differentiator: it lets the UI show "this sentence came from 14:22" and makes the report defensible. Design it in now; it's painful to retrofit.
- `job` makes transcription resumable and lets long work survive an app restart.
- `audit_event` hash chain: each row hashes the previous, so tampering is detectable. Cheap, and it's a strong thing to show an auditor.
- Soft-delete only where legally required (patient erasure = hard delete + audit).

---

## 6. The four capture modes

| Mode | Records patient voice | Consent needed | Typical use |
|---|---|---|---|
| 1. Session recording | yes | yes | Therapist comfortable with recording |
| 2. Post-session dictation | no | no | Patient declines recording — **the cleanest legal path** |
| 3. Typed notes | no | no | Short sessions, sensitive topics |
| 4. Hybrid | yes | yes | Recording + therapist's own 2-min summary appended |

**Critical abstraction:** the pipeline never sees "an audio file." It sees a list of `SourceSegment { role, start, end, text }`. All four modes converge there. Getting this right in Phase 1 prevents a rewrite in Phase 3.

Mode 2 deserves its own product surface: three guided prompts (what happened / what changed / goals & next steps) and a 3-minute dictation. It is cheaper, faster, legally simpler, and many therapists will prefer it. Market it explicitly.

---

## 7. Pipeline

Deterministic, five bounded stages. No agency, no tool-choosing, no loops.

```
1 INGEST     audio file(s) | dictation | typed text  → normalized 16k mono
2 SPEECH     VAD → diarize → transcribe → align → SourceSegment[]
3 EXTRACT    SourceSegment[] + template schema → validated JSON findings
4 RENDER     findings + template → sectioned draft (typst for PDF, text in DB)
5 REVIEW     human edits → checklist → sign-off → PDF + citations + audit
```

**Design rules**

- Stage 3 output is **schema-constrained JSON**, validated with `schemars`/`serde` + a JSON Schema. Invalid → retry once with the validation error → fail loudly. Never "best effort" into the report.
- Stage 4 is a **template render**, not free generation. Prose polish is allowed *inside* a section, bounded by a max-length and a "no new facts" instruction.
- Every report stores `model`, `provider`, `prompt_version`, `prompt_hash`, `params`. Reproducibility is a feature you will be asked about.
- Nothing auto-sends. Nothing auto-signs. The therapist signs.

**Self-check checklist** (stage 5, shown as a gate before sign-off):
- Every required template section non-empty
- No hallucinated names, dates, or medications (string-diff against findings)
- Session duration and date match metadata
- Speaker attribution present for every claim
- No diagnostic language unless the therapist typed it
- Report length within payer norms

---

## 8. Providers & model policy

### 8.1 Abstraction

One `LlmProvider` trait, one `SttProvider` trait. Implementations: `openai-compatible` (covers Requesty, OpenRouter, Azure EU, local llama.cpp), `whisper-rs`, and cloud STT. **No provider-specific code paths.** Provider choice is a config row, switchable without a release.

### 8.2 Local model sizing (Q4, plus ~2 GB runtime overhead)

| Model | Params | Disk | Comfortable on |
|---|---|---|---|
| Qwen3 4B | 4B | ~2.5 GB | 8 GB RAM — usable |
| Qwen3 / Llama 3.1 8B | 8B | ~4.7 GB | 16 GB RAM — realistic laptop ceiling |
| Gemma 3 12B | 12B | ~7.5 GB | 24 GB RAM / 8–12 GB VRAM |
| Qwen3 14B / Mistral Small 24B | 14–24B | ~9–14 GB | 32 GB RAM / 16 GB+ VRAM |
| Qwen3 32B | 32B | ~19 GB | 32 GB tight; wants 24 GB VRAM |

STT: `whisper.cpp` large-v3-turbo q5 ≈ 0.9 GB, ~0.3–0.5× realtime on CPU.

**Policy:** cloud default; local offered after a first-run probe (RAM, VRAM, CPU) that estimates minutes/session. Local tier = "your data never leaves the practice," slower. This is the pricing ladder.

### 8.3 Provider shortlist to evaluate in the spike

| Role | Candidates | Note |
|---|---|---|
| LLM (cloud) | Requesty (EU), OpenRouter, Azure OpenAI EU, Mistral | evaluate German quality on real transcripts |
| STT (cloud) | Gladia (FR/EU, diarization), Speechmatics (UK, diarization), Azure Speech EU, Mistral Voxtral, Deepgram | diarization quality is the deciding factor |
| STT (local) | whisper-rs + ONNX speaker embeddings | no cloud dependency |
| Embeddings/RAG | same provider as LLM | for the history-retrieval feature (v2) |

Verify each provider's current entity, EU residency, DPA terms, and zero-retention option **before** it goes in a build.

### 8.4 Proxy contract (control plane)

```
POST /v1/draft     { plan, provider, model, schema, segments[], template }  → streamed sections
POST /v1/stt       (multipart audio)                                        → segments[]
GET  /v1/entitlement                                                        → plan, caps, used
POST /v1/license/activate | /refresh
GET  /v1/templates                                                          → pack versions
```

Invariants: no request bodies persisted; logs contain token counts, latency, provider, status only; per-license rate caps; graceful degradation to local when the proxy is unreachable.

---

## 9. UX & module map

```
/                      Dashboard: drafts awaiting review, sessions this week, storage, privacy status
/patients              List + search + archive
/patients/:id          Timeline · sessions · documents · goals (v1.5) · export · erase
/sessions              List, filters, status
/sessions/:id          ▸ Capture (mode selector, consent gate, level meter)
                       ▸ Transcript (speakers, timestamps, click-to-play, edit)
                       ▸ Bericht (sectioned draft, per-section regenerate, citations)
                       ▸ Review (checklist, sign-off, PDF)
/reports               Queue: draft · in review · signed · overdue
/templates             Installed packs, version, preview
/settings              Providers & keys · privacy · retention · backup · updates
/audit                 Append-only event log
--- module registry (later) ---
/calendar  /billing  /supervision  /admin
```

**Module registry rule:** each module registers routes, its own migration namespace, and dashboard widgets. Patient is the spine. This keeps the "all-in-one for therapists and self-employed" vision real without shipping five half-products.

**Editor design:** sections with `origin` (ai/human/mixed), inline citation chips linking to transcript timestamps, per-section "regenerate with instruction," and a diff view of AI vs. edited text (useful for the eval set later).

---

## 10. Security & privacy implementation

| Concern | Implementation |
|---|---|
| Encryption at rest | SQLCipher DB; audio encrypted AES-256-GCM with per-file DEK wrapped by a master key |
| Master key | OS keychain (`keyring`); never on disk in plaintext |
| Backups | `age`-encrypted archive, scheduled, to a user-chosen path (local disk / their cloud drive), restorable from within the app |
| Transport | TLS only; certificate pinning for the control plane |
| Logging | Redaction layer; no transcript text, no names in logs; log level capped in release |
| Crash reporting | **opt-in**, content-free (stack, version, counters). A hard allowlist of fields |
| Retention | Audio auto-delete N days after sign-off (configurable, default 30); transcripts kept; both surfaced in Settings |
| Erasure | Hard delete patient → cascades + audit entry |
| Access | Single-user app; optional app passcode + auto-lock after inactivity |
| Updates | Tauri signed updater; never auto-update during an active session |
| Export | Lossless: PDFs + JSON + transcripts + audit; no lock-in |

---

## 11. Eval harness (`crates/eval`)

Built in Phase 2, extended forever. Without it you cannot change a model or prompt safely.

- **Fixtures:** 10–20 consented sessions, anonymized, each with the therapist's own real Bericht as the reference.
- **Metrics:**
  - WER (German) on the transcript
  - **DER** (diarization error rate) + speaker-swap rate — the make-or-break metric
  - Schema validity rate of stage 3 output
  - Field-level precision/recall of extracted findings vs. the reference report
  - Section-level human rubric (1–5) on a sample
  - Cost and wall-clock per session
- **Command:** `cargo run -p eval -- --suite de-therapy-v1 --provider <x> --model <y>`
- **Gate:** no prompt or model change ships if DER or rubric scores regress.

---

## 12. Phases

Estimates are solo-dev working days and assume the client supplies materials on time.

### Phase 0 — Discovery & quality spike (5 d) — **GO / NO-GO GATE**

| # | Task | Output |
|---|---|---|
| 0.1 | Collect from client: 5–10 anonymized Berichte, actual templates, payer mix, laptop specs | `fixtures/`, `templates/` |
| 0.2 | Collect 3 consented session recordings | `fixtures/audio/` |
| 0.3 | Hand-transcribe 20 min of one session as ground truth | WER/DER baseline |
| 0.4 | Run STT + diarization candidates, measure WER, DER, speaker-swap, time, cost | comparison table |
| 0.5 | Run 2 LLM candidates on a real transcript → draft vs. the therapist's real report | qualitative gap report |
| 0.6 | Write the one-page findings + recommended default provider | decision |

**Exit criteria:** speaker-swap rate low enough that the therapist says "yes, that's usable," and a per-session cost that supports €99/mo.
**If it fails:** the project pivots to mode 2/3 (dictation + typed notes, no session recording) — still valuable, and it's the cheaper build.

### Phase 1 — Walking skeleton (12–15 d)

- 1.1 Workspace, `core` crate skeleton, repo traits, migrations, `refinery` wiring
- 1.2 Tauri shell + TanStack Router + sidebar + module registry
- 1.3 Patient CRUD + session CRUD + timeline
- 1.4 Consent capture UI + records
- 1.5 Audio import (file picker + drag-drop) → `audio_asset`
- 1.6 Recording engine (`cpal`, 5s crash-safe chunks, device picker, level meter, disk-space guard, keep-awake)
- 1.7 Cloud STT via proxy → `transcript` + `transcript_segment`
- 1.8 Transcript viewer: speakers, timestamps, click-to-play, inline edit
- 1.9 Manual report from a template → sectioned form → PDF (typst)
- 1.10 Audit log + Settings shell (retention, backup stub)

**Exit:** the therapist uses it on real sessions for two weeks with manual drafting. **No LLM yet.** This buys you real data and real feedback before you automate anything.
**Ship to pilot here.**

### Phase 2 — Drafting pipeline (12–15 d)

- 2.1 `SourceSegment` normalization across all 4 capture modes
- 2.2 Stage 3: schema-constrained extraction + validation + retry
- 2.3 Stage 4: template render, per-section generation, bounded polish
- 2.4 `report_citation` wiring (section sentence → segment → timestamp)
- 2.5 Review UI: per-section regenerate with instruction, origin badges, diff
- 2.6 Checklist gate before sign-off
- 2.7 Sign-off + PDF + archive + audit entries
- 2.8 Prompt versioning + `crates/eval` + golden set
- 2.9 Second template pack (e.g. Verlaufsbericht vs. Erstbericht) to prove the template engine generalizes

**Exit:** 3 consecutive sessions where the therapist's edit time is <10 min and they'd sign the draft.
**This is the real milestone** — it's the moment the product exists.

### Phase 3 — Trust & polish (8–10 d)

- 3.1 SQLCipher + keychain + audio envelope encryption
- 3.2 Encrypted scheduled backup + restore + verified restore test
- 3.3 Retention job + audio auto-delete + visible privacy status
- 3.4 Local STT (`whisper-rs`) + local LLM option + first-run hardware probe
- 3.5 Per-session "what leaves this machine" indicator
- 3.6 App passcode + auto-lock
- 3.7 Log redaction pass; crash reporting (opt-in, allowlisted)
- 3.8 Signed auto-update + Windows signing + macOS notarization
- 3.9 Lossless export (PDF + JSON + transcripts + audit)
- 3.10 Error states, empty states, offline behaviour, migration dry-run

**Exit:** you can hand it to a stranger and they can't lose data or leak it by accident.

### Phase 4 — Productization (10–12 d)

- 4.1 Control plane: auth, license/entitlement, model proxy, metering
- 4.2 License model: offline-capable, **30-day grace period** — never brick a therapist mid-session
- 4.3 Stripe: trial, monthly/annual, plan caps
- 4.4 Landing page: the privacy story is the hero; a 90-second demo video; German copy
- 4.5 Onboarding: hardware probe, provider choice, first patient, first report
- 4.6 Template packs + versioned distribution
- 4.7 Docs: therapist guide, IT/DSB one-pager, AVV template, subprocessor list
- 4.8 Pricing page + trial flow + cancellation

**Exit:** a therapist who has never met you can buy, install, and produce a signed Bericht unaided.

**Total: ~50–57 working days (≈11–13 weeks solo).**

---

## 13. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| **Diarization swaps speakers** | Every downstream sentence wrong; product worthless | Phase 0 gate; DER metric; manual speaker-fix UI as fallback; mode 2/3 pivot |
| German STT quality on soft therapy speech | Usable but exhausting to edit | Spike multiple engines; vocabulary/prompt priming with the therapist's terms; custom word list |
| Client's laptop is weak | Local tier unusable, cloud-only | Hardware probe; cloud default; don't promise local |
| Scope creep to full practice management | v1 never ships | Module registry + written v1 freeze + Phase 4 gating |
| Provider terms change / US-only | Legal exposure, rework | Provider-agnostic trait; proxy lets you swap without a release; contract on EU residency |
| Report hallucinates a fact | Clinical/legal incident | Schema-constrained extraction, citation binding, checklist, mandatory sign-off, no auto-send |
| Signing / notarization delays | Blocks delivery | Start certificates and Apple notarization in Phase 1, not Phase 3 |
| Templates arrive late or vague | Rework | Make them task 0.1; refuse to design the schema from imagination |
| Solo-dev bus factor | Client stranded | Everything in `core`, documented, migratable, lossless export |

---

## 14. Open questions for the client

1. Jurisdiction and payer mix — DE/AT/CH? Kasse (EBM/GOP) or private/Heilpraktiker (GebüH)?
2. Laptop OS + RAM + GPU?
3. Templates and 5–10 anonymized Berichte — available now?
4. Which report kinds matter most: Erstbericht, Verlaufsbericht, Verlängerungsantrag, Abschlussbericht, Gutachten?
5. Will they pay a monthly subscription after delivery? (Validates pricing; decides whether Phase 4 starts immediately.)
6. Do they have an existing patient-record system this must coexist with, and an export format they need?
7. Who is their Datenschutzbeauftragte/r or Berufsverband contact? (They will be asked about this app.)

---

## 15. Commercial

**Positioning:** "The documentation assistant that never sends your sessions anywhere."

**Pricing (draft):**

| Tier | Price | Includes |
|---|---|---|
| Local | €79/mo | All features, local models only, fully offline, unlimited sessions |
| Cloud | €129/mo | Local + cloud drafting/STT with generous fair-use cap, priority support |
| Practice | €249/mo | Multi-seat (later), shared template packs, admin export |

COGS per session: cents. 20 sessions/month × ~40 min saved ≈ 13 h/month → €129 is trivially justified.

**Build fee:** separate from subscription. The pilot fee buys the build; the subscription buys the product. Do not let the build fee set the subscription price.

**Contract essentials:** scope doc stating "draft for review and signature, not a diagnostic device"; data-processing terms; who owns the data (the therapist, always); lossless export guarantee; support SLA; what happens on termination.

---

## 16. Immediate next actions

1. Send the client the Phase 0 material request (templates, anonymized Berichte, laptop specs, payer mix, 3 consented recordings).
2. Book the Phase 0 spike week; do not write app code before it.
3. Start the certificate/notarization paperwork in parallel (it has lead time).
4. Draft the one-page scope + MDR-boundary statement for signature.
5. Decide whether Phase 4 starts immediately after Phase 2 (i.e., whether the pilot becomes a paying subscriber).
