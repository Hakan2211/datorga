-- V17: patient profile for the card view (Phase 4, slice 29).
-- gender: w | m | d, NULL = keine Angabe.
ALTER TABLE patient ADD COLUMN gender TEXT;
-- Working diagnoses per Behandlungsfall, entered by the therapist only (never AI-filled; MDR
-- boundary). JSON list of {code, label, certainty}; a new Bericht starts its Diagnosen section from it.
ALTER TABLE episode ADD COLUMN diagnoses_json TEXT NOT NULL DEFAULT '[]';
-- Optional portrait: a small JPEG (normalized by core::photo) inside the encrypted database, so it
-- is in every backup and goes with the patient on erasure. Never sent to any model.
CREATE TABLE patient_photo (
    patient_id TEXT PRIMARY KEY REFERENCES patient(id) ON DELETE CASCADE,
    jpeg       BLOB NOT NULL,
    updated_at TEXT NOT NULL
);
