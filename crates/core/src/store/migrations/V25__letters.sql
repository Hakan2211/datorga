-- V25: Briefe (Phase 6, slice 45). A letter is a report with kind = 'brief' plus one `letter`
-- row for what a letter has around its sections: recipient, Betreff, references, sending.
-- `report.kind = 'bericht'` stays PTV 3 only.

CREATE TABLE letter (
    report_id              TEXT PRIMARY KEY REFERENCES report(id) ON DELETE CASCADE,
    patient_id             TEXT NOT NULL REFERENCES patient(id) ON DELETE CASCADE,
    -- The Behandlungsfall it is filed under on the timeline (the newest open one by default).
    episode_id             TEXT REFERENCES episode(id) ON DELETE SET NULL,
    -- The address book entry it was written to. No foreign key: the snapshot below is what was
    -- written, and a deleted book entry must not take the letter's address with it.
    recipient_id           TEXT,
    -- {kind, name, department, lines[]} as printed in the address window.
    recipient_json         TEXT NOT NULL DEFAULT '{}',
    salutation             TEXT NOT NULL DEFAULT '',
    subject                TEXT NOT NULL DEFAULT '',
    -- [{label, value}] under the Betreff (Versicherungsnummer, Aktenzeichen, ...).
    refs_json              TEXT NOT NULL DEFAULT '[]',
    place                  TEXT,
    -- YYYY-MM-DD printed after "Ort, den"; NULL = the day of signing (today while a draft).
    letter_date            TEXT,
    -- The pasted request and its questions (slice 47).
    request_text           TEXT,
    request_questions_json TEXT NOT NULL DEFAULT '[]',
    material_json          TEXT NOT NULL DEFAULT '{}',
    sent_at                TEXT,                          -- YYYY-MM-DD
    sent_via               TEXT,                          -- post | fax | email | kim | personal
    follow_up_on           TEXT,                          -- YYYY-MM-DD Wiedervorlage / Frist
    follow_up_note         TEXT
);
CREATE INDEX idx_letter_patient ON letter(patient_id);

-- Sections that come from a request (slice 47) carry their own heading and order; template
-- sections keep NULL and follow the template.
ALTER TABLE report_section ADD COLUMN heading TEXT;
ALTER TABLE report_section ADD COLUMN ord INTEGER;

-- Address book. Address and salutation never go to an AI.
CREATE TABLE recipient (
    id               TEXT PRIMARY KEY,
    kind             TEXT NOT NULL,      -- rentenversicherung | pflegekasse | versorgungsamt | arzt | krankenkasse | sonstige
    name             TEXT NOT NULL,
    department       TEXT,
    lines_json       TEXT NOT NULL DEFAULT '[]',   -- street, "PLZ Ort", ...
    salutation       TEXT,
    default_template TEXT,
    use_count        INTEGER NOT NULL DEFAULT 0,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);

-- Recipients a patient has ("Hausärztin", "Psychiater:in") for one-click picks.
CREATE TABLE patient_recipient (
    patient_id   TEXT NOT NULL REFERENCES patient(id) ON DELETE CASCADE,
    recipient_id TEXT NOT NULL REFERENCES recipient(id) ON DELETE CASCADE,
    role         TEXT,
    created_at   TEXT NOT NULL,
    PRIMARY KEY (patient_id, recipient_id)
);

-- What a letter's Betreff needs about the patient.
ALTER TABLE patient ADD COLUMN street TEXT;
ALTER TABLE patient ADD COLUMN postal_code TEXT;
ALTER TABLE patient ADD COLUMN city TEXT;
ALTER TABLE patient ADD COLUMN insurer TEXT;
ALTER TABLE patient ADD COLUMN insurance_no TEXT;
ALTER TABLE patient ADD COLUMN pension_no TEXT;

-- Textbausteine (slice 48).
CREATE TABLE text_block (
    id                 TEXT PRIMARY KEY,
    title              TEXT NOT NULL,
    body               TEXT NOT NULL,
    template_keys_json TEXT NOT NULL DEFAULT '[]',
    section_key        TEXT,
    origin             TEXT NOT NULL DEFAULT 'user',  -- builtin | user
    use_count          INTEGER NOT NULL DEFAULT 0,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL
);

-- Letterhead logo and the optional signature/stamp image, normalized PNG, in the encrypted DB.
CREATE TABLE practice_asset (
    kind       TEXT PRIMARY KEY,                 -- logo | signature
    png        BLOB NOT NULL,
    updated_at TEXT NOT NULL
);

-- Schweigepflichtentbindung per recipient (slice 46).
ALTER TABLE consent ADD COLUMN recipient_id TEXT;
