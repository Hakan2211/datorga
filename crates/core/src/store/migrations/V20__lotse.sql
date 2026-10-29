-- V20: Bericht-Lotse (Phase 5, slice 37).

-- An optional section (template `optional: true`) the therapist marked "nicht relevant": it
-- doesn't block signing and is left out of the export.
ALTER TABLE report_section ADD COLUMN not_relevant INTEGER NOT NULL DEFAULT 0;
