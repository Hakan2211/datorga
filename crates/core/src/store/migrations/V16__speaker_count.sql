-- V16: how many people speak in a session recording (2 = therapist and patient; 3–4 with a
-- parent or partner). Chosen per recording; a "diarize" job separates the draft's speakers again.
ALTER TABLE session_source ADD COLUMN speakers INTEGER NOT NULL DEFAULT 2;
