-- V27: her own old letters and Berichte as style exemplars (Phase 6, slice 49; PLAN-AMENDMENTS §3).
-- Only the redacted text is kept (she reviewed every mark), with the uploaded file's SHA-256; the
-- file itself is not stored. v1: whole text per document type, no section mapping.
CREATE TABLE style_exemplar (
    id            TEXT PRIMARY KEY,
    template_key  TEXT NOT NULL,
    section_key   TEXT,
    title         TEXT NOT NULL,
    redacted_text TEXT NOT NULL,
    active        INTEGER NOT NULL DEFAULT 1,
    source_sha256 TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
CREATE INDEX style_exemplar_template ON style_exemplar (template_key, active, created_at);
