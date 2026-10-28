-- V15: who speaks in a session recording (Phase 3, slice 27). Labels go into the existing
-- transcript_segment.speaker_label: therapist | patient | speaker_n (not matched to her profile).

-- The diarization engine (runtime + models) a transcript's labels came from; NULL = none ran.
ALTER TABLE transcript ADD COLUMN diarization TEXT;
-- The therapist set this segment's speaker by hand.
ALTER TABLE transcript_segment ADD COLUMN speaker_edited INTEGER NOT NULL DEFAULT 0;

-- One voice print per speaker of a recording (little-endian f32, unit length). Biometric data
-- of the patient too: deleted when the transcript is confirmed, with the audio.
CREATE TABLE source_speaker (
    source_id  TEXT NOT NULL REFERENCES session_source(id) ON DELETE CASCADE,
    label      TEXT NOT NULL,
    embedding  BLOB NOT NULL,
    seconds    REAL NOT NULL
);
CREATE INDEX idx_source_speaker ON source_speaker(source_id);

-- The therapist's voice profile: prints from her dictations and from recordings where she
-- marked herself. Only prints of the current engine count (others aren't comparable).
CREATE TABLE voice_print (
    id          TEXT PRIMARY KEY,
    origin      TEXT NOT NULL,               -- dictation | session
    engine      TEXT NOT NULL,
    embedding   BLOB NOT NULL,
    seconds     REAL NOT NULL,
    created_at  TEXT NOT NULL
);
