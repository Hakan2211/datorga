# Phase 5 plan (approved 2026-10-29): UI fixes + the Bericht-Lotse (guided Bericht workflow)

## Context
The user (developer of thera; the pilot psychotherapist uses it) asked for four small UI improvements and, most importantly, a redesign of the way a Bericht is created. The goal is to make it easier and simpler for the therapist, without adding chaos.

The exploration found that the Bericht page today is one long editor. Its 5-step bar only scrolls the page. The other problems:
- Facts are reviewed somewhere else (each Sitzungsnotiz) and their tags can't be changed once applied.
- There are 5 entry points, and they behave differently.
- Each drafting round takes many clicks per section.
- The AI silently leaves out content no session fact backs: Medikation, Vorbehandlungen, Ziele, Setting, Prognose. So §4 and §6 stay thin, and nothing tells her what is missing.
- Every section must have text before she can sign, even Somatik or §7.
- Diagnoses are copied into the Bericht once and never synced.
- There are three AI paths with three different mental models: Autopilot, Entwurf-Dialog and Werkstatt.

User decisions (2026-10-29):
- Shape: a guided wizard.
- Features: A start screen, B gap questions, C one-section focus mode, D "nicht relevant" toggle, E reuse of the previous Bericht, F Gutachter-Check, G facts inside the Bericht.
- No voice interview (H).
- Gap answers are typed or dictated, with quick chips.
- Fällig list: name on top, Chiffre small underneath.

This file is the plan. Every slice ends with the after-slice routine: commit, push main, `npm run app:build`. Each slice's decisions go into PLAN-AMENDMENTS §22+ for the user's review.

## Why this is simpler, not more (guiding rule: replace, don't add)
- **One path instead of five.** Every entry point opens the same wizard at the right step: patient page, Heute/Fällig, the board banner, Autopilot, and the board itself.
  - The Entwurf-Dialog, the auto-draft banner, the draft card and the per-section "Entwurf…" buttons go away as separate concepts.
  - The Werkstatt becomes one "Frei diktieren" option inside step 2. It is no longer a separate page she has to know about.
- **One question at a time.** Each step has exactly one primary button ("Weiter").
  - She never has to work out what comes next.
  - The checklist card, the side cards and the bottom-bar hints merge into the step itself.
- **No silent gaps.** Instead of the AI quietly dropping sentences, the app asks her up front for exactly what's missing (B). The draft then reads complete the first time, which means fewer rounds.
- **Fewer hard blocks.** "Nicht relevant" (D) removes the "write something in Somatik so you can sign" chore.
- **The full editor stays, as a view, not a second workflow.** "Ganzes Dokument" (today's Abschnitte/Dokument modes plus the preview) is reachable from every step for free editing, and the wizard state follows it.
- The new pieces live inside the steps and don't add screens. G sits in the focus mode's right rail. F is step 4. E is an input plus a short "Seit dem letzten Bericht" line.

---

## Slice 36: UI quick fixes (items 1–4)

**1. Name plus Chiffre in Fällig**
- Add a new shared `ui/src/components/patient-label.tsx`. It shows the name (normal size, truncates) with the Chiffre small (`font-mono text-xs text-muted-foreground`) underneath, plus an optional small `PatientAvatar` (`ui/src/components/patient-avatar.tsx`). The name comes from `patient_name`, and the Chiffre line is left out when it's null.
- Use it in `DueLink` and `ApplicationDueRow` (`ui/src/routes/index.tsx:279-330`). Those rows currently show `item.chiffre ?? item.patient_name`.
- Also use it in the board `ReportCard` (see 4). `ReportSummary.chiffre` already exists.
- No backend change: `DueItem` already carries `patient_name` and `chiffre`.

**2. Photo preview right after choosing**
- Add a new Tauri command `patient_photo_preview { path }` in `crates/app/src/lib.rs`, next to `patient_photo_set` (:674).
  - It uses the same 25 MB guard and `crate::photo::normalize` (`crates/core/src/photo.rs:17`), and returns a `data:image/jpeg;base64` URL without storing anything.
  - So the preview is exactly the saved result: the EXIF rotation and the square crop are already applied.
- Add an `api.patientPhotoPreview` wrapper in `ui/src/lib/api.ts`.
- In `ui/src/components/patient-form.tsx`, `pickPhoto` (:83) fetches the preview and keeps it in state.
- `PatientAvatar` gets an optional `src` override and shows the preview, instead of the initials it shows today (:141).
- If the image can't be read, show the error inline right away, instead of only when she saves.

**3. Back button**
- In the header of `ui/src/components/app-shell.tsx` (:178), add a ghost icon button (ArrowLeft) before the search button.
  - It calls `router.history.back()` and is disabled when `useCanGoBack()` is false.
  - Tooltip: "Zurück (Alt+←)". A matching forward arrow is optional; I'd leave it out.
- Shortcuts: add Alt+← to the existing keydown handler (:85-94), and listen for the mouse back button (`mouseup` with `button === 3`).
- First check that the installed TanStack Router version exports `useCanGoBack`. If it doesn't, track it with a small history-index counter.

**4. Berichte board overflowing when the Assistent is open** (`ui/src/routes/reports/index.tsx`)
- The grid (:111) changes from `grid-cols-4` to columns with a minimum width that scroll sideways, the usual kanban behaviour: `grid grid-flow-col auto-cols-[minmax(14rem,1fr)] overflow-x-auto pb-2`. When the panel is open, the board scrolls instead of squeezing.
- Column `h2` (:116): add `gap-2`; the label gets `min-w-0 truncate`; the count stays `shrink-0`.
- `ReportCard` (:132-170):
  - the card gets `min-w-0 overflow-hidden`
  - the name line uses `PatientLabel`
  - the kind line gets `truncate`
  - the progress text and `Deadline` lose `whitespace-nowrap` (`flex-wrap` already handles them).
- Check by screenshot: window ~1130 px wide with the Assistent open, compared with the user's screenshot.

---

## Slice 37: Bericht-Lotse frame, start screen (A), "nicht relevant" (D)

**Wizard frame**
- `/reports/$reportId` becomes the wizard, with steps in a search param `?step=start|fragen|abschnitte|check|fertig`.
- New components go in `ui/src/components/lotse/`:
  - a top stepper with clickable steps
  - one primary button per step
  - a "Ganzes Dokument" link, which opens today's editor body as a view: the `SectionEditor`/`report-document`/`report-preview` code moves into a component.
- `ui/src/routes/reports/$reportId.tsx` (899 lines) gets split. The existing pieces are reused and not rewritten:
  - `SectionEditor`
  - `RichField`
  - `DiagnosesEditor`
  - `DraftProposal`
  - `ReportPreview`
  - `ChecklistCard` logic
  - `ExportCard`.
- Step state is derived from the data, like `REPORT_FLOW` today, plus `in_review`, which is now actually set (see slice 39).

**Step 1 "Start" (A)**
- **New Bericht entry:** a "Neuer Bericht" button on the board and on the patient page. It first asks for the patient, then opens the Start step.
- **Suggested Antragsart:** comes from the episode via a new `core::lotse::suggest_kind`:
  - no application yet → Erstantrag
  - stage KZT and a signed Erstantrag → Umwandlung
  - LZT → Fortführung Nr. n.
  - She can still pick another kind.
  - If a draft of the same kind already exists, the button says "Vorhandenen Entwurf öffnen" instead of creating a duplicate.
  - Change: the three always-visible buttons on the patient page (`ui/src/routes/patients/$patientId.tsx:171-175`) become this single entry.
- **Bereitschaft (readiness):** a list of sections, each with a traffic light computed from the number of tagged facts and the open questions (slice 38). Shown as "§4 Lebensgeschichte: 3 Fakten, 2 offene Fragen".
- **Inline fixes, so she doesn't navigate away:**
  - Diagnoses: if the episode differs from the Bericht, show "Diagnosen vom Behandlungsfall übernehmen?". This fixes the one-time copy.
  - A missing Chiffre.
  - A missing phase or therapy start, which the contingent needs.
- **Auto-draft:** `usePrepareApplication` and `AutoDraft` (`ui/src/components/autopilot.tsx:223-293`) now land on this step. Autopilot still drafts in the background; the wizard just shows the result in step 3.

**D "Nicht relevant"**
- New migration: `report_section.not_relevant INTEGER NOT NULL DEFAULT 0`, plus a store setter with an audit entry.
- In the template YAML, `optional: true` is set on sections that PTV 3 marks "bei Relevanz": `templates/ptv3/erst-umwandlung.yaml` somatik (check the PTV 3 wording for which others).
- `pii::checklist` (`crates/core/src/pii.rs:111`) skips `not_relevant` sections.
- The export (`render/bericht.typ`, `render/mod.rs`) leaves those sections out. This is my default; it goes in PLAN-AMENDMENTS for her review.
- The toggle only appears on optional sections.

---

## Slice 38: Lücken-Interview (B), step 2 "Fragen"

**Question catalogue (data, no code)**
- Template YAML sections get an `asks:` list: `key`, `question`, quick `chips` (e.g. "keine", "nicht bekannt", "nicht relevant"), and `when` (kind, method, age, like the hints).
- Examples:
  - §1: Beruf/Familienstand
  - §3: somatische Befunde, Medikation, Konsiliarbericht
  - §4: Vorbehandlungen, Bedingungsmodell
  - §6: Therapieziele, Setting/Frequenz, Stundenzahl, Prognose
  - §7 Umwandlung: Begründung
  - Fortführung: Verlauf, bisherige Zielerreichung, Begründung der Fortführung.
- I draft the questions from the PTV 3 hints already in the YAML; the user/therapist reviews them.

**Coverage check (`crates/core/src/gaps.rs`)**
- One pseudonymized LLM call per Bericht, sent through the existing provider layer and redaction preview.
  - Input: the asks plus the tagged facts per section.
  - Output: per ask, covered or not covered, with the fact ids. Only uncovered asks become questions.
- Deterministic shortcuts that skip the LLM:
  - asks answered from the patient record (age, gender)
  - asks already answered.
- If no AI is configured, every ask is shown, which still works.

**Answers**
- New table `report_answer`: report_id, section_key, ask_key, text, source typed|dictated|chip, timestamps.
- In the step 2 UI, one card per section with its questions. Each question has a text field, a mic button (reusing the existing local dictation from slice 24) and chips. "Später" lets her skip.
- "Frei diktieren" uses `core::workshop` (`crates/core/src/workshop.rs`) as the alternative: she talks about everything at once and the workshop splits it.
- Answers count as **her own statements**:
  - They go into drafting as cited sources ("Ihre Angabe").
  - They are exempt from the `GROUNDED` drop in `crates/core/src/draft.rs:417-444`; the ICD, name and length checks still apply.
  - They are pseudonymized like facts.
- Bump the prompt version in `draft.rs`.
- **The MDR boundary holds:** there is no ask for diagnoses, and a medication answer is reproduced as she wrote it, never suggested.

---

## Slice 39: Focus mode (C) + facts in the Bericht (G), step 3 "Abschnitte"

**Layout:** three columns, like the chosen preview.
- Left: a list of sections with status dots (empty, has a proposal, accepted, not relevant).
- Middle: the current section. Its proposal is shown sentence by sentence, each with fact or answer chips, using the reused `DraftProposal` (`ui/src/components/report-draft.tsx:253`).
  - **Keyboard:** Enter = übernehmen, Entf = verwerfen, E = bearbeiten, ↓/↑ = next/previous sentence, N = next section.
  - Accepted text goes straight into the `RichField` below, for free edits.
  - "Neu entwerfen" with an optional instruction.
- Right: "Woher kommt das?" (sources of the selected sentence), "Was fehlt noch?" (open asks for this section, answerable right there) and **"Fakten zu diesem Abschnitt" (G)**.

**Drafting**
- One action, "Alle Abschnitte entwerfen", with a single redaction preview. It reuses `draft_preview`/`draft_run` (`crates/app/src/lib.rs:2439-2647`), runs when she leaves step 2, and needs no separate dialog.
- The session choice and "ergänzen" move into a small "Optionen" popover.

**G: facts inside the Bericht**
- The right rail lists the section's tagged facts; "Weitere Fakten" lists the untagged ones.
- She can change a fact's tags even after it's applied (removing today's freeze):
  - a store setter `set_fact_sections` on `note_fact_section` with `source = therapist` and an audit entry
  - the readiness view and the drafting pick it up.
- A click on "+ zu diesem Abschnitt" works (no drag-and-drop; clicks are simpler and more reliable).

**Status:** when all sections are accepted or not relevant, the Bericht moves to `in_review` and step 4 opens. This fixes the unused status; update `ui/src/lib/queue.ts` `boardColumn` to use the real status.

---

## Slice 40: Previous Bericht (E) + Gutachter-Check (F), steps 4–5

**E: reuse the previous Bericht** (Umwandlung, Fortführung)
- The last signed Bericht of the same episode becomes extra pseudonymized context in `draft::section_message`, with citations labelled "Vorbericht (Datum)".
- Step 1 shows "Seit dem letzten Bericht: n Sitzungen, m neue Fakten".
- Fortführung drafts are told to describe the change since the Vorbericht.

**F: step 4 "Check"**
- One list with two kinds of entries, ranked:
  - deterministic checks: empty sections, identifiers, missing Chiffre, length, diagnoses differing from the episode
  - an AI "Gutachter-Check" (new `crates/core/src/review.rs`): one pseudonymized call with the full text plus the PTV 3 hints. It returns hints per section ("Ein Gutachter würde fragen: …"), and **never changes the text**.
- Each hint has "Zum Abschnitt", which goes back to step 3 at that section, and "Ignorieren".

**Step 5 "Fertig"**
- The preview (`ReportPreview`), the signer's name filled in from settings, the sign button, and PDF/DOCX.
- Exporting before signing shows a clear "unsigniert" hint.

**Also**
- Rewrite `ui/src/content/hilfe/bericht.md` for the wizard.
- Update the Rundgang (tour) steps that point at the old Bericht page.
- Update the video script (`docs/video/skript.md`) shots, if any of them show the Bericht page.

---

## Open points I'll decide (and log in PLAN-AMENDMENTS) unless the user objects
- "Nicht relevant" sections are left out of the export, not printed as "keine Besonderheiten".
- No drag-and-drop for facts, only click to assign.
- The AI coverage check and the Gutachter-Check use the "draft" task model and reasoning medium. They run on local models too, and show up in the cost view (slice 33) as their own tasks.
- Optional mockups: I add GPT-image prompts for the wizard steps to the appendix of PLAN-PHASE5. Slices 37 and 39 can wait for them if the user wants, as in Phase 4.

## Verification
- `cargo test -p thera-core` covers:
  - `suggest_kind`
  - `not_relevant` in the checklist and export
  - gaps parsing and coverage with a mock
  - answers exempt from grounding but not from the ICD and name checks
  - `set_fact_sections` after apply
  - Vorbericht context
  - review parsing.
  - It also regenerates `ui/src/bindings`.
- `npm run typecheck` / lint in `ui/`.
- E2E over CDP (thera-dev-env memory), with a seeded `THERA_DATA_DIR` and an LLM mock that picks its answer by system prompt:
  - **Slice 36:** screenshots of Heute Fällig, the photo dialog after picking an image, back after patient → Bericht, and the board at 1130 px with the Assistent open.
  - **Slices 37–40:** run the full wizard for an Erstantrag:
    - start
    - answer 3 questions (typed and chip)
    - draft
    - accept with the keyboard
    - mark Somatik not relevant
    - re-tag one fact
    - check
    - sign
    - export the PDF; check with `pdftotext` that Somatik is left out and the answers are present.
  - Then a Fortführung with a Vorbericht.
- After each slice: commit, push main, `npm run app:build`.

---

## Step 0: Mockups (user, before slice 37)
The user generates mockups 19–24 from the appendix prompts into `docs/mockups/19-…` and following. Read them like the Phase 4 ones: take the content area, keep the existing shell (sidebar, top bar with back arrow, Strg+K, Assistent) and the slice 20 tokens, ignore invented sidebar entries and model names.

**Done (2026-10-29):** `19-lotse-start`, `20-lotse-fragen`, `21-lotse-abschnitte`, `22-lotse-pruefen`, `23-lotse-fertig`, `24-neuer-bericht`. How to read them:
- The stepper labels differ between images; the real ones are those of #19: Start · Fragen · Abschnitte · Prüfen · Fertig, with the title "‹Antragsart› · ‹Name›", Chiffre and method badge above (#19).
- #19 is close to the target as is: two cards (Antragsart tiles with the "Empfohlen" reason, Bereitschaft list with dots + counts and the diagnoses notice), a bottom bar with "Ganzes Dokument" left and one primary button right.
- #20: progress bar "5 von 8 beantwortet", "Lieber frei diktieren" top right, cards grouped by section, mic inside the field, chips below, "Ihre Angabe" check once answered, "Später" + primary button at the bottom.
- #21: the section names (Anamnese, Ressourcen …) and the fact tags (Familie, Kindheit …) are invented; use the template's sections and the real fact text. Keep the layout: rail with status dots and a legend, middle column in serif with the current sentence in a teal box, the keyboard bar at the bottom, the three cards on the right.
- #22: take the hint cards (section number badge, text, "Zum Abschnitt" / "Ignorieren"), the green "Pflichtangaben vollständig" row, the preview on the right and "Zurück" bottom left. Its header and 4-step stepper are off-prompt.
- #23: only the layout (A4 preview with page arrows below, the sign card on the right). The document text is invented and wrong for a Bericht (it shows an expert opinion with ICD codes in the text).
- #24: only the dialog and the "Neuer Bericht" button at the top right; the page behind it shows tabs and a table instead of the board.
- Ignore the invented sidebar footers (Datenschutz, user profile, "DSGVO-konform").

## Appendix: GPT-image prompts (Bericht-Lotse)

Prepend the `[style]` block below to each prompt. Make 2–3 variants each. Fictional data only.

[style] High-fidelity desktop app UI screenshot, 1440×900, Windows 11 app window, for "thera", a calm, private documentation app for German psychotherapists. Visual style: quiet, warm, trustworthy, clinical but not cold; soft warm-neutral background (#F7F5F2), white cards with subtle 1px borders and very soft shadows, muted teal accent (#2F7A78), a secondary warm sand accent, rounded corners 10px, generous whitespace, Inter-like sans-serif for UI and an elegant serif for document text, small lucide-style line icons. All UI text in German. No stock photos, no illustrations of people, no clutter, no gradients. Realistic and implementable with shadcn/ui + Tailwind. Left sidebar (Heute, Kalender, Patient:innen, Sitzungen, Berichte, Assistent) and a top bar with a back arrow, search "Suchen oder Befehl… Strg K" and a button "Assistent".

19. **Lotse – Schritt 1 „Start"**: [style] Page "Umwandlungsantrag · Anna K." with Chiffre "K120385" in monospace and badge "VT". Below the title a horizontal 5-step stepper: "1 Start" (active, teal), "2 Fragen", "3 Abschnitte", "4 Prüfen", "5 Fertig". Left card "Antragsart": three selectable tiles "Erstantrag", "Umwandlungsantrag" (selected, with tag "Empfohlen · KZT 1 fast aufgebraucht, Sitzung 10 von 12") and "Fortführungsantrag"; small line "Seit dem letzten Bericht: 8 Sitzungen, 23 neue Fakten". Right card "Bereitschaft": list of sections with traffic-light dots: "1 Soziodemographie ● 4 Fakten", "2 Symptomatik & Befund ● 9 Fakten", "3 Somatik ◐ 1 offene Frage", "4 Lebensgeschichte & Modell ◐ 3 Fakten · 2 offene Fragen", "5 Diagnosen ● vom Behandlungsfall", "6 Behandlungsplan & Prognose ○ 4 offene Fragen", "7 Umwandlung ○ 1 offene Frage". A small inline amber notice "Diagnosen am Behandlungsfall wurden geändert – übernehmen?" with button "Übernehmen". Bottom right one primary teal button "Weiter: 8 Fragen beantworten →", left a quiet link "Ganzes Dokument".

20. **Lotse – Schritt 2 „Fragen"**: [style] Same header and stepper, "2 Fragen" active, subtitle "Was die Sitzungen nicht hergeben – kurz beantworten, Stichworte genügen." Vertical list of question cards grouped by section headings ("3 Somatik", "6 Behandlungsplan & Prognose"). Each card: the question in medium weight, e.g. "Nimmt die Patientin aktuell Medikamente?", a text field with placeholder "Stichworte oder Satz…", a small microphone button inside the field, and quick chips below "keine", "nicht bekannt", "nicht relevant". One card answered (text "Sertralin 50 mg seit 03/2026, verordnet von Hausärztin" with a small teal check "Ihre Angabe"), one card answered by chip "keine" (chip filled teal), one card "Welche Therapieziele haben Sie vereinbart?" in focus with the mic recording (small red dot and waveform). Top right a secondary button "Lieber frei diktieren". Progress text "5 von 8 beantwortet". Bottom: link "Später", primary button "Weiter: Entwurf schreiben lassen →".

21. **Lotse – Schritt 3 „Abschnitte" (Fokus)**: [style] Same header and stepper, "3 Abschnitte" active. Three columns. Left narrow rail: list of the 7 sections with status dots (filled teal = übernommen, half = Vorschlag offen, empty, grey dash = "nicht relevant" on "3 Somatik"); "4 Lebensgeschichte & Modell" selected. Middle wide column in serif document text: heading "4 Lebensgeschichte & Modell", a proposal shown sentence by sentence; the current sentence highlighted in a soft teal box with small chips "Sitzung 3" "Sitzung 5" and inline buttons "Übernehmen ↵" and "Verwerfen Entf"; already accepted sentences in normal black text above, remaining ones in muted grey below; small keyboard hint bar at the bottom "↵ übernehmen · Entf verwerfen · E bearbeiten · ↓ nächster Satz · N nächster Abschnitt". Right rail with three small cards: "Woher kommt das?" showing two short fact quotes with session dates; "Was fehlt noch?" with one open question and a mini text field; "Fakten zu diesem Abschnitt" listing 4 facts with small tag chips and a "+ Weitere Fakten" link. Top right of the middle column: ghost buttons "Neu entwerfen" and "Optionen".

22. **Lotse – Schritt 4 „Prüfen" (Gutachter-Check)**: [style] Same header and stepper, "4 Prüfen" active. Card "Gutachter-Check" with subtitle "Hinweise, keine Änderungen am Text". A ranked list of hints, each with a section badge and two buttons "Zum Abschnitt" and "Ignorieren": amber "6 Behandlungsplan: Die Prognose wird genannt, aber nicht begründet. Ein Gutachter würde fragen: Woran machen Sie die günstige Prognose fest?", amber "7 Umwandlung: Warum reicht die Kurzzeittherapie nicht aus?", grey "Länge: ca. 2,3 Seiten – im Rahmen". A green row at the top "Pflichtangaben vollständig · Chiffre gesetzt · keine Klarnamen im Text". Right side a small page preview thumbnail of the Bericht. Primary button bottom right "Weiter: Unterschreiben →".

23. **Lotse – Schritt 5 „Fertig"**: [style] Same header and stepper, "5 Fertig" active. Left two thirds: an A4 page preview of the "Bericht an den Gutachter" in elegant serif with numbered headings, Chiffre top right, slight paper shadow, page indicator "Seite 1 von 3". Right card "Unterschreiben": field "Name" prefilled "Dipl.-Psych. M. Berger", checkbox "Ich habe den Bericht geprüft", primary teal button "Unterschreiben", then a divider and two buttons "Als PDF exportieren" and "Als DOCX exportieren", a small muted note "Nach dem Unterschreiben ist der Text schreibgeschützt; Nachträge bleiben möglich."

24. **Neuer Bericht (Auswahl)**: [style] The "Berichte" board page dimmed with a centered dialog "Neuer Bericht". Search field "Patient:in suchen…", list of 4 patients with round initials avatars, name, Chiffre in monospace below the name, and a muted line on the right ("VT · KZT 1 · Sitzung 10 von 12 → Umwandlungsantrag empfohlen", "TP · noch kein Antrag → Erstantrag", "VT · Entwurf vorhanden → Entwurf öffnen"). Primary button "Weiter" bottom right, "Abbrechen" left of it.
