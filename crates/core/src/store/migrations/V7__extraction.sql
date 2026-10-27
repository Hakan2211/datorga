-- V7: per-session fact extraction (Phase 2, slice 13).

-- Terms the therapist adds in the redaction preview (relatives, places, employers…). They feed
-- the pseudonymizer for every later request about this patient. Local only, like the patient record.
CREATE TABLE pseudo_term (
    id          TEXT PRIMARY KEY,
    patient_id  TEXT NOT NULL REFERENCES patient(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,                   -- person | ort | arbeitgeber | datum
    text        TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    UNIQUE (patient_id, text)
);

-- One model run over a session's confirmed transcript, for a Sitzungsnotiz.
CREATE TABLE extraction (
    id             TEXT PRIMARY KEY,
    report_id      TEXT NOT NULL REFERENCES report(id) ON DELETE CASCADE,
    provider       TEXT NOT NULL,
    model          TEXT NOT NULL,
    prompt_version TEXT NOT NULL,
    prompt_hash    TEXT NOT NULL,
    chunks         INTEGER NOT NULL,
    dropped        INTEGER NOT NULL DEFAULT 0,  -- answer entries that failed the checks
    created_at     TEXT NOT NULL
);

CREATE TABLE note_fact (
    id            TEXT PRIMARY KEY,
    extraction_id TEXT NOT NULL REFERENCES extraction(id) ON DELETE CASCADE,
    report_id     TEXT NOT NULL REFERENCES report(id) ON DELETE CASCADE,
    seq           INTEGER NOT NULL,
    section_key   TEXT NOT NULL,
    text          TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'proposed', -- proposed | rejected | applied
    edited        INTEGER NOT NULL DEFAULT 0,
    applied_at    TEXT
);

-- Fact → transcript segment. A fact stays traceable to the words it came from.
CREATE TABLE fact_citation (
    fact_id    TEXT NOT NULL REFERENCES note_fact(id) ON DELETE CASCADE,
    segment_id TEXT NOT NULL REFERENCES transcript_segment(id) ON DELETE CASCADE,
    PRIMARY KEY (fact_id, segment_id)
);

CREATE INDEX idx_note_fact_report ON note_fact(report_id, seq);
CREATE INDEX idx_pseudo_term_patient ON pseudo_term(patient_id);
