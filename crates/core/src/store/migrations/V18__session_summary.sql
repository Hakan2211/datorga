-- V18: AI key points per session for the patient timeline (Phase 4, slice 30).
-- points_json: [{text, segments: [transcript_segment.id], note: [report_section.key]}], 3–6 points.
-- status: draft (as the model wrote it) | edited (the therapist changed it). A new run replaces it.
CREATE TABLE session_summary (
    session_id     TEXT PRIMARY KEY REFERENCES session(id) ON DELETE CASCADE,
    points_json    TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'draft',
    provider       TEXT NOT NULL,
    model          TEXT NOT NULL,
    prompt_version TEXT NOT NULL,
    prompt_hash    TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);
