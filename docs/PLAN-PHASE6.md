# Phase 6 plan (approved 2026-11-07): Weitere Berichte + Briefe (quick AI writing)

## Context
The pilot therapist sent four bundles of her real letters:
- `Gutachten-Umwandlungsanträge.pdf`: PTV 3 Berichte zum Umwandlungsantrag (points 1–7, Chiffre top right).
- `Berichte-Rentenversicherung.pdf`: Befundberichte to the DRV (Beurteilung psychischer Befund, Weitergewährung EM-Rente).
- `Berichte-Pflegekasse.pdf`: Stellungnahmen for a Pflegegrad (to the Pflegekasse or MD-Gutachter:in).
- `Berichte-Landratsamt.pdf`: Befundberichte to the Versorgungsamt (GdB, Merkzeichen, Widerspruch; answering the Amt's numbered questions), plus Befundberichte to doctors (Neurologe, Hausärztin).

Today Theralo only knows PTV 3 Berichte. These other documents are a large share of her writing work, and they look alike:
- the same letterhead
- recipient address, place/date, Betreff with name, birth date and Versicherungsnummer/Aktenzeichen
- "Sehr geehrte Damen und Herren"
- Behandlung seit…, ICD-10 diagnoses, symptoms, Funktionseinschränkungen, Prognose/Behandlungsbedürftigkeit
- "Bei Fragen stehe ich Ihnen gerne zur Verfügung", signature block with titles

She reuses whole paragraphs nearly word for word (the Schmerzgedächtnis/Teufelskreis paragraph, the list Kognitiv/Emotional/Motivational/Vegetativ/Verhaltensebene/Sozialebene, "Ziel der psychotherapeutischen Behandlung ist es…").

Goals:
1. **Berichte**: the new types can be picked in "Neuer Bericht", and the Lotse guides her through them like a PTV 3 Bericht (Start · Fragen · Abschnitte · Prüfen · Fertig).
2. **Briefe** (new sidebar route): a fast path. She picks the patient, type and recipient, optionally pastes or scans the request, and gets one click to an AI draft from the templates + patient material. She edits, signs, exports and marks it as sent.
3. **Afterwards**: the letter appears on the patient's Verlauf timeline, with a Wiedervorlage/Frist on Heute when needed, and its Schweigepflichtentbindung is checked.

User decisions (2026-11-07):
- The PDFs are used for **structure only**. The real letters never go into the repo; built-in templates get our own hints and fictional wording. She can upload her own old letters in the app as private, pseudonymized style examples.
- **Textbausteine library in v1.**
- **Full DIN 5008 letter** with uploadable logo and a footer (address, phone, bank, Steuernummer).
- **Recipient address book**, **patient fields for the Betreff** (address, Kasse, Versicherungsnummer, per-letter Aktenzeichen), and **paste or scan the request** (the AI answers its questions point by point).

Guiding rule, as in Phase 5: **replace, don't add**. One document model and one Lotse; "Briefe" is the fast view of the same document, not a second editor.

---

## Product spec

### Document types (built-in templates, `templates/briefe/*.yaml`)
| Key | Title (UI) | Typical recipient | Sections (fictional hints, derived from her structure) |
|---|---|---|---|
| `drv-befund` | Befundbericht Rentenversicherung | DRV (EM-Rente, Weitergewährung, Reha) | Anlass · Behandlung seit / Setting (language, frequency) · Diagnosen (ICD-10) · Somatische Begleiterkrankungen (optional, "nicht relevant") · Psychopathologischer Befund & Symptomatik (Affekt, Schmerz/Somatik, Schlaf, Kognition, Antrieb/Belastbarkeit) · Psychotherapeutische Beurteilung (quantitatives/qualitatives Leistungsvermögen, Prognose) · Behandlungsbedürftigkeit · Zusammenfassende Empfehlung |
| `pflege-stellungnahme` | Stellungnahme Pflegegrad | Pflegekasse / MD-Gutachter:in | Anlass (Befürwortung) · Diagnosen · Anamnese & Behandlungsverlauf · Klinischer Befund & Belastungsfaktoren (psychisch, sozial, körperlich) · Neuropsychologische Funktionseinschränkungen (kognitiv/emotional with "Konkrete Beobachtungen") · Auswirkungen auf Selbstversorgung & Alltag (hints along the NBA modules 2, 3, 5, 6) · Fazit & Empfehlung |
| `versorgungsamt-befund` | Befundbericht Versorgungsamt (GdB) | Landratsamt / Versorgungsamt | Bezug (Anschreiben vom…, Widerspruch, Merkzeichen) · Datum der letzten Behandlung / Behandlung seit · Diagnosen · Psychopathologischer Befund (Art, Häufigkeit, Dauer) · Auswirkungen auf Teilhabe (soziale Integration, Alltag, Beruf, kognitive Belastbarkeit) · Medikation · Therapie & Behandlungsintensität · Einschätzung & Prognose. Hints follow the Versorgungsmedizinische Grundsätze (Teilhabe across domains); the letter doesn't propose a GdB number unless she writes one. |
| `arztbrief` | Befundbericht an Ärzt:in | Hausärztin, Neurologe, Psychiater | Anlass · Behandlung seit / letzte Behandlung · Diagnosen · Befund & Verlauf · Therapie & Empfehlung |
| `bescheinigung` | Behandlungsbestätigung | Patient:in, Arbeitgeber, Schule | One short section (in Behandlung seit…, frequency); no diagnoses unless she adds them |
| `freier-brief` | Freier Brief | anyone | One free section; the AI writes from her instruction |

- `category` per template: `ptv3` (existing), `bericht` (DRV/Pflege/Versorgungsamt/Arzt), `brief` (Bescheinigung/frei). The Berichte board shows `ptv3` + `bericht`; the Briefe page shows `bericht` + `brief`. Both open the same document.
- **Umwandlungs-Gutachten:** these are the existing PTV 3 Umwandlung. Compare `templates/ptv3/erst-umwandlung.yaml` against her 1–7 structure (Familienanamnese/Vorbehandlungen/Psychischer Befund inside 2; Prädisponierende/Auslösende/Aufrechterhaltende Faktoren in 4; Prognose in 6; Verlauf in 7). Adjust hints and asks where they differ. Her style comes from her uploaded exemplars (slice 49), not from the repo.
- Each template gets `asks:` (Fragen step), e.g. DRV: "Seit wann in Behandlung, wie oft, in welcher Sprache?", "Befristung der Rente bis?", "Arbeitsfähig: unter 3 h / 3–6 h / über 6 h?"; Pflege: "Wer unterstützt im Alltag?", "Stürze, Orientierung, Termine vergessen?"; Versorgungsamt: "Aktenzeichen, Anschreiben vom?", "Merkzeichen beantragt?". She can tick "nicht relevant" as in slice 37.
- **Betreff pattern** per template, filled locally: e.g. `Beurteilung psychischer Erkrankung von {Frau|Herrn} {Vorname Nachname}, geb. {Geburtsdatum}`, plus reference lines (Versicherungsnummer, Aktenzeichen, Kennzeichen, BSNR…) from the patient and the letter.

### Letter layout (DIN 5008, PDF + DOCX)
- Letterhead: practice name + subtitle centred, logo top right, sender line above the address window, recipient address, then right-aligned "Ort, den Datum", bold Betreff + reference lines, salutation, body sections (heading per section, optional numbering), closing ("Bei Fragen stehe ich Ihnen gerne zur Verfügung." / "Mit freundlichen Grüßen"), signature block (name + title lines), optional signature/stamp image.
- Footer on every page: practice address, phone/fax/e-mail, bank (IBAN/BIC), Steuernummer; page numbers from page 2.
- PTV 3 Berichte keep their current layout (Chiffre, no names).

### Settings → Praxis (replaces the free `practice_header` text; migrated into the new fields)
Praxisname, Untertitel, Inhaber:in, Straße, PLZ/Ort, Telefon, Fax, E-Mail, Bank, IBAN, BIC, Steuernummer, Titelzeilen for the signature (multi-line, e.g. "Diplom-Psychologin / Psychologische Psychotherapeutin (VT)"), default place for "Ort, den". Logo upload (PNG/JPEG, normalized like the patient photo, stored in the encrypted DB). Optional signature/stamp image, off by default, with a note. Live preview of the letterhead.

### Patient fields (patient form, collapsible "Für Briefe")
Anschrift (Straße, PLZ, Ort), Krankenkasse, Versichertennummer (KV), Rentenversicherungsnummer. Per letter: free reference lines (label + value, e.g. Aktenzeichen, Kennzeichen, BSNR). The last value per recipient + patient is remembered as a suggestion.

### Recipient address book (`/briefe/empfaenger` + picker)
- Entry: kind (Rentenversicherung · Pflegekasse/MD · Versorgungsamt · Ärzt:in · Krankenkasse · Sonstige), name, department, address lines, salutation ("Sehr geehrte Damen und Herren," / "Sehr geehrte Frau Dr. med. Poppe,"), default document type.
- Picker with search; "neu anlegen" inline. A recipient typed in a letter is saved to the book on signing (opt-out). A patient can link recipients ("Hausärztin", "Psychiater:in") for one-click picks.
- Address and salutation never go to the AI; they are filled in locally.

### Briefe route (sidebar "Briefe", between Berichte and Kalender; Strg+K "Neuer Brief", "Briefe")
- **List**: rows with patient (avatar, name), type, recipient, date, status (Entwurf · Fertig · Versendet), Wiedervorlage; filter chips per status/type, search; empty state.
- **"Neuer Brief" dialog**: patient (prefilled when opened from the patient page) → type tiles (`bericht` + `brief` categories; "Empfohlen" by recipient kind) → recipient → optional request (paste text, or "Anschreiben scannen/hochladen") → "Erstellen". Also on the patient page ("Brief schreiben") and from the Berichte board.
- **Quick page `/briefe/$id`** (one screen, no steps):
  - Left: the letter as an A4 document (the existing "Ganzes Dokument" editor with the new letter frame: address, Betreff, salutation editable inline), toolbar with mic and Textbausteine.
  - Right rail: "Anschreiben" (the request and its extracted questions with ✓ when answered), "Material" (what the AI may use, with checkboxes: Diagnosen of the Behandlungsfall, bestätigte Fakten, Sitzungs-Zusammenfassungen, last Bericht/Brief, her answers), "Woher kommt das?" for the focused sentence.
  - One primary button **"Mit KI schreiben"** (all empty sections) → redaction preview (one click, "Senden") → streamed draft into the sections, cancelable. Per section: "Neu schreiben" with an instruction ("kürzer", "mehr zur Alltagsbewältigung"), "Textbaustein einfügen".
  - "Geführt bearbeiten" opens the Lotse for the same document (Berichte route) when she wants the Fragen step and checks.
  - Bottom bar: "Prüfen & unterschreiben" → checks (below) → sign → PDF/DOCX → "Als versendet markieren" (date + Weg: Post · Fax · E-Mail · eArztbrief/KIM · persönlich).

### Request handling ("Anschreiben")
- Paste text, or upload PDF/image. Text PDFs are read locally; scans and photos go through **local OCR** (Windows.Media.Ocr, German, no download, nothing leaves the machine). Unreadable → "bitte abtippen oder diktieren".
- The AI (pseudonymized) extracts the numbered questions/requests ("1./2. Erbetene Befunde ab 2013 / Datum der letzten Behandlung", "3. Depressionen (Art, Häufigkeit und Dauer)") → they become the letter's sections in the Amt's own order and wording, plus the template's Diagnosen section. She can reorder, rename or drop them. A deadline in the request ("bis zum …") is proposed as the Frist.

### AI writing (`core::letter`, prompt `letter-2026.1`)
- Input: template (sections, hints, audience), her answers, selected material (facts with tags, summaries, episode diagnoses, last signed Bericht/Brief as "Vorbericht"), request questions, the type's Textbausteine, and 1–2 of her own style exemplars for the type (slice 49). Everything is pseudonymized with one token map (`Pseudonymized<T>`). Names are restored locally in the output, since these letters are named (like the workshop's Sitzungsnotiz audience). Recipient, address and Betreff are never sent.
- Output: per-section prose with source markers per sentence (`[F n]` fact, `[A n]` her answer, `[V n]` Vorbericht, `[T n]` Textbaustein, `[Q n]` request). The existing `draft::unsupported` grounding runs, but in Briefe ungrounded sentences are **kept and highlighted "ohne Beleg"** rather than silently dropped; she must accept or delete them before signing. Third-party sentences are dropped as today.
- Gender/pronoun forms come from the patient record (Herr/Frau, er/sie).
- Audit `llm.letter` (counts only), spend task "Briefe", provider/model/prompt version stored on the report.

### Textbausteine (`/briefe/textbausteine` + inline)
- Entry: title, body (rich text), types it applies to (or all), optional section, placeholders `{Anrede}`, `{Name}`, `{Nachname}`, `{er/sie}`, `{seine/ihre}`, `{Patient/Patientin}`, `{Behandlung seit}`, `{Diagnosen}`, resolved locally from the patient.
- Insert via the toolbar button or `/` in the editor (search by title). Select text → "Als Textbaustein speichern". Usage count drives the order.
- Built-in set: ~10 generic blocks written by us (Teufelskreis Schmerz, Funktionsebenen-Liste, Behandlungsziel Stabilisierung, Schlussformel, Behandlung in der Muttersprache…), origin `builtin`, copyable and editable as her own.
- The AI gets the type's blocks as "bevorzugte Formulierungen", to adapt only where her material supports them.

### Checks before signing (Prüfen step / quick-page dialog)
- Recipient address complete; Betreff references present (a template can mark a reference as required, e.g. Versicherungsnummer for DRV).
- **Schweigepflichtentbindung** for this recipient: a new consent kind `release_of_information` in `core::consent` (per patient, recipient or kind, date, optional scan). Missing → warning with "jetzt erfassen" (not a hard block; she may have it on paper).
- Unanswered request questions; sentences "ohne Beleg"; empty required sections; diagnoses differing from the Behandlungsfall (as L6).
- Optional AI "Empfänger-Check" (generalized Gutachter-Check, `core::review`): does the letter answer the request and describe function/Teilhabe concretely enough for this recipient?

### After completion (status, timeline, follow-up)
- Status: Entwurf → Fertig (signed; read-only, part of the Dokumentation per § 630f BGB; corrections as a Nachtrag, or as a "korrigierte Fassung" that links the original) → Versendet (date, Weg). Exports are marked unsigned as today.
- **Timeline**: new node "Brief" in the patient's Verlauf (type, recipient, status, date sent, open PDF), grouped into the Behandlungsfall (the newest open one, changeable). Filter "Alles | Sitzungen | Briefe & Berichte".
- **Wiedervorlage / Frist**: optional date on the letter (e.g. EM-Rente befristet bis 31.01.2027 → reminder 3 months before; the Amt's deadline) → task on Heute ("Brief an DRV fällig", "Wiedervorlage: Rentenbescheid?").
- Delete: a draft is deleted; a signed letter goes to the Papierkorb with the retention warning (as slice 43). `erase_patient` cascades.
- Audit: `letter.create/sign/export/send/delete`, `recipient.*`, `textblock.*` (ids and counts only, no text).
- Assistant: read tool `get_letters`, proposal tool `propose_letter_text` (later in the phase).
- Hilfe article "Briefe & weitere Berichte", Strg+K entries, a Demo-Praxis letter (Lena: Befundbericht Versorgungsamt draft), a Rundgang hint on the Briefe entry.
- Later (not in this phase): billing of Befundberichte (JVEG/DRV fees) once a Rechnungen module exists; sending via KIM/fax.

---

## Technical design

### Data model (migration V25, `crates/core/src/store/migrations/`)
- `report.kind` gains `brief` (check for a CHECK constraint on `kind`; rebuild the table if there is one). `kind='bericht'` stays PTV 3 only, so the ~12 hardcoded `kind='bericht'` filters (focus.rs, review.rs, calendar.rs, mod.rs:470/1785, trash) stay correct. Review each one: trash and delete must include `brief`.
- `letter` (report_id PK → report cascade, patient_id, episode_id NULL, recipient_id NULL, recipient_json (snapshot), salutation, subject, refs_json, place, letter_date, request_text, request_questions_json, material_json, sent_at, sent_via, follow_up_on, follow_up_note).
- `report_section` + `heading TEXT NULL`, `ord INTEGER NULL` (dynamic sections from the request; template sections keep NULL).
- `recipient` (id, kind, name, department, lines_json, salutation, default_template, use_count, timestamps) and `patient_recipient` (patient_id, recipient_id, role).
- `patient` + `street`, `postal_code`, `city`, `insurer`, `insurance_no`, `pension_no` (ignore the unused `contact_json`).
- `text_block` (id, title, body, template_keys_json, section_key, origin builtin|user, use_count, timestamps).
- `practice_asset` (kind logo|signature PK, JPEG/PNG blob); practice fields as `app_setting` keys `practice.*`. Migrate the old `practice_header` text into `practice.name` + address lines on first open.
- Consent kind `release_of_information` (+ `recipient_id`, `scope`) in the existing consent table.
- V27 (slice 49; V26 went to slice 47's `draft_sentence.unsupported`): `style_exemplar` (id, template_key, section_key NULL, redacted_text, source sha256, created_at) as §3 of PLAN-AMENDMENTS planned; `source_document` (exists from V1) holds the upload.

### Core (`crates/core`)
- `templates.rs`: `Template` + `category`, `audience` (gutachter | behoerde | arzt | patient | frei), `layout` (bericht | letter), `subject`, `refs` (required reference labels), `closing`; `report_kinds` stays for `ptv3`. Add `template_for_letter(key)` next to `template_key_for(kind)`. `BUILTIN` gains the six `templates/briefe/*.yaml`. `validate()` rules for the new fields.
- `draft.rs`: `Frame.kind: ApplicationKind` becomes `Frame.target: Target { Ptv3(ApplicationKind), Letter { audience, title } }`; `system_prompt` picks Gutachter vs letter wording. The Lotse's Abschnitte step then drafts letter-type Berichte with the same machinery.
- New `letter.rs`: `prepare` / `preview` / `parse` for the quick one-shot draft and section re-write, based on `workshop.rs` (all sections in one request) + the citation and grounding pieces from `draft.rs` (`unsupported`, third-party filter); `extract_questions(request)`; placeholder resolver for Textbausteine. `Effort::Medium`.
- `render/`: new `brief.typ` (DIN 5008 Form B, header/footer, logo) and a DOCX letter builder (header/footer with `docx_rs`); `build_doc` gets a `Layout::Letter { letterhead, recipient, subject, refs, salutation, closing, signature }` branch. The pattern is `einwilligung.typ`, which already takes therapist and practice.
- `pii.rs`: letter templates have `rules.pseudonymized: false`, so the checklist doesn't flag names (already honoured).
- `ocr.rs` (slice 49): Windows.Media.Ocr via the `windows` crate (feature-gated, Windows only); PDF text via `pdf-extract`.
- Store: `create_letter`, `letter`, `list_letters(filter)`, `save_letter_meta`, `set_letter_sections` (dynamic), `sign_report` reused, `mark_letter_sent`, recipients CRUD, text blocks CRUD + builtin seeding (like `seed_builtin_templates`), `patient_timeline` → `TimelineItem::Letter`, due kind `letter_follow_up` for Heute, `SpendTask::Letter`.

### App (`crates/app/src/lib.rs`)
Commands: `letter_create`, `letter_get` (ReportView + letter meta), `letter_list`, `letter_meta_save`, `letter_preview` / `letter_run(digest, request_id)` (streamed, cancelable, `letter-progress`, same digest pattern as `draft_preview`/`draft_run`), `letter_section_rewrite_*`, `letter_questions_*`, `letter_mark_sent`, `request_import(path)` (text/OCR), `recipient_*`, `textblock_*`, `practice_logo_set/remove`. `report_sign`/`report_export`/`report_delete` accept `brief`. ts-rs bindings regenerated.

### UI (`ui/src`)
- `components/app-shell.tsx` `modules` + `lib/locales/de.ts` `nav.briefe`; `components/command-palette.tsx` entries.
- `routes/briefe/index.tsx` (list), `routes/briefe/$briefId.tsx` (quick page), `routes/briefe/empfaenger.tsx`, `routes/briefe/textbausteine.tsx`.
- `components/briefe/`: `new-letter-dialog.tsx`, `letter-frame.tsx` (address, Betreff, salutation inline inside the A4 document view), `material-rail.tsx`, `request-card.tsx`, `recipient-picker.tsx`, `textblock-menu.tsx` (toolbar + `/` suggestion via TipTap), `send-dialog.tsx`.
- Reuse: `report-document.tsx` / `lib/editor.ts` (document editor), `RichField`, `EditorToolbar` + `DictationButton` (dictation entity type `letter` in `log_dictation`), `RedactionDialog` (`fact-draft.tsx:504`), `PatientLabel`, `PatientAvatar`, `FilterChip`, `storedChoice`, `DeleteReport`, `HelpLink`.
- Lotse: `lotse/new-report.tsx` gets two groups ("Gutachterverfahren (PTV 3)" / "Weitere Berichte"); `start-step.tsx` shows recipient, references and request for letter templates instead of the Antragsart tiles; `check-step.tsx` adds the letter checks; `finish-step.tsx` uses the letter preview + "Als versendet markieren". `lib/lotse.ts` reads the template category instead of assuming PTV 3.
- `components/patient-timeline.tsx`: `LetterNode` + filter; invalidate `['timeline', patientId]` after letter changes.
- `routes/settings/allgemein.tsx` → "Praxis & Briefkopf" card (fields, logo, preview); `patient-form.tsx` "Für Briefe" fields; `routes/patients/$patientId.tsx` "Brief schreiben" + linked recipients.
- `content/hilfe/briefe.md`, `lib/help.ts` slug.

---

## Slices (each ends with: tests green → commit → push main → `npm run app:build` → ask about a release; decisions into PLAN-AMENDMENTS §31+)

**Step 0 (on approval):** save this PRD as `docs/PLAN-PHASE6.md`, add a pointer in `docs/HANDOFF.md`, commit + push. Optional: the user generates mockups for the Briefe list, quick page and letter layout (as in Phases 4/5).

1. **Slice 45: Letter foundation (no AI).** V25 migration; Praxis & Briefkopf settings + logo; patient "Für Briefe" fields; recipient book; `brief.typ` + DOCX letter; `report.kind='brief'` with the `freier-brief` and `bescheinigung` templates; Briefe list + quick page without AI (manual writing, dictation); sign/export/mark sent; timeline node; delete/Papierkorb/erase cascade; audit; Strg+K + sidebar.
2. **Slice 46: New Bericht types + Lotse.** Template schema fields (category/audience/layout/subject/refs); YAML for `drv-befund`, `pflege-stellungnahme`, `versorgungsamt-befund`, `arztbrief` with hints + asks; `draft::Frame.target` generalization; "Neuer Bericht" groups; Lotse start/Fragen/Abschnitte/Prüfen/Fertig for letter templates; Schweigepflichtentbindung consent kind + check; PTV 3 Umwandlung hint review against her 1–7 structure. **The therapist should review the wording of hints and questions.**
3. **Slice 47: Quick AI writing in Briefe.** `core::letter` (one-shot draft, section re-write with instruction, "ohne Beleg" highlighting), material rail, pasted request → extracted questions → dynamic sections + Frist proposal, redaction preview, spend task, `llm.letter` audit.
4. **Slice 48: Textbausteine.** Table + builtin seed, library page, toolbar + `/` insert, placeholders, "Als Textbaustein speichern", AI use as preferred wording.
5. **Slice 49: Scan + style.** Request upload with PDF text extraction + local Windows OCR; her own old letters as pseudonymized style exemplars per type (upload → OCR → redaction review → section mapping light: whole text per type in v1) used as few-shot in `letter`/`draft` prompts.
6. **Slice 50: Follow-ups + polish.** Wiedervorlage/Frist on Heute, Empfänger-Check (generalized `core::review`), assistant `get_letters`/`propose_letter_text`, Hilfe article, Demo-Praxis letter, Rundgang hint, video shot list update (re-recording left to the user).

## Verification (per slice)
- `cargo test -p thera-core` (new tests: migration on an old DB copy, letter create/sign/send/erase cascade, timeline item, template validate + resolve for each new YAML, subject/placeholder resolution with gender, letter prompt pseudonymization: no name/birth date/address in the sent text, names restored, grounding keeps + flags; OCR behind `#[ignore]` with a fixture image), Vitest (`lib/` helpers), `npm run typecheck`.
- E2E in the debug app via CDP on an isolated data dir/keychain user (as in earlier slices), with the streaming mock LLM: create a letter from the patient page, paste a request with 3 numbered questions → 3 sections, draft, "ohne Beleg" shown, insert a Textbaustein with placeholders, sign, PDF export (check page 1 screenshot: logo, address window, Betreff, footer), mark sent → timeline node + Heute Wiedervorlage; the mock sees no name, address or birth date; audit chain intact without text. Lotse path for `drv-befund` from "Neuer Bericht" through Fertig.
- Real-key run of one quick letter and one guided DRV Bericht (user), and a test print of the letterhead by the therapist.
