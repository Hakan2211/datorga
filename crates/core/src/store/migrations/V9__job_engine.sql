-- Engine chosen per transcription (local | cloud); NULL = from Settings when the job runs.
ALTER TABLE job ADD COLUMN engine TEXT;
