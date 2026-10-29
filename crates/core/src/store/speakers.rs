//! Speakers of session recordings and the therapist's voice profile (slice 27). Labels can be
//! changed only while the transcript is a draft; a confirmed transcript keeps its labels and
//! loses the recording's voice prints.

use rusqlite::{params, OptionalExtension};
use serde_json::json;

use super::{Store, ACTOR};
use crate::audit::{self, Entry};
use crate::diarize::{self, DiarizeConfig};
use crate::domain::*;
use crate::{Error, Result};

/// Little-endian f32s.
pub(super) fn blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn unblob(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

impl Store {
    pub fn diarize_config(&self) -> Result<DiarizeConfig> {
        Ok(self.setting(diarize::CONFIG_SETTING)?.map(serde_json::from_value).transpose()?.unwrap_or_default())
    }

    /// Audited: switching off "learn" stops collecting her voice prints (it doesn't delete them).
    pub fn set_diarize_config(&self, c: &DiarizeConfig) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        self.set_setting(diarize::CONFIG_SETTING, &serde_json::to_value(c)?)?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "settings.diarize", entity_type: "setting", entity_id: Some(diarize::CONFIG_SETTING),
            meta: json!({ "enabled": c.enabled, "learn": c.learn }),
        })?;
        tx.commit()?;
        Ok(())
    }

    fn draft_for_speakers(&self, transcript_id: &str) -> Result<Transcript> {
        let t = self.transcript(transcript_id)?;
        if t.status == TranscriptStatus::Confirmed {
            return Err(Error::Invalid("Die Abschrift ist bestätigt und schreibgeschützt".into()));
        }
        Ok(t)
    }

    /// "Neu trennen": separates the speakers of a draft's recording again, as `speakers` people
    /// (also the count a later re-transcription uses). The text stays as it is.
    pub fn enqueue_diarization(&self, source_id: &str, speakers: u8) -> Result<Job> {
        diarize::check_speakers(speakers)?;
        let src = self.source(source_id)?;
        if src.kind != SourceKind::SessionAudio {
            return Err(Error::Invalid("Sprecher werden nur in Sitzungsaufnahmen getrennt".into()));
        }
        if src.audio_deleted_at.is_some() {
            return Err(Error::Invalid("Das Audio wurde bereits gelöscht".into()));
        }
        let t = self.transcript_for_source(source_id)?.ok_or_else(|| Error::Invalid("Noch keine Abschrift".into()))?;
        self.draft_for_speakers(&t.id)?;
        if self.latest_job(source_id)?.is_some_and(|j| matches!(j.status, JobStatus::Queued | JobStatus::Running)) {
            return Err(Error::Invalid("Für diese Aufnahme läuft schon eine Verarbeitung".into()));
        }
        let id = new_id();
        let ts = now();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("UPDATE session_source SET speakers = ?2 WHERE id = ?1", params![source_id, speakers])?;
        tx.execute(
            "INSERT INTO job (id, kind, source_id, created_at, updated_at) VALUES (?1, 'diarize', ?2, ?3, ?3)",
            params![id, source_id, ts],
        )?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "diarization.enqueue", entity_type: "job", entity_id: Some(&id),
            meta: json!({ "source_id": source_id, "speakers": speakers }),
        })?;
        tx.commit()?;
        self.job(&id)
    }

    /// Stores a new speaker separation on the draft and closes the job. Labels set by hand are
    /// replaced too: they referred to the old speakers.
    pub fn finish_diarization(&self, job_id: &str, d: &diarize::Diarization) -> Result<Transcript> {
        let source_id = self.job(job_id)?.source_id.ok_or_else(|| Error::Invalid("job has no source".into()))?;
        let t = self.transcript_for_source(&source_id)?.ok_or_else(|| Error::Invalid("Noch keine Abschrift".into()))?;
        self.draft_for_speakers(&t.id)?;
        if d.labels.len() != t.segments.len() {
            return Err(Error::Invalid("speaker labels don't match the segments".into()));
        }
        let ts = now();
        let tx = self.conn.unchecked_transaction()?;
        for (seg, label) in t.segments.iter().zip(&d.labels) {
            tx.execute(
                "UPDATE transcript_segment SET speaker_label = ?3, speaker_edited = 0 WHERE transcript_id = ?1 AND idx = ?2",
                params![t.id, seg.idx, label],
            )?;
        }
        tx.execute("DELETE FROM source_speaker WHERE source_id = ?1", [&source_id])?;
        for p in &d.speakers {
            tx.execute(
                "INSERT INTO source_speaker (source_id, label, embedding, seconds) VALUES (?1, ?2, ?3, ?4)",
                params![source_id, p.label, blob(&p.embedding), p.seconds as f64],
            )?;
        }
        tx.execute("UPDATE transcript SET diarization = ?2 WHERE id = ?1", params![t.id, d.engine])?;
        tx.execute("UPDATE job SET status = 'done', progress = 100, updated_at = ?2 WHERE id = ?1", params![job_id, ts])?;
        audit::append(&tx, Entry {
            actor: "system", action: "transcript.speakers", entity_type: "transcript", entity_id: Some(&t.id),
            meta: json!({ "action": "rediarize", "speakers": d.speakers.len(), "voice_match": d.matched.is_some() }),
        })?;
        tx.commit()?;
        self.transcript(&t.id)
    }

    /// The therapist corrects the speaker of one segment.
    pub fn set_segment_speaker(&self, transcript_id: &str, idx: i64, label: &str) -> Result<()> {
        if !diarize::is_label(label) {
            return Err(Error::Invalid(format!("Unbekannter Sprecher: {label}")));
        }
        self.draft_for_speakers(transcript_id)?;
        let n = self.conn.execute(
            "UPDATE transcript_segment SET speaker_label = ?3, speaker_edited = 1 WHERE transcript_id = ?1 AND idx = ?2",
            params![transcript_id, idx, label],
        )?;
        if n == 0 {
            return Err(Error::NotFound(format!("segment {idx}")));
        }
        Ok(())
    }

    /// "Sprecher tauschen" for the whole recording.
    pub fn swap_speakers(&self, transcript_id: &str) -> Result<Transcript> {
        let t = self.draft_for_speakers(transcript_id)?;
        let map: Vec<(String, String)> = self
            .speaker_labels(&t)?
            .into_iter()
            .map(|l| {
                let to = diarize::swapped(&l);
                (l, to)
            })
            .filter(|(a, b)| a != b)
            .collect();
        let tx = self.conn.unchecked_transaction()?;
        relabel(&tx, &t, &map)?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "transcript.speakers", entity_type: "transcript", entity_id: Some(transcript_id),
            meta: json!({ "action": "swap" }),
        })?;
        tx.commit()?;
        self.transcript(transcript_id)
    }

    /// "Das bin ich": `label` is the therapist's voice. With `learn` on, that speaker's print
    /// joins her voice profile, so later recordings are labelled without asking.
    pub fn assign_therapist(&self, transcript_id: &str, label: &str) -> Result<Transcript> {
        let t = self.draft_for_speakers(transcript_id)?;
        let labels = self.speaker_labels(&t)?;
        if !labels.iter().any(|l| l == label) {
            return Err(Error::Invalid(format!("Unbekannter Sprecher: {label}")));
        }
        let print: Option<(Vec<u8>, f64)> = self
            .conn
            .query_row(
                "SELECT embedding, seconds FROM source_speaker WHERE source_id = ?1 AND label = ?2",
                params![t.source_id, label],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let learn = self.diarize_config()?.learn;
        let map = diarize::assign_therapist(&labels, label);
        let tx = self.conn.unchecked_transaction()?;
        relabel(&tx, &t, &map)?;
        // Not when the label was already "therapist": that came from her profile, and adding the
        // match back would only reinforce it.
        let enrolled = match (&print, &t.diarization) {
            (Some((embedding, seconds)), Some(engine)) if learn && label != diarize::THERAPIST => {
                insert_print(&tx, "session", engine, &unblob(embedding), *seconds)?;
                true
            }
            _ => false,
        };
        audit::append(&tx, Entry {
            actor: ACTOR, action: "transcript.speakers", entity_type: "transcript", entity_id: Some(transcript_id),
            meta: json!({ "action": "assign", "enrolled": enrolled }),
        })?;
        tx.commit()?;
        self.transcript(transcript_id)
    }

    /// Distinct labels of a transcript's segments and of its recording's prints.
    fn speaker_labels(&self, t: &Transcript) -> Result<Vec<String>> {
        let mut labels: Vec<String> = t.segments.iter().filter_map(|s| s.speaker.clone()).collect();
        let mut stmt = self.conn.prepare("SELECT label FROM source_speaker WHERE source_id = ?1")?;
        labels.extend(stmt.query_map([&t.source_id], |r| r.get::<_, String>(0))?.collect::<std::result::Result<Vec<_>, _>>()?);
        labels.sort();
        labels.dedup();
        Ok(labels)
    }

    /// A print of her voice from a dictation. Keeps the newest [`diarize::MAX_PRINTS`].
    pub fn add_voice_print(&self, origin: &str, engine: &str, embedding: &[f32], seconds: f64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        insert_print(&tx, origin, engine, embedding, seconds)?;
        tx.commit()?;
        Ok(())
    }

    /// Her profile for `engine` (mean of its prints), if she has one.
    pub fn voice_profile_embedding(&self, engine: &str) -> Result<Option<Vec<f32>>> {
        let mut stmt = self.conn.prepare("SELECT embedding FROM voice_print WHERE engine = ?1")?;
        let prints: Vec<Vec<f32>> = stmt
            .query_map([engine], |r| r.get::<_, Vec<u8>>(0))?
            .map(|b| b.map(|b| unblob(&b)))
            .collect::<std::result::Result<_, _>>()?;
        Ok(diarize::mean(&prints))
    }

    pub fn voice_profile(&self, engine: &str) -> Result<VoiceProfile> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(origin = 'dictation'), 0), COALESCE(SUM(origin = 'session'), 0), COALESCE(SUM(seconds), 0)
             FROM voice_print WHERE engine = ?1",
            [engine],
            |r| Ok(VoiceProfile { dictations: r.get(0)?, sessions: r.get(1)?, seconds: r.get(2)? }),
        )?)
    }

    /// Deletes all her voice prints (every engine). Recordings are then labelled "Sprecher 1/2"
    /// until she has dictated or marked herself again.
    pub fn delete_voice_profile(&self) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute("DELETE FROM voice_print", [])?;
        audit::append(&tx, Entry {
            actor: ACTOR, action: "voice.delete", entity_type: "voice_print", entity_id: None,
            meta: json!({ "prints": n }),
        })?;
        tx.commit()?;
        Ok(n)
    }
}

fn insert_print(tx: &rusqlite::Transaction<'_>, origin: &str, engine: &str, embedding: &[f32], seconds: f64) -> Result<()> {
    if embedding.is_empty() || !embedding.iter().all(|x| x.is_finite()) {
        return Err(Error::Invalid("Stimmabdruck ungültig".into()));
    }
    let id = new_id();
    tx.execute(
        "INSERT INTO voice_print (id, origin, engine, embedding, seconds, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, origin, engine, blob(&diarize::normalize(embedding)), seconds, now()],
    )?;
    tx.execute(
        "DELETE FROM voice_print WHERE engine = ?1 AND id NOT IN
           (SELECT id FROM voice_print WHERE engine = ?1 ORDER BY created_at DESC, rowid DESC LIMIT ?2)",
        params![engine, diarize::MAX_PRINTS as i64],
    )?;
    audit::append(tx, Entry {
        actor: "system", action: "voice.enroll", entity_type: "voice_print", entity_id: Some(&id),
        meta: json!({ "origin": origin, "seconds": seconds.round() }),
    })?;
    Ok(())
}

/// Renames labels in a draft's segments and its recording's prints. Two passes, so a swap
/// (a → b, b → a) doesn't merge both into one.
fn relabel(tx: &rusqlite::Transaction<'_>, t: &Transcript, map: &[(String, String)]) -> Result<()> {
    for (from, to) in map {
        tx.execute(
            "UPDATE transcript_segment SET speaker_label = '#' || ?3 WHERE transcript_id = ?1 AND speaker_label = ?2",
            params![t.id, from, to],
        )?;
        tx.execute("UPDATE source_speaker SET label = '#' || ?3 WHERE source_id = ?1 AND label = ?2", params![t.source_id, from, to])?;
    }
    tx.execute(
        "UPDATE transcript_segment SET speaker_label = substr(speaker_label, 2) WHERE transcript_id = ?1 AND speaker_label LIKE '#%'",
        [&t.id],
    )?;
    tx.execute("UPDATE source_speaker SET label = substr(label, 2) WHERE source_id = ?1 AND label LIKE '#%'", [&t.source_id])?;
    Ok(())
}
