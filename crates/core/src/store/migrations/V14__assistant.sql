-- V14: the assistant chat (Phase 3, slice 25). The assistant reads through tools and only
-- proposes changes; the therapist applies or discards each proposal.

CREATE TABLE chat_thread (
    id          TEXT PRIMARY KEY,
    scope       TEXT NOT NULL,              -- global | patient | session
    ref_id      TEXT,                       -- the patient or session of a scoped thread
    title       TEXT,
    token_map   TEXT NOT NULL,              -- JSON TokenMap: a name keeps its token for the whole thread
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- Text is stored restored (real names), encrypted with the rest of the database.
CREATE TABLE chat_message (
    id            TEXT PRIMARY KEY,
    thread_id     TEXT NOT NULL REFERENCES chat_thread(id) ON DELETE CASCADE,
    seq           INTEGER NOT NULL,
    role          TEXT NOT NULL,            -- user | assistant | tool
    content       TEXT NOT NULL,
    tool_calls    TEXT,                     -- assistant: JSON [{id, name, arguments}]
    tool_call_id  TEXT,                     -- tool: the call it answers
    tool_name     TEXT,
    created_at    TEXT NOT NULL,
    UNIQUE (thread_id, seq)
);

-- Patients a thread touched (its scope, tool reads, proposals, names in the text). Erasing a
-- patient deletes these threads. No foreign key: the link must still be there when the patient
-- row goes.
CREATE TABLE chat_thread_patient (
    thread_id   TEXT NOT NULL REFERENCES chat_thread(id) ON DELETE CASCADE,
    patient_id  TEXT NOT NULL,
    PRIMARY KEY (thread_id, patient_id)
);
CREATE INDEX idx_chat_thread_patient ON chat_thread_patient(patient_id);

CREATE TABLE agent_proposal (
    id          TEXT PRIMARY KEY,
    thread_id   TEXT NOT NULL REFERENCES chat_thread(id) ON DELETE CASCADE,
    message_id  TEXT NOT NULL REFERENCES chat_message(id) ON DELETE CASCADE,  -- the tool result
    kind        TEXT NOT NULL,              -- session | reschedule | task | section_text | note_text | day_plan
    patient_id  TEXT REFERENCES patient(id) ON DELETE CASCADE,
    summary     TEXT NOT NULL,
    payload     TEXT NOT NULL,              -- JSON; checked when proposed and again when applied
    status      TEXT NOT NULL DEFAULT 'open',  -- open | applied | discarded
    result_id   TEXT,                       -- what applying created or changed
    created_at  TEXT NOT NULL,
    decided_at  TEXT
);
CREATE INDEX idx_agent_proposal_thread ON agent_proposal(thread_id);
