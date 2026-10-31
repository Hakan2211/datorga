-- V24: Papierkorb (slice 43). A deleted session with a signed note, or a deleted signed Bericht,
-- is kept here for 30 days as a snapshot of every row the delete removed, then purged.
CREATE TABLE trash_item (
    id          TEXT PRIMARY KEY,
    patient_id  TEXT NOT NULL REFERENCES patient(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,                 -- session | bericht
    entity_id   TEXT NOT NULL,
    detail      TEXT,                          -- bericht: the Antragsart (erstantrag, umwandlung, fortfuehrung)
    occurred_at TEXT,                          -- session: its time; bericht: when it was signed
    deleted_at  TEXT NOT NULL,
    payload     TEXT NOT NULL                  -- JSON: the removed rows per table
);
CREATE INDEX idx_trash_patient ON trash_item(patient_id);
