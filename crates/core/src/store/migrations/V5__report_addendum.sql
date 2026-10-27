-- V5: Nachtrag to a signed Sitzungsnotiz (§ 630f BGB). Written and signed in one step; the original
-- note and earlier addenda stay unchanged. Rows are never updated; they only go with the report.

CREATE TABLE report_addendum (
    id         TEXT PRIMARY KEY,
    report_id  TEXT NOT NULL REFERENCES report(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,                 -- 1, 2, … per report
    body       TEXT NOT NULL,                    -- TipTap HTML, like a prose section
    created_at TEXT NOT NULL,                    -- also the signing time
    signed_by  TEXT NOT NULL,
    UNIQUE (report_id, seq)
);

CREATE TRIGGER report_addendum_immutable BEFORE UPDATE ON report_addendum
BEGIN
    SELECT RAISE(ABORT, 'report_addendum is append-only');
END;
