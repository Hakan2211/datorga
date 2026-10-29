-- V23: Bericht-Lotse "Prüfen" and the Vorbericht (Phase 5, slice 40).

-- Draft sentence → section of the Vorbericht it rests on (next to report_citation and
-- answer_citation). The Vorbericht is signed, so its text no longer changes.
CREATE TABLE prior_citation (
    sentence_id TEXT NOT NULL REFERENCES draft_sentence(id) ON DELETE CASCADE,
    report_id   TEXT NOT NULL REFERENCES report(id) ON DELETE CASCADE,
    section_key TEXT NOT NULL,
    PRIMARY KEY (sentence_id, report_id, section_key)
);

-- The last AI Gutachter-Check of a Bericht. It never changes the text.
CREATE TABLE report_review (
    report_id   TEXT PRIMARY KEY REFERENCES report(id) ON DELETE CASCADE,
    provider    TEXT NOT NULL,
    model       TEXT NOT NULL,
    prompt_hash TEXT NOT NULL,
    text_digest TEXT NOT NULL,                 -- the text it read, to tell when she changed it since
    checked_at  TEXT NOT NULL
);

CREATE TABLE report_review_hint (
    id          TEXT PRIMARY KEY,
    report_id   TEXT NOT NULL REFERENCES report_review(report_id) ON DELETE CASCADE,
    seq         INTEGER NOT NULL,
    section_key TEXT,                          -- NULL: the whole Bericht
    level       TEXT NOT NULL CHECK (level IN ('important', 'note')),
    text        TEXT NOT NULL,
    ignored     INTEGER NOT NULL DEFAULT 0     -- "Ignorieren"
);

CREATE INDEX idx_review_hint_report ON report_review_hint(report_id, seq);
