-- V6: when a report was last exported and how many Nachträge that file contained, so a note
-- with a newer Nachtrag can say "neu exportieren". NULL count (exported before V6) counts as 0.

ALTER TABLE report ADD COLUMN exported_at TEXT;
ALTER TABLE report ADD COLUMN exported_addenda INTEGER;
