-- V1: patient spine, episodes/applications (PTV 3), sessions, reports, templates, audit.
-- Timestamps are RFC 3339 UTC strings. IDs are UUID v4 strings.

CREATE TABLE patient (
    id                TEXT PRIMARY KEY,
    external_ref      TEXT,
    first_name        TEXT NOT NULL,
    last_name         TEXT NOT NULL,
    birth_date        TEXT,                      -- YYYY-MM-DD
    chiffre           TEXT,                      -- pseudonym for the Gutachter (PTV 3)
    pronouns          TEXT,
    contact_json      TEXT NOT NULL DEFAULT '{}',
    consent_recording INTEGER NOT NULL DEFAULT 0, -- derived from latest consent; gates the Record button
    status            TEXT NOT NULL DEFAULT 'active',
    created_at        TEXT NOT NULL,
    archived_at       TEXT
);

CREATE TABLE consent (
    id           TEXT PRIMARY KEY,
    patient_id   TEXT NOT NULL REFERENCES patient(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL,                  -- recording | cloud_processing
    scope        TEXT,
    text_version TEXT NOT NULL,
    text_hash    TEXT NOT NULL,
    granted_at   TEXT NOT NULL,
    revoked_at   TEXT,
    note         TEXT
);

-- Behandlungsfall
CREATE TABLE episode (
    id          TEXT PRIMARY KEY,
    patient_id  TEXT NOT NULL REFERENCES patient(id) ON DELETE CASCADE,
    method      TEXT NOT NULL,                   -- VT | TP | AP
    age_group   TEXT NOT NULL,                   -- adult | kjp
    setting     TEXT NOT NULL,                   -- einzel | gruppe | kombi
    stage       TEXT NOT NULL DEFAULT 'probatorik', -- probatorik | kzt1 | kzt2 | lzt
    started_at  TEXT NOT NULL,
    ended_at    TEXT
);

-- Antrag (Erst-/Umwandlungs-/Fortführungsantrag)
CREATE TABLE application (
    id          TEXT PRIMARY KEY,
    episode_id  TEXT NOT NULL REFERENCES episode(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,                   -- erstantrag | umwandlung | fortfuehrung
    seq         INTEGER NOT NULL DEFAULT 1,      -- Fortführungsberichte are numbered consecutively
    status      TEXT NOT NULL DEFAULT 'draft',
    report_id   TEXT,
    created_at  TEXT NOT NULL
);

CREATE TABLE session (
    id             TEXT PRIMARY KEY,
    patient_id     TEXT NOT NULL REFERENCES patient(id) ON DELETE CASCADE,
    episode_id     TEXT REFERENCES episode(id) ON DELETE SET NULL,
    scheduled_at   TEXT,
    started_at     TEXT,
    ended_at       TEXT,
    duration_s     INTEGER,
    modality       TEXT,                         -- praesenz | video
    status         TEXT NOT NULL DEFAULT 'planned',
    consent_id     TEXT REFERENCES consent(id),
    therapist_note TEXT
);

-- Uploaded documents: old Berichte, forms for the template builder, test results, external reports.
CREATE TABLE source_document (
    id             TEXT PRIMARY KEY,
    patient_id     TEXT REFERENCES patient(id) ON DELETE CASCADE,
    kind           TEXT NOT NULL,                -- old_bericht | template_source | test_result | external_report
    file_name      TEXT NOT NULL,
    path           TEXT NOT NULL,
    sha256         TEXT NOT NULL,
    extracted_text TEXT,
    redacted_text  TEXT,
    status         TEXT NOT NULL DEFAULT 'uploaded',
    created_at     TEXT NOT NULL
);

CREATE TABLE template (
    id          TEXT PRIMARY KEY,
    key         TEXT NOT NULL,
    version     TEXT NOT NULL,
    origin      TEXT NOT NULL DEFAULT 'builtin', -- builtin | user
    parent_key  TEXT,
    definition  TEXT NOT NULL,                   -- validated template JSON
    active      INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT NOT NULL,
    UNIQUE (key, version)
);

CREATE TABLE report (
    id               TEXT PRIMARY KEY,
    patient_id       TEXT NOT NULL REFERENCES patient(id) ON DELETE CASCADE,
    episode_id       TEXT REFERENCES episode(id) ON DELETE CASCADE,
    application_id   TEXT REFERENCES application(id) ON DELETE CASCADE,
    session_id       TEXT REFERENCES session(id) ON DELETE CASCADE,
    template_key     TEXT NOT NULL,
    template_version TEXT NOT NULL,
    kind             TEXT NOT NULL,              -- bericht | sitzungsnotiz
    status           TEXT NOT NULL DEFAULT 'draft', -- draft | in_review | signed | exported
    model            TEXT,
    provider         TEXT,
    prompt_version   TEXT,
    prompt_hash      TEXT,
    params_json      TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    signed_at        TEXT,
    signed_by        TEXT
);

CREATE TABLE report_section (
    id          TEXT PRIMARY KEY,
    report_id   TEXT NOT NULL REFERENCES report(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    body        TEXT NOT NULL DEFAULT '',        -- prose: TipTap HTML; lists: JSON
    origin      TEXT NOT NULL DEFAULT 'human',   -- ai | human | mixed
    edited_at   TEXT,
    UNIQUE (report_id, key)
);

-- Append-only, hash-chained. meta_json must never hold PII (names, free text) so erasure keeps the chain valid.
CREATE TABLE audit_event (
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    at          TEXT NOT NULL,
    actor       TEXT NOT NULL,
    action      TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id   TEXT,
    meta_json   TEXT NOT NULL DEFAULT '{}',
    prev_hash   TEXT NOT NULL,
    hash        TEXT NOT NULL
);
CREATE TRIGGER audit_no_update BEFORE UPDATE ON audit_event
BEGIN SELECT RAISE(ABORT, 'audit_event is append-only'); END;
CREATE TRIGGER audit_no_delete BEFORE DELETE ON audit_event
BEGIN SELECT RAISE(ABORT, 'audit_event is append-only'); END;

CREATE TABLE app_setting (
    key        TEXT PRIMARY KEY,
    value_json TEXT NOT NULL
);

CREATE INDEX idx_episode_patient ON episode(patient_id);
CREATE INDEX idx_session_patient ON session(patient_id);
CREATE INDEX idx_report_patient ON report(patient_id);
CREATE INDEX idx_application_episode ON application(episode_id);
