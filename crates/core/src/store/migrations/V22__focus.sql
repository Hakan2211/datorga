-- V22: Bericht-Lotse focus mode (Phase 5, slice 39).

-- The therapist's decision on one sentence of a proposed draft: 'accepted' (the sentence went
-- into the section, `edited` = in her own wording) or 'rejected'. NULL while undecided. Once
-- every sentence is decided, the draft is applied (or rejected when none was accepted).
ALTER TABLE draft_sentence ADD COLUMN decision TEXT CHECK (decision IN ('accepted', 'rejected'));
ALTER TABLE draft_sentence ADD COLUMN edited INTEGER NOT NULL DEFAULT 0;
