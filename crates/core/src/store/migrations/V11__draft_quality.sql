-- V11: drafting quality (Phase 3, slice 19).

-- Bericht sections a Sitzungsnotiz fact belongs to (keys of the Erst-/Umwandlungsbericht). The
-- model proposes them in extraction (`ai`); the therapist adds or removes them in the review
-- (`therapist`). A section draft then sees only the facts tagged for it.
CREATE TABLE note_fact_section (
    fact_id     TEXT NOT NULL REFERENCES note_fact(id) ON DELETE CASCADE,
    section_key TEXT NOT NULL,
    source      TEXT NOT NULL CHECK (source IN ('ai', 'therapist')),
    PRIMARY KEY (fact_id, section_key)
);

-- 1 once the fact went through tagging (even with no tag). Facts from before stay 0 and are
-- offered to every section.
ALTER TABLE note_fact ADD COLUMN bericht_tagged INTEGER NOT NULL DEFAULT 0;

-- Draft provenance: the sessions the therapist chose (JSON array of session ids, NULL = all) and
-- whether the section's existing text went along ("ergänzen").
ALTER TABLE section_draft ADD COLUMN session_ids TEXT;
ALTER TABLE section_draft ADD COLUMN with_existing INTEGER NOT NULL DEFAULT 0;
