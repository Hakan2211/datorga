-- V28: her own notes on how she writes (slice 52, PLAN-AMENDMENTS §3 "style notes"), per document
-- type or '*' for all types. With the profile computed from her exemplars they go into drafts as
-- "Stilprofil". The computed part is not stored: it follows the active exemplars.
CREATE TABLE style_note (
    template_key TEXT PRIMARY KEY,
    notes        TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
