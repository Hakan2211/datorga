-- V13: voice mode + Diktat-Werkstatt (Phase 3, slice 24).

-- Where a section draft came from: the applied Sitzungsnotiz facts (`facts`, cites them) or the
-- therapist's Werkstatt dictation (`dictation`, cites nothing).
ALTER TABLE section_draft ADD COLUMN source TEXT NOT NULL DEFAULT 'facts';

-- The Werkstatt text of a Sitzungsnotiz or Bericht (TipTap HTML). Only text: the audio of a live
-- dictation is never stored.
CREATE TABLE workshop_text (
    report_id  TEXT PRIMARY KEY REFERENCES report(id) ON DELETE CASCADE,
    body       TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
