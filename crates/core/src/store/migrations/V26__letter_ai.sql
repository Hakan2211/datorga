-- V26: quick AI writing in Briefe (Phase 6, slice 47).

-- A letter draft keeps a sentence the material doesn't back, marked "ohne Beleg": she keeps or
-- deletes it before signing. Bericht drafts still drop such sentences.
ALTER TABLE draft_sentence ADD COLUMN unsupported INTEGER NOT NULL DEFAULT 0;
