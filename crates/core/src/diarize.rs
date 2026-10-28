//! Who speaks in a session recording (slice 27): therapist or patient.
//!
//! sherpa-onnx (pyannote segmentation + a speaker embedding model) runs locally in a separate
//! process ([`Sidecar`], the app started again with `--diarize`, see `thera-diarize`). It finds
//! the turns of two speakers and a voice print (embedding) per speaker. The transcript segments
//! get the speaker they overlap most ([`label`]); which speaker is the therapist comes from her
//! voice profile, the mean of prints from her own dictations and from recordings where she
//! marked herself. Without a match the speakers stay "Sprecher 1/2" and she picks.
//!
//! Like the speech sidecar, the plaintext audio exists on disk only in a per-job work directory
//! that is removed afterwards. Prints of the patient's voice are biometric data: they are kept
//! only until the transcript is confirmed (the store deletes them with the audio).

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

pub use thera_diarize::{normalize, Mode, Output, Print, Turn};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::audio::{self, AudioKey};
use crate::domain::Speaker;
use crate::stt::Segment;
use crate::{Error, Result};

pub const THERAPIST: &str = "therapist";
pub const PATIENT: &str = "patient";

/// Cosine similarity from which a speaker counts as the therapist. Measured on TTS voices
/// (same voice 0.98, other voice 0.17); real voices across rooms and microphones score lower.
pub const MATCH: f32 = 0.5;
/// The therapist's speaker must beat the other one by this much, or nobody is labelled.
pub const MARGIN: f32 = 0.15;
/// Newest prints that make up the profile (older ones are deleted).
pub const MAX_PRINTS: usize = 20;
/// Session recordings are taken as two speakers (therapist and patient).
pub const SPEAKERS: i32 = 2;

/// Setting key holding the [`DiarizeConfig`] JSON.
pub const CONFIG_SETTING: &str = "diarize_config";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiarizeConfig {
    /// Separate speakers in session recordings (local, on by default).
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Learn her voice from her dictations and from "das bin ich".
    #[serde(default = "yes")]
    pub learn: bool,
}

fn yes() -> bool {
    true
}

impl Default for DiarizeConfig {
    fn default() -> Self {
        Self { enabled: true, learn: true }
    }
}

/// "Sprecher n" (1-based), for speakers not matched to the profile.
pub fn speaker_label(n: usize) -> String {
    format!("speaker_{n}")
}

pub fn is_label(s: &str) -> bool {
    s == THERAPIST || s == PATIENT || s.strip_prefix("speaker_").is_some_and(|n| matches!(n.parse::<u8>(), Ok(1..=9)))
}

/// The role a stored label stands for.
pub fn role(label: Option<&str>) -> Speaker {
    match label {
        Some(THERAPIST) => Speaker::Therapist,
        Some(PATIENT) => Speaker::Patient,
        _ => Speaker::Unknown,
    }
}

// ---------- sidecar ----------

pub struct Sidecar {
    pub exe: PathBuf,
    /// Arguments before the request path (the app: `--diarize`).
    pub args: Vec<OsString>,
    /// Folder with the library and the models (`crates/app/diarize`).
    pub dir: PathBuf,
    pub threads: i32,
    /// Called with the started process (the app puts it into its kill-on-close job object).
    pub on_spawn: Option<Arc<dyn Fn(&Child) + Send + Sync>>,
}

const SEGMENTATION: &str = "segmentation.onnx";
const EMBEDDING: &str = "embedding.onnx";

impl Sidecar {
    /// Library and both models are there.
    pub fn ready(dir: &Path) -> bool {
        [thera_diarize::LIBRARY, SEGMENTATION, EMBEDDING].iter().all(|f| dir.join(f).exists())
    }

    /// What the folder's `VERSION` says (runtime and models); prints are only comparable within one.
    pub fn version(dir: &Path) -> Option<String> {
        let v = std::fs::read_to_string(dir.join("VERSION")).ok()?;
        Some(v.trim().to_string()).filter(|v| !v.is_empty())
    }

    /// Runs `mode` on `pcm`. `progress` gets 0..=100; setting `cancel` kills the process. The
    /// work directory is removed in every case.
    pub fn run(&self, mode: Mode, pcm: &[i16], work_dir: &Path, progress: impl FnMut(u8), cancel: &AtomicBool) -> Result<Output> {
        let result = self.run_in(mode, pcm, work_dir, progress, cancel);
        let _ = std::fs::remove_dir_all(work_dir);
        result
    }

    fn run_in(&self, mode: Mode, pcm: &[i16], work_dir: &Path, mut progress: impl FnMut(u8), cancel: &AtomicBool) -> Result<Output> {
        std::fs::create_dir_all(work_dir)?;
        let raw: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        let pcm_path = work_dir.join("input.pcm");
        std::fs::write(&pcm_path, raw)?;
        let req = thera_diarize::Request {
            mode,
            pcm: pcm_path,
            out: work_dir.join("out.json"),
            lib_dir: self.dir.clone(),
            segmentation: self.dir.join(SEGMENTATION),
            embedding: self.dir.join(EMBEDDING),
            threads: self.threads,
        };
        let req_path = work_dir.join("request.json");
        std::fs::write(&req_path, serde_json::to_vec(&req)?)?;

        let mut cmd = Command::new(&self.exe);
        cmd.args(&self.args).arg(&req_path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| Error::Stt(format!("Sprechertrennung startet nicht: {e}")))?;
        if let Some(f) = &self.on_spawn {
            f(&child);
        }
        let stderr = child.stderr.take().expect("piped");
        let (tx, rx) = mpsc::channel::<String>();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(std::io::Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut last = String::new();
        let mut drain = |progress: &mut dyn FnMut(u8)| {
            while let Ok(line) = rx.try_recv() {
                match line.strip_prefix("progress ").and_then(|p| p.trim().parse::<u8>().ok()) {
                    Some(p) => progress(p.min(100)),
                    None if !line.trim().is_empty() => last = line,
                    None => {}
                }
            }
        };
        let status = loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(Error::Cancelled);
            }
            drain(&mut progress);
            if let Some(status) = child.try_wait()? {
                break status;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let _ = reader.join();
        drain(&mut progress);
        if !status.success() {
            let detail: String = last.trim_start_matches("error: ").chars().take(300).collect();
            return Err(Error::Stt(format!("Sprechertrennung fehlgeschlagen ({status}): {detail}")));
        }
        let out = std::fs::read(&req.out).map_err(|e| Error::Stt(format!("Sprechertrennung lieferte kein Ergebnis: {e}")))?;
        Ok(serde_json::from_slice(&out)?)
    }
}

/// Decrypts a session recording and finds its speakers.
pub fn diarize_source(audio_dir: &Path, key: &AudioKey, sidecar: &Sidecar, work_dir: &Path, progress: impl FnMut(u8), cancel: &AtomicBool) -> Result<Output> {
    let pcm = audio::read_pcm(audio_dir, key)?;
    sidecar.run(Mode::Diarize { speakers: SPEAKERS }, &pcm, work_dir, progress, cancel)
}

/// Decrypts a dictation and returns the print of its (single) voice, if there was enough speech.
pub fn embed_source(audio_dir: &Path, key: &AudioKey, sidecar: &Sidecar, work_dir: &Path, cancel: &AtomicBool) -> Result<Option<Print>> {
    let pcm = audio::read_pcm(audio_dir, key)?;
    Ok(sidecar.run(Mode::Embed, &pcm, work_dir, |_| {}, cancel)?.speakers.into_iter().next())
}

// ---------- labels ----------

/// A speaker of one recording under its label, kept until the transcript is confirmed (so
/// "das bin ich" can add the therapist's print to her profile).
#[derive(Debug, Clone, PartialEq)]
pub struct LabelledPrint {
    pub label: String,
    pub embedding: Vec<f32>,
    pub seconds: f32,
}

/// What the store keeps of one diarization.
#[derive(Debug, Clone, PartialEq)]
pub struct Diarization {
    /// [`Sidecar::version`].
    pub engine: String,
    /// One per transcript segment; `None` where no speaker turn is near.
    pub labels: Vec<Option<String>>,
    pub speakers: Vec<LabelledPrint>,
    /// Similarity of the speaker taken for the therapist, if one was.
    pub matched: Option<f32>,
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Unit-length mean of unit-length prints (the voice profile).
pub fn mean(prints: &[Vec<f32>]) -> Option<Vec<f32>> {
    let first = prints.first()?;
    let mut sum = vec![0.0f32; first.len()];
    for p in prints.iter().filter(|p| p.len() == first.len()) {
        sum.iter_mut().zip(p).for_each(|(s, x)| *s += x);
    }
    Some(thera_diarize::normalize(&sum))
}

/// Names the speakers and gives each transcript segment the speaker it overlaps most.
pub fn label(segments: &[Segment], out: &Output, profile: Option<&[f32]>, engine: &str) -> Diarization {
    // Speakers in order of first appearance: the first to speak is "Sprecher 1".
    let mut order: Vec<i32> = Vec::new();
    for t in &out.turns {
        if !order.contains(&t.speaker) {
            order.push(t.speaker);
        }
    }
    let prints: HashMap<i32, &Print> = out.speakers.iter().map(|p| (p.speaker, p)).collect();

    let mut matched = None;
    let mut names: HashMap<i32, String> = HashMap::new();
    if let Some(profile) = profile {
        let mut sims: Vec<(i32, f32)> =
            order.iter().filter_map(|id| prints.get(id).map(|p| (*id, dot(profile, &p.embedding)))).collect();
        sims.sort_by(|a, b| b.1.total_cmp(&a.1));
        if let Some(&(best, sim)) = sims.first() {
            let runner_up = sims.get(1).map_or(f32::MIN, |s| s.1);
            if sim >= MATCH && sim - runner_up >= MARGIN {
                matched = Some(sim);
                names.insert(best, THERAPIST.into());
                let others: Vec<i32> = order.iter().copied().filter(|id| *id != best).collect();
                if let [other] = others[..] {
                    names.insert(other, PATIENT.into());
                }
            }
        }
    }
    let mut n = 0;
    for id in &order {
        if !names.contains_key(id) {
            n += 1;
            names.insert(*id, speaker_label(n));
        }
    }

    let labels = segments
        .iter()
        .map(|s| {
            let mut overlap: HashMap<i32, i64> = HashMap::new();
            for t in &out.turns {
                let o = s.end_ms.min(t.end_ms) - s.start_ms.max(t.start_ms);
                if o > 0 {
                    *overlap.entry(t.speaker).or_default() += o;
                }
            }
            let id = match overlap.into_iter().max_by_key(|(id, o)| (*o, std::cmp::Reverse(order.iter().position(|x| x == id)))) {
                Some((id, _)) => Some(id),
                // A segment in a gap between turns: the nearest turn within a second.
                None => out
                    .turns
                    .iter()
                    .map(|t| (t.speaker, (t.start_ms - s.end_ms).max(s.start_ms - t.end_ms)))
                    .filter(|(_, gap)| *gap <= 1000)
                    .min_by_key(|(_, gap)| *gap)
                    .map(|(id, _)| id),
            };
            id.and_then(|id| names.get(&id).cloned())
        })
        .collect();

    let speakers = order
        .iter()
        .filter_map(|id| {
            let p = prints.get(id)?;
            Some(LabelledPrint { label: names[id].clone(), embedding: p.embedding.clone(), seconds: p.seconds })
        })
        .collect();
    Diarization { engine: engine.to_string(), labels, speakers, matched }
}

/// "Sprecher tauschen": therapist ↔ patient and Sprecher 1 ↔ Sprecher 2.
pub fn swapped(label: &str) -> String {
    match label {
        THERAPIST => PATIENT.into(),
        PATIENT => THERAPIST.into(),
        "speaker_1" => "speaker_2".into(),
        "speaker_2" => "speaker_1".into(),
        other => other.into(),
    }
}

/// The renaming when the therapist says `chosen` is her voice: `chosen` becomes the therapist;
/// with one other speaker that one becomes the patient, with more they keep (or, for a former
/// therapist, get) a "Sprecher n" label. `labels` are the distinct labels of the recording.
pub fn assign_therapist(labels: &[String], chosen: &str) -> Vec<(String, String)> {
    let mut map = vec![(chosen.to_string(), THERAPIST.to_string())];
    let others: Vec<&String> = labels.iter().filter(|l| l.as_str() != chosen).collect();
    if let [other] = others[..] {
        map.push((other.clone(), PATIENT.into()));
    } else if others.iter().any(|l| l.as_str() == THERAPIST) {
        let free = (1..=9).map(speaker_label).find(|l| !labels.contains(l)).unwrap_or_else(|| speaker_label(9));
        map.push((THERAPIST.into(), free));
    }
    map.retain(|(a, b)| a != b);
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start_ms: i64, end_ms: i64) -> Segment {
        Segment { start_ms, end_ms, text: "x".into() }
    }

    fn turn(start_ms: i64, end_ms: i64, speaker: i32) -> Turn {
        Turn { start_ms, end_ms, speaker }
    }

    fn print(speaker: i32, embedding: Vec<f32>) -> Print {
        Print { speaker, embedding, seconds: 10.0 }
    }

    fn out() -> Output {
        Output {
            turns: vec![turn(0, 3000, 1), turn(3500, 9000, 0), turn(9500, 12_000, 1)],
            speakers: vec![print(0, vec![1.0, 0.0]), print(1, vec![0.0, 1.0])],
        }
    }

    #[test]
    fn labels_follow_the_largest_overlap_and_first_appearance() {
        let segs = [seg(0, 2900), seg(2800, 8000), seg(9200, 9400), seg(20_000, 21_000)];
        let d = label(&segs, &out(), None, "v");
        // Speaker id 1 spoke first, so it is "Sprecher 1".
        assert_eq!(
            d.labels,
            vec![Some("speaker_1".into()), Some("speaker_2".into()), Some("speaker_1".into()), None],
            "segment in a gap takes the nearest turn within 1 s; far away stays open"
        );
        assert_eq!(d.matched, None);
        assert_eq!(d.speakers.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["speaker_1", "speaker_2"]);
    }

    #[test]
    fn profile_names_therapist_and_patient() {
        let segs = [seg(0, 2900), seg(3600, 8000)];
        let d = label(&segs, &out(), Some(&[0.1, 0.99]), "v");
        assert_eq!(d.labels, vec![Some(THERAPIST.into()), Some(PATIENT.into())]);
        assert!(d.matched.unwrap() > 0.9);
        assert_eq!(d.speakers[0].label, THERAPIST);
    }

    #[test]
    fn unsure_profile_leaves_speakers_unnamed() {
        let segs = [seg(0, 2900)];
        // Too far from both.
        assert_eq!(label(&segs, &out(), Some(&[0.3, 0.3]), "v").labels, vec![Some("speaker_1".into())]);
        // Close to both: no margin.
        let near = Output { speakers: vec![print(0, vec![0.8, 0.6]), print(1, vec![0.6, 0.8])], ..out() };
        assert_eq!(label(&segs, &near, Some(&[0.70710677, 0.70710677]), "v").matched, None);
    }

    #[test]
    fn one_speaker_matching_the_profile_is_the_therapist() {
        let one = Output { turns: vec![turn(0, 5000, 0)], speakers: vec![print(0, vec![1.0, 0.0])] };
        assert_eq!(label(&[seg(0, 4000)], &one, Some(&[1.0, 0.0]), "v").labels, vec![Some(THERAPIST.into())]);
        let empty = Output::default();
        assert_eq!(label(&[seg(0, 4000)], &empty, Some(&[1.0, 0.0]), "v").labels, vec![None]);
    }

    #[test]
    fn swap_and_assign() {
        assert_eq!(swapped(THERAPIST), PATIENT);
        assert_eq!(swapped("speaker_2"), "speaker_1");
        assert_eq!(swapped("speaker_3"), "speaker_3");
        let two = ["speaker_1".to_string(), "speaker_2".to_string()];
        assert_eq!(
            assign_therapist(&two, "speaker_2"),
            vec![("speaker_2".into(), THERAPIST.into()), ("speaker_1".into(), PATIENT.into())]
        );
        let named = [THERAPIST.to_string(), PATIENT.to_string()];
        assert_eq!(
            assign_therapist(&named, PATIENT),
            vec![(PATIENT.into(), THERAPIST.into()), (THERAPIST.into(), PATIENT.into())]
        );
        assert!(assign_therapist(&named, THERAPIST).is_empty(), "already right");
        let three = [THERAPIST.to_string(), "speaker_2".to_string(), "speaker_3".to_string()];
        assert_eq!(
            assign_therapist(&three, "speaker_3"),
            vec![("speaker_3".into(), THERAPIST.into()), (THERAPIST.into(), "speaker_1".into())]
        );
    }

    #[test]
    fn labels_and_roles() {
        for ok in [THERAPIST, PATIENT, "speaker_1", "speaker_9"] {
            assert!(is_label(ok), "{ok}");
        }
        for bad in ["", "speaker_0", "speaker_10", "Therapist", "speaker_x"] {
            assert!(!is_label(bad), "{bad}");
        }
        assert_eq!(role(Some(THERAPIST)), Speaker::Therapist);
        assert_eq!(role(Some("speaker_1")), Speaker::Unknown);
        assert_eq!(role(None), Speaker::Unknown);
    }

    #[test]
    fn profile_is_the_normalized_mean() {
        let m = mean(&[vec![1.0, 0.0], vec![0.0, 1.0]]).unwrap();
        assert!((m[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!(mean(&[]).is_none());
    }
}
