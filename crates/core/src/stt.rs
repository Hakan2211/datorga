//! Local speech-to-text through a whisper.cpp sidecar (`whisper-cli`), run as a separate process.
//!
//! whisper-cli reads a WAV file and writes JSON, so plaintext audio and text exist on disk while
//! it runs. Both live in a per-job work directory that is always removed afterwards, also on
//! error or cancel. A crash can leave one behind: the caller wipes the whole work root on startup.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use serde::Deserialize;

use crate::audio::{self, AudioKey};
use crate::{Error, Result};

pub const ENGINE: &str = "whisper.cpp";

/// One timed piece of recognized text, as the engine returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

// ---------- model catalog ----------

pub use crate::models::{sha256_file, ModelInfo};

pub const DEFAULT_MODEL: &str = "large-v3-turbo-q5_0";

pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        id: "large-v3-turbo-q5_0",
        file: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        bytes: 574_041_195,
    },
    ModelInfo {
        id: "large-v3-turbo",
        file: "ggml-large-v3-turbo.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        sha256: "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
        bytes: 1_624_555_275,
    },
    ModelInfo {
        id: "small-q5_1",
        file: "ggml-small-q5_1.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
        sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
        bytes: 190_085_487,
    },
];

pub fn model(id: &str) -> Result<&'static ModelInfo> {
    MODELS.iter().find(|m| m.id == id).ok_or_else(|| Error::Invalid(format!("unknown model {id}")))
}

// ---------- sidecar ----------

pub struct WhisperCli {
    pub exe: PathBuf,
    pub model: PathBuf,
    pub language: String,
    pub threads: usize,
}

impl WhisperCli {
    /// Runs the sidecar on `wav`. `progress` gets 0..=100; setting `cancel` kills the process.
    pub fn run(&self, wav: &Path, work_dir: &Path, mut progress: impl FnMut(u8), cancel: &AtomicBool) -> Result<Vec<Segment>> {
        let out_base = work_dir.join("out");
        let mut cmd = Command::new(&self.exe);
        cmd.arg("-m").arg(&self.model)
            .arg("-f").arg(wav)
            .arg("-l").arg(&self.language)
            .arg("-t").arg(self.threads.to_string())
            .arg("-oj")
            .arg("-of").arg(&out_base)
            .arg("-pp")
            .stdin(Stdio::null())
            .stdout(Stdio::null()) // the transcript goes to the JSON file only
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| Error::Stt(format!("Transkriptionsprogramm startet nicht: {e}")))?;

        let stderr = child.stderr.take().expect("piped");
        let (tx, rx) = mpsc::channel::<String>();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(std::io::Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });

        let mut last_lines: Vec<String> = Vec::new();
        let mut drain = |rx: &mpsc::Receiver<String>, progress: &mut dyn FnMut(u8)| {
            while let Ok(line) = rx.try_recv() {
                match parse_progress(&line) {
                    Some(p) => progress(p),
                    None if !line.trim().is_empty() => {
                        last_lines.push(line);
                        if last_lines.len() > 5 {
                            last_lines.remove(0);
                        }
                    }
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
            drain(&rx, &mut progress);
            if let Some(status) = child.try_wait()? {
                break status;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let _ = reader.join();
        drain(&rx, &mut progress);
        if !status.success() {
            let detail: String = last_lines.join(" | ").chars().take(300).collect();
            return Err(Error::Stt(format!("Transkription fehlgeschlagen ({status}): {detail}")));
        }
        let json = std::fs::read(out_base.with_extension("json"))
            .map_err(|e| Error::Stt(format!("Transkription lieferte kein Ergebnis: {e}")))?;
        let segments = parse_json(&json);
        progress(100);
        segments
    }
}

/// Decrypts a recording, runs the sidecar and returns its segments. The work directory holds
/// the only plaintext copies (WAV, JSON) and is removed in every case.
pub fn transcribe(
    audio_dir: &Path,
    key: &AudioKey,
    cli: &WhisperCli,
    work_dir: &Path,
    progress: impl FnMut(u8),
    cancel: &AtomicBool,
) -> Result<Vec<Segment>> {
    let result = (|| {
        std::fs::create_dir_all(work_dir)?;
        let pcm = audio::read_pcm(audio_dir, key)?;
        if pcm.is_empty() {
            return Err(Error::Invalid("Die Aufnahme ist leer".into()));
        }
        let envelope = loudness_envelope(&pcm);
        let wav = work_dir.join("input.wav");
        audio::write_wav(&wav, &pcm)?;
        drop(pcm);
        let mut segments = cli.run(&wav, work_dir, progress, cancel)?;
        segments.retain(|s| has_sound(&envelope, s.start_ms, s.end_ms));
        Ok(segments)
    })();
    let _ = std::fs::remove_dir_all(work_dir);
    result
}

const WINDOW_MS: i64 = 100;
/// Whisper invents fluent text on silence (a muted mic, the pause after a dictation). A segment
/// whose loudest 100 ms stays below this is dropped. Speech, even quiet, is far above it.
const SILENCE_DBFS: f32 = -50.0;

/// RMS level in dBFS per 100 ms window.
fn loudness_envelope(pcm: &[i16]) -> Vec<f32> {
    let window = (audio::SAMPLE_RATE as i64 * WINDOW_MS / 1000) as usize;
    pcm.chunks(window)
        .map(|w| {
            let mean_sq = w.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / w.len() as f64;
            (20.0 * (mean_sq.sqrt() / 32768.0).max(1e-9).log10()) as f32
        })
        .collect()
}

fn has_sound(envelope: &[f32], start_ms: i64, end_ms: i64) -> bool {
    let from = (start_ms / WINDOW_MS).max(0) as usize;
    let to = ((end_ms + WINDOW_MS - 1) / WINDOW_MS) as usize;
    envelope.get(from..to.min(envelope.len())).is_some_and(|w| w.iter().any(|&db| db > SILENCE_DBFS))
}

/// `whisper_print_progress_callback: progress =  42%` → 42.
pub fn parse_progress(line: &str) -> Option<u8> {
    let rest = line.split("progress =").nth(1)?;
    rest.trim().trim_end_matches('%').trim().parse::<u8>().ok().map(|p| p.min(100))
}

#[derive(Deserialize)]
struct WhisperJson {
    transcription: Vec<WhisperSegment>,
}

#[derive(Deserialize)]
struct WhisperSegment {
    offsets: Offsets,
    text: String,
}

#[derive(Deserialize)]
struct Offsets {
    from: i64,
    to: i64,
}

/// Whisper's well-known German hallucinations, learned from TV subtitle credits. They also
/// appear over quiet room noise, which the silence gate lets through.
const HALLUCINATIONS: &[&str] = &["untertitel von stephanie geiges", "vielen dank fürs zuschauen", "copyright wdr"];

fn is_hallucination(text: &str) -> bool {
    let t = text.to_lowercase();
    (t.contains("untertitel") && (t.contains("zdf") || t.contains("amara"))) || HALLUCINATIONS.iter().any(|h| t.starts_with(h))
}

/// Parses `whisper-cli -oj` output. Bytes are decoded lossily: whisper can split a multi-byte
/// character across tokens.
pub fn parse_json(bytes: &[u8]) -> Result<Vec<Segment>> {
    let text = String::from_utf8_lossy(bytes);
    let parsed: WhisperJson =
        serde_json::from_str(&text).map_err(|e| Error::Stt(format!("unerwartete Ausgabe der Transkription: {e}")))?;
    Ok(parsed
        .transcription
        .into_iter()
        .map(|s| Segment { start_ms: s.offsets.from, end_ms: s.offsets.to, text: s.text.trim().to_string() })
        .filter(|s| !s.text.is_empty() && !is_hallucination(&s.text))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_whisper_json_and_drops_empty_and_hallucinated_segments() {
        let json = r#"{
            "systeminfo": "AVX2 = 1",
            "result": { "language": "de" },
            "transcription": [
                { "timestamps": { "from": "00:00:00,000", "to": "00:00:04,200" },
                  "offsets": { "from": 0, "to": 4200 }, "text": " Die Patientin schläft schlecht." },
                { "offsets": { "from": 4200, "to": 5000 }, "text": "   " },
                { "offsets": { "from": 5000, "to": 9000 }, "text": " Untertitel im Auftrag des ZDF, 2020" },
                { "offsets": { "from": 9000, "to": 12000 }, "text": " Wir üben die Atemtechnik." }
            ]
        }"#;
        let segs = parse_json(json.as_bytes()).unwrap();
        assert_eq!(
            segs,
            vec![
                Segment { start_ms: 0, end_ms: 4200, text: "Die Patientin schläft schlecht.".into() },
                Segment { start_ms: 9000, end_ms: 12000, text: "Wir üben die Atemtechnik.".into() },
            ]
        );
    }

    #[test]
    fn split_utf8_does_not_fail_parsing() {
        let mut json = br#"{"transcription":[{"offsets":{"from":0,"to":1},"text":" sch"#.to_vec();
        json.push(0xC3); // first byte of "ä" only
        json.extend(br#"n"}]}"#);
        assert_eq!(parse_json(&json).unwrap()[0].text, "sch\u{FFFD}n");
    }

    #[test]
    fn subtitle_credit_variants_are_hallucinations() {
        for t in ["Untertitelung des ZDF, 2020", "Untertitel im Auftrag des ZDF für funk, 2017", "Untertitel der Amara.org-Community"] {
            assert!(is_hallucination(t), "{t}");
        }
        assert!(!is_hallucination("Sie sieht abends gern ZDF."));
    }

    #[test]
    fn silence_gate_uses_the_loudest_window() {
        // 1 s silence, 0.5 s speech-level signal, 1 s silence.
        let mut pcm = vec![0i16; 16_000];
        pcm.extend((0..8_000).map(|i| if i % 2 == 0 { 3_000 } else { -3_000 }));
        pcm.extend(vec![0i16; 16_000]);
        let env = loudness_envelope(&pcm);
        assert_eq!(env.len(), 25);
        assert!(!has_sound(&env, 0, 1000), "silence");
        assert!(has_sound(&env, 0, 2500), "segment spanning the speech");
        assert!(!has_sound(&env, 1500, 2500));
        assert!(!has_sound(&env, 9000, 12000), "past the end");
        // Room noise around -60 dBFS stays below the gate.
        let noise: Vec<i16> = (0..16_000).map(|i| if i % 2 == 0 { 30 } else { -30 }).collect();
        assert!(!has_sound(&loudness_envelope(&noise), 0, 1000));
    }

    #[test]
    fn progress_lines() {
        assert_eq!(parse_progress("whisper_print_progress_callback: progress =  42%"), Some(42));
        assert_eq!(parse_progress("main: progress = 100%"), Some(100));
        assert_eq!(parse_progress("whisper_init_from_file: loading model"), None);
    }

    #[test]
    fn catalog_has_default_and_valid_hashes() {
        assert!(model(DEFAULT_MODEL).is_ok());
        assert!(MODELS.iter().all(|m| m.sha256.len() == 64 && m.url.ends_with(m.file)));
    }
}
