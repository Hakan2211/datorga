-- V3: transcription. One transcript per source; a job row lets a long run survive an app restart.

CREATE TABLE transcript (
    id           TEXT PRIMARY KEY,
    source_id    TEXT NOT NULL UNIQUE REFERENCES session_source(id) ON DELETE CASCADE,
    engine       TEXT NOT NULL,                  -- e.g. whisper.cpp b5454
    model        TEXT NOT NULL,                  -- catalog id, e.g. large-v3-turbo-q5_0
    language     TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'draft',  -- draft | confirmed
    created_at   TEXT NOT NULL,
    confirmed_at TEXT
);

CREATE TABLE transcript_segment (
    id            TEXT PRIMARY KEY,
    transcript_id TEXT NOT NULL REFERENCES transcript(id) ON DELETE CASCADE,
    idx           INTEGER NOT NULL,
    start_ms      INTEGER NOT NULL,
    end_ms        INTEGER NOT NULL,
    speaker_label TEXT,                          -- NULL until diarization
    text          TEXT NOT NULL,
    edited        INTEGER NOT NULL DEFAULT 0,
    UNIQUE (transcript_id, idx)
);

CREATE TABLE job (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,                    -- transcribe
    source_id  TEXT REFERENCES session_source(id) ON DELETE CASCADE,
    status     TEXT NOT NULL DEFAULT 'queued',   -- queued | running | done | failed | cancelled
    progress   INTEGER NOT NULL DEFAULT 0,       -- percent
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,                             -- technical message only, never transcript text
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_job_status ON job(status, created_at);
CREATE INDEX idx_job_source ON job(source_id);
