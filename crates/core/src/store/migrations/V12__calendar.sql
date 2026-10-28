-- V12: calendar and day planning (Phase 3, slice 22).

-- Planned length (NULL = the default 50 min) and why a session was cancelled (status 'cancelled').
ALTER TABLE session ADD COLUMN planned_minutes INTEGER;
ALTER TABLE session ADD COLUMN cancelled_reason TEXT;

-- One time format for range queries: RFC 3339 UTC without fractions (the UI wrote '.000Z').
UPDATE session SET scheduled_at = strftime('%Y-%m-%dT%H:%M:%SZ', scheduled_at)
    WHERE scheduled_at IS NOT NULL AND strftime('%Y-%m-%dT%H:%M:%SZ', scheduled_at) IS NOT NULL;
CREATE INDEX idx_session_when ON session(COALESCE(scheduled_at, started_at));

-- To-dos for a day (local date 'YYYY-MM-DD', optional 'HH:MM'), optionally tied to a patient or session.
CREATE TABLE task (
    id          TEXT PRIMARY KEY,
    date        TEXT NOT NULL,
    time        TEXT,
    title       TEXT NOT NULL,
    done        INTEGER NOT NULL DEFAULT 0,
    patient_id  TEXT REFERENCES patient(id) ON DELETE CASCADE,
    session_id  TEXT REFERENCES session(id) ON DELETE CASCADE,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_task_date ON task(date);

-- Free planning text per local date.
CREATE TABLE day_note (
    date        TEXT PRIMARY KEY,
    body        TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
