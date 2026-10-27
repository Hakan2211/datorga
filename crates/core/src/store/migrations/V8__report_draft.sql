-- V8: Bericht drafting per section from applied Sitzungsnotiz facts (Phase 2, slice 14).

-- One model answer for one section of a Bericht. Provenance per draft; the report row gets the
-- provenance of the draft applied last.
CREATE TABLE section_draft (
    id             TEXT PRIMARY KEY,
    report_id      TEXT NOT NULL REFERENCES report(id) ON DELETE CASCADE,
    section_key    TEXT NOT NULL,
    provider       TEXT NOT NULL,
    model          TEXT NOT NULL,
    prompt_version TEXT NOT NULL,
    prompt_hash    TEXT NOT NULL,
    instructed     INTEGER NOT NULL DEFAULT 0,  -- the therapist added an instruction (text not stored)
    dropped        INTEGER NOT NULL DEFAULT 0,  -- answer entries that failed the checks
    status         TEXT NOT NULL DEFAULT 'proposed', -- proposed | applied | rejected | superseded
    mode           TEXT,                         -- replace | append, once applied
    created_at     TEXT NOT NULL,
    applied_at     TEXT
);

CREATE TABLE draft_sentence (
    id       TEXT PRIMARY KEY,
    draft_id TEXT NOT NULL REFERENCES section_draft(id) ON DELETE CASCADE,
    seq      INTEGER NOT NULL,
    text     TEXT NOT NULL
);

-- Sentence → Sitzungsnotiz fact (→ transcript segment via fact_citation, PLAN §5).
CREATE TABLE report_citation (
    sentence_id TEXT NOT NULL REFERENCES draft_sentence(id) ON DELETE CASCADE,
    fact_id     TEXT NOT NULL REFERENCES note_fact(id) ON DELETE CASCADE,
    PRIMARY KEY (sentence_id, fact_id)
);

CREATE INDEX idx_section_draft_report ON section_draft(report_id, section_key);
CREATE INDEX idx_draft_sentence_draft ON draft_sentence(draft_id, seq);
