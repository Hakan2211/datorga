-- V19: autopilot after the session + contingent count (Phase 4, slice 31).

-- A job about a whole session (kind `autopilot`) instead of one recording.
ALTER TABLE job ADD COLUMN session_id TEXT REFERENCES session(id) ON DELETE CASCADE;
CREATE INDEX idx_job_session ON job(session_id);

-- The steps of an autopilot job, in order. `sent_json` keeps what went to the model for
-- "Was wurde gesendet?" (pseudonymized requests + the local token table, extract::Preview).
CREATE TABLE autopilot_step (
    job_id     TEXT NOT NULL REFERENCES job(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    step       TEXT NOT NULL,                    -- note | extract | summary
    status     TEXT NOT NULL DEFAULT 'pending',  -- pending | running | done | skipped | failed
    detail     TEXT,                             -- why skipped/failed; technical, never patient text
    sent_json  TEXT,
    provider   TEXT,
    model      TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (job_id, step)
);

-- Start of the Richtlinientherapie (first session after the Probatorik), set with the stage.
-- Sessions from then on count against the contingent.
ALTER TABLE episode ADD COLUMN therapy_since TEXT;

-- Length of the note text a summary was written from, to re-run it after signing when the
-- note changed a lot.
ALTER TABLE session_summary ADD COLUMN note_chars INTEGER NOT NULL DEFAULT 0;
