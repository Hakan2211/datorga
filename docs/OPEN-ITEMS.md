# thera: open items after Phase 2 (2026-10-27)

All planned slices are done (Phase 1: 1–10, Phase 2: 11–15). These are the loose ends collected from
`docs/HANDOFF.md`, as input for the next plan.

## Measurement (from slice 15)

- Re-run Qwen3 14B on the new draft prompt: `cargo run -p eval -- --provider local --model qwen3-14b --runs 3`,
  one model at a time (the full 8B+14B matrix ran out of memory).
- Measure one cloud model. Needs an OpenRouter or Requesty key in `THERA_EVAL_API_KEY`.
- Test OpenRouter and Requesty once with a real key before real use. So far they've only been tested against a mock.

## Drafting quality (the obvious next feature)

- Tag each fact with the Bericht sections it belongs to, let the therapist correct the tags, and draft each section
  only from its tagged facts. Drafts currently put too much everywhere (~50% of cited facts belong in the section),
  and the 8B still invents prognoses and goals.
- When drafting with "Anhängen", send the section's existing text as context, so the draft doesn't repeat it.
- Let the therapist pick which sessions feed a Fortführungsbericht.

## Waiting on the therapist

- The Vorlagen module needs her old Berichte and Sitzungen, which she'll upload herself in the app. Those also become
  a real eval suite and the first style examples.
- Her laptop specs decide the GPU vs. CPU builds for transcription and local models.
- Her Chiffre format.

## Before real patients

- Legal review of the consent text.
- A restore test on her own machine.

## Smaller technical items

- Speaker separation for session recordings. Today extraction can't tell the patient from the therapist and gets
  their gender wrong.
- Audio playback in the transcript viewer.
- A cancel button for long extractions.
- Merging duplicate facts across chunks.
- Putting whisper-cli into the job object, so a crashed app can't leave it running.

## Open decisions

- Phase 3: drafting quality or the Vorlagen module (depends on when her material arrives).
- When to do the real-key cloud test.
