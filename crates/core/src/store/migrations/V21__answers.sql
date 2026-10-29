-- V21: Bericht-Lotse "Fragen" (Phase 5, slice 38).

-- The therapist's answer to one question of the template (`asks:`), per Bericht. Drafts use it as
-- her own statement. The question is kept as asked, in case the template changes.
CREATE TABLE report_answer (
    id          TEXT PRIMARY KEY,
    report_id   TEXT NOT NULL REFERENCES report(id) ON DELETE CASCADE,
    section_key TEXT NOT NULL,
    ask_key     TEXT NOT NULL,
    question    TEXT NOT NULL,
    text        TEXT NOT NULL,
    source      TEXT NOT NULL,                 -- typed | dictated | chip
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    UNIQUE (report_id, section_key, ask_key)
);

-- Draft sentence → answer it rests on (next to report_citation for facts).
CREATE TABLE answer_citation (
    sentence_id TEXT NOT NULL REFERENCES draft_sentence(id) ON DELETE CASCADE,
    answer_id   TEXT NOT NULL REFERENCES report_answer(id) ON DELETE CASCADE,
    PRIMARY KEY (sentence_id, answer_id)
);

-- The last AI check of which questions the session facts already answer.
CREATE TABLE report_gap_check (
    report_id    TEXT PRIMARY KEY REFERENCES report(id) ON DELETE CASCADE,
    provider     TEXT NOT NULL,
    model        TEXT NOT NULL,
    prompt_hash  TEXT NOT NULL,
    facts_digest TEXT NOT NULL,                -- the facts it saw, to tell when new ones arrived
    covered      TEXT NOT NULL,                -- JSON [{section, ask, facts: [fact_id]}]
    checked_at   TEXT NOT NULL
);
