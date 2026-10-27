-- V2: capture. A session can have several sources (a recording, a dictated summary, ...).

CREATE TABLE session_source (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,                   -- session_audio | dictation
    consent_id  TEXT REFERENCES consent(id),     -- required for session_audio
    status      TEXT NOT NULL DEFAULT 'recording', -- recording | recorded | interrupted
    language    TEXT NOT NULL DEFAULT 'de',
    started_at  TEXT NOT NULL,
    ended_at    TEXT,                            -- the cut point
    end_reason  TEXT                             -- stopped | consent_revoked | device_error | crash
);

CREATE TABLE audio_asset (
    id                     TEXT PRIMARY KEY,
    source_id              TEXT NOT NULL UNIQUE REFERENCES session_source(id) ON DELETE CASCADE,
    path                   TEXT NOT NULL,        -- directory of encrypted chunks
    codec                  TEXT NOT NULL,
    dek                    TEXT NOT NULL,        -- per-recording key (hex); the DB itself is encrypted
    sha256                 TEXT,                 -- of the plaintext PCM; NULL after a crash
    duration_ms            INTEGER,
    retention_delete_after TEXT,
    deleted_at             TEXT
);

CREATE INDEX idx_source_session ON session_source(session_id);
