-- Voice prep before a session (slice 26): the conversation and the prep sheet drawn from it.
CREATE TABLE session_prep (
    session_id     TEXT PRIMARY KEY REFERENCES session(id) ON DELETE CASCADE,
    transcript     TEXT NOT NULL,                -- JSON [{speaker: therapist|assistant, text}], restored
    seconds        REAL,                         -- billed voice duration (session.closed)
    sheet          TEXT,                         -- prep sheet HTML, editable
    sheet_model    TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);
