//! Speech-to-text: locally through a whisper.cpp sidecar (`whisper-cli`, a separate process), or
//! through an OpenAI-compatible cloud endpoint: the therapist's own dictations always may, session
//! recordings only with the patient's consent to cloud transcription.
//!
//! whisper-cli reads a WAV file and writes JSON, so plaintext audio and text exist on disk while
//! it runs. Both live in a per-job work directory that is always removed afterwards, also on
//! error or cancel. A crash can leave one behind: the caller wipes the whole work root on startup.
//! The cloud engine sends the audio from memory; it never touches the disk.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::audio::{self, AudioKey};
use crate::domain::{SourceKind, SttTarget};
use crate::llm::{Endpoint, ProviderId};
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
    /// Called with the started process (the app puts it into its kill-on-close job object).
    pub on_spawn: Option<Arc<dyn Fn(&Child) + Send + Sync>>,
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

// ---------- engine choice ----------

/// Setting key holding the [`SttConfig`] JSON.
pub const CONFIG_SETTING: &str = "stt_config";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SttMode {
    /// Everything on this computer.
    #[default]
    Local,
    /// Dictations in the cloud; session recordings too where the patient consented to it.
    Cloud,
    /// Locally, in the cloud only when no local model is ready (same consent rule).
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SttConfig {
    #[serde(default)]
    pub mode: SttMode,
    /// OpenRouter or the custom endpoint of the AI settings (same key, same base URL).
    #[serde(default = "default_cloud_provider")]
    pub cloud_provider: ProviderId,
    /// `None` = [`CLOUD_DEFAULT_MODEL`] on OpenRouter; a custom endpoint needs one.
    #[serde(default)]
    pub cloud_model: Option<String>,
}

fn default_cloud_provider() -> ProviderId {
    ProviderId::Openrouter
}

impl Default for SttConfig {
    fn default() -> Self {
        Self { mode: SttMode::Local, cloud_provider: default_cloud_provider(), cloud_model: None }
    }
}

impl SttConfig {
    pub fn validate(&self) -> Result<()> {
        if !matches!(self.cloud_provider, ProviderId::Openrouter | ProviderId::Custom) {
            return Err(Error::Invalid("Cloud-Transkription gibt es nur über OpenRouter oder einen eigenen Endpunkt".into()));
        }
        Ok(())
    }

    /// The model to send, if any.
    pub fn model(&self) -> Option<String> {
        match self.cloud_model.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
            Some(m) => Some(m.to_string()),
            None if self.cloud_provider == ProviderId::Openrouter => Some(CLOUD_DEFAULT_MODEL.to_string()),
            None => None,
        }
    }
}

/// Whether `kind` may go to the cloud. A session recording holds the patient's voice and needs
/// the patient's active `cloud_transcription` consent; a dictation is the therapist alone.
pub fn cloud_allowed(kind: SourceKind, cloud_consent: bool) -> bool {
    kind == SourceKind::Dictation || cloud_consent
}

/// The engine Settings pick for a recording. `local_ready`: sidecar and speech model installed.
pub fn default_target(config: &SttConfig, kind: SourceKind, local_ready: bool, cloud_consent: bool) -> SttTarget {
    let cloud = match config.mode {
        SttMode::Local => false,
        SttMode::Cloud => true,
        SttMode::Fallback => !local_ready,
    };
    if cloud && cloud_allowed(kind, cloud_consent) { SttTarget::Cloud } else { SttTarget::Local }
}

/// The engine a job runs on: the one chosen in the session, else [`default_target`]. A cloud
/// choice for a session recording without consent is refused, not quietly run locally.
pub fn choose(
    config: &SttConfig,
    kind: SourceKind,
    local_ready: bool,
    cloud_consent: bool,
    requested: Option<SttTarget>,
) -> Result<SttTarget> {
    match requested {
        Some(SttTarget::Cloud) if !cloud_allowed(kind, cloud_consent) => Err(Error::Invalid(NO_CLOUD_CONSENT.into())),
        Some(t) => Ok(t),
        None => Ok(default_target(config, kind, local_ready, cloud_consent)),
    }
}

pub const NO_CLOUD_CONSENT: &str =
    "Für die Cloud-Transkription dieser Sitzungsaufnahme fehlt die Einwilligung der Patientin bzw. des Patienten";

/// A transcription model on OpenRouter, chosen by the user (2026-10-28): German-capable, with a
/// zero-retention endpoint (requests are sent with `zdr: true` and fail without one).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CloudSttModel {
    pub id: &'static str,
    pub label: &'static str,
    /// OpenRouter price per hour of audio (2026-10-28). `None`: the listed price has no clear unit.
    pub usd_per_hour: Option<f64>,
    /// Who serves it with zero retention.
    pub zdr_via: &'static str,
}

pub const CLOUD_DEFAULT_MODEL: &str = "openai/whisper-large-v3-turbo";

pub const CLOUD_MODELS: &[CloudSttModel] = &[
    CloudSttModel { id: "openai/whisper-large-v3-turbo", label: "Whisper Large v3 Turbo", usd_per_hour: Some(0.012), zdr_via: "Groq, DeepInfra" },
    CloudSttModel { id: "openai/whisper-large-v3", label: "Whisper Large v3", usd_per_hour: Some(0.027), zdr_via: "Groq, Together, DeepInfra" },
    // Listed as 0.1 per unit of audio; read as per second (like the others) that would be 360 $/h.
    CloudSttModel { id: "microsoft/mai-transcribe-2", label: "Microsoft MAI-Transcribe 2", usd_per_hour: None, zdr_via: "Azure" },
    CloudSttModel { id: "elevenlabs/scribe-v2", label: "ElevenLabs Scribe v2", usd_per_hour: Some(0.22), zdr_via: "ElevenLabs" },
];

// ---------- engines ----------

/// Where a recording is transcribed. The cloud gets the audio itself, which can't be
/// pseudonymized, so [`transcribe`] runs it only for the therapist's own dictations.
pub enum SttEngine {
    Local(WhisperCli),
    Cloud(CloudStt),
}

/// What a cloud transcription used, for the audit log (no text).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CloudUsage {
    /// Audio seconds sent (silent pieces are skipped).
    pub seconds: f64,
    /// Billed USD, when the router reports it (OpenRouter does).
    pub cost: Option<f64>,
    pub requests: usize,
}

#[derive(Debug)]
pub struct Transcribed {
    pub segments: Vec<Segment>,
    /// `None` for the local engine.
    pub cloud: Option<CloudUsage>,
}

/// Decrypts a recording and transcribes it with `engine`. The local engine's work directory
/// holds the only plaintext copies (WAV, JSON) and is removed in every case; the cloud engine
/// keeps the audio in memory. Cloud is refused for a session recording without `cloud_consent`
/// (the patient's active consent, looked up when the job runs).
#[allow(clippy::too_many_arguments)]
pub fn transcribe(
    kind: SourceKind,
    cloud_consent: bool,
    audio_dir: &Path,
    key: &AudioKey,
    engine: &SttEngine,
    work_dir: &Path,
    progress: impl FnMut(u8),
    cancel: &AtomicBool,
) -> Result<Transcribed> {
    if matches!(engine, SttEngine::Cloud(_)) && !cloud_allowed(kind, cloud_consent) {
        return Err(Error::Invalid(NO_CLOUD_CONSENT.into()));
    }
    let result = (|| {
        let pcm = audio::read_pcm(audio_dir, key)?;
        if pcm.is_empty() {
            return Err(Error::Invalid("Die Aufnahme ist leer".into()));
        }
        let envelope = loudness_envelope(&pcm);
        let (mut segments, cloud) = match engine {
            SttEngine::Local(cli) => {
                std::fs::create_dir_all(work_dir)?;
                let wav = work_dir.join("input.wav");
                audio::write_wav(&wav, &pcm)?;
                drop(pcm);
                (cli.run(&wav, work_dir, progress, cancel)?, None)
            }
            SttEngine::Cloud(cloud) => {
                let (segments, usage) = cloud.run(&pcm, &envelope, progress, cancel)?;
                (segments, Some(usage))
            }
        };
        segments.retain(|s| has_sound(&envelope, s.start_ms, s.end_ms) && !is_hallucination(&s.text));
        Ok(Transcribed { segments, cloud })
    })();
    let _ = std::fs::remove_dir_all(work_dir);
    result
}

// ---------- cloud ----------

/// Upstream providers give up after 60 s per request (OpenRouter), and a multipart upload may
/// be at most 25 MB: 4 minutes of 16 kHz mono WAV is 7.7 MB and transcribes in a few seconds.
const PIECE_MS: i64 = 240_000;
/// A piece ends at the quietest 100 ms within its last 20 s, so words aren't cut in half.
const CUT_SEARCH_MS: i64 = 20_000;

/// An OpenAI-compatible `POST /audio/transcriptions` (OpenRouter or a custom endpoint).
pub struct CloudStt {
    pub endpoint: Endpoint,
    pub model: String,
    pub language: String,
}

impl CloudStt {
    /// Transcribes `pcm` piece by piece. `envelope` is its [`loudness_envelope`]: silent pieces
    /// aren't sent. Setting `cancel` returns within ~100 ms.
    fn run(&self, pcm: &[i16], envelope: &[f32], mut progress: impl FnMut(u8), cancel: &AtomicBool) -> Result<(Vec<Segment>, CloudUsage)> {
        if self.model.trim().is_empty() {
            return Err(Error::Invalid("Kein Cloud-Sprachmodell gewählt".into()));
        }
        let pieces = pieces(envelope, PIECE_MS, CUT_SEARCH_MS);
        let (mut segments, mut usage) = (Vec::new(), CloudUsage::default());
        let samples_per_ms = audio::SAMPLE_RATE as usize / 1000;
        for (i, (from_ms, to_ms)) in pieces.iter().copied().enumerate() {
            if !has_sound(envelope, from_ms, to_ms) {
                continue;
            }
            let piece = &pcm[(from_ms as usize * samples_per_ms).min(pcm.len())..(to_ms as usize * samples_per_ms).min(pcm.len())];
            let wav = audio::wav_bytes(piece);
            let (req, ep) = (self.request(wav)?, self.endpoint.clone());
            let answer = cancellable(cancel, move || req.send(&ep))?;
            usage.requests += 1;
            usage.seconds += answer.seconds.unwrap_or((to_ms - from_ms) as f64 / 1000.0);
            if let Some(c) = answer.cost {
                *usage.cost.get_or_insert(0.0) += c;
            }
            segments.extend(answer.segments.into_iter().map(|s| Segment {
                start_ms: (s.start_ms + from_ms).min(to_ms),
                end_ms: s.end_ms.map_or(to_ms, |e| (e + from_ms).min(to_ms)),
                text: s.text,
            }));
            progress(((i + 1) * 100 / pieces.len()) as u8);
        }
        progress(100);
        Ok((segments, usage))
    }

    /// One short piece of a live dictation, from memory, as plain text.
    pub fn transcribe_piece(&self, pcm: &[i16], cancel: &AtomicBool) -> Result<(String, CloudUsage)> {
        if self.model.trim().is_empty() {
            return Err(Error::Invalid("Kein Cloud-Sprachmodell gewählt".into()));
        }
        let (req, ep) = (self.request(audio::wav_bytes(pcm))?, self.endpoint.clone());
        let answer = cancellable(cancel, move || req.send(&ep))?;
        let seconds = answer.seconds.unwrap_or(pcm.len() as f64 / audio::SAMPLE_RATE as f64);
        let text = answer.segments.into_iter().map(|s| s.text).collect::<Vec<_>>().join(" ");
        Ok((text, CloudUsage { seconds, cost: answer.cost, requests: 1 }))
    }

    fn request(&self, wav: Vec<u8>) -> Result<CloudRequest> {
        Ok(CloudRequest {
            url: self.endpoint.url("/audio/transcriptions")?,
            model: self.model.clone(),
            language: self.language.clone(),
            wav,
        })
    }
}

/// One piece, ready to send from a helper thread.
struct CloudRequest {
    url: String,
    model: String,
    language: String,
    wav: Vec<u8>,
}

impl Drop for CloudRequest {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.wav);
    }
}

/// A segment relative to its piece. `end_ms: None` = the whole piece (text-only answer).
struct PieceSegment {
    start_ms: i64,
    end_ms: Option<i64>,
    text: String,
}

struct PieceAnswer {
    segments: Vec<PieceSegment>,
    seconds: Option<f64>,
    cost: Option<f64>,
}

impl CloudRequest {
    /// Asks for segments (`verbose_json`); a model that refuses that (HTTP 400) is asked again
    /// for plain text, which becomes one segment over the piece.
    fn send(&self, ep: &Endpoint) -> Result<PieceAnswer> {
        match self.post(ep, true) {
            Err(Error::Llm(m)) if m.contains("HTTP 400") => self.post(ep, false),
            r => r,
        }
    }

    fn post(&self, ep: &Endpoint, verbose: bool) -> Result<PieceAnswer> {
        let format = if verbose { "verbose_json" } else { "json" };
        let req = ep.auth(ep.agent(Duration::from_secs(120)).post(&self.url));
        let resp = if ep.provider == ProviderId::Openrouter {
            // JSON with base64 audio, so the zero-retention routing rule can go along.
            use base64::Engine as _;
            let mut body = json!({
                "model": self.model,
                "input_audio": { "data": base64::engine::general_purpose::STANDARD.encode(&self.wav), "format": "wav" },
                "language": self.language,
                "response_format": format,
                "provider": { "zdr": true, "data_collection": "deny" },
            });
            if verbose {
                body["timestamp_granularities"] = json!(["segment"]);
            }
            req.header("Content-Type", "application/json").send(body.to_string())
        } else {
            let boundary = format!("thera-{}", uuid::Uuid::new_v4().simple());
            let mut body = Vec::with_capacity(self.wav.len() + 1024);
            for (name, value) in [("model", self.model.as_str()), ("language", &self.language), ("response_format", format)] {
                body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
            }
            body.extend(
                format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n")
                    .as_bytes(),
            );
            body.extend(&self.wav);
            body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
            req.header("Content-Type", format!("multipart/form-data; boundary={boundary}")).send(&body[..])
        };
        let mut resp = resp.map_err(crate::llm::net)?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().with_config().limit(16 << 20).read_to_string().map_err(crate::llm::net)?;
        if status != 200 {
            return Err(Error::Llm(crate::llm::api_error(status, &text)));
        }
        let v: Value = serde_json::from_str(&text)?;
        if v.get("error").is_some() {
            return Err(Error::Llm(crate::llm::api_error(status, &text)));
        }
        Ok(parse_cloud(&v))
    }
}

/// Reads `verbose_json` segments (seconds) or, without them, the plain `text`.
fn parse_cloud(v: &Value) -> PieceAnswer {
    let ms = |x: &Value| x.as_f64().map(|s| (s * 1000.0).round() as i64);
    let segments: Vec<PieceSegment> = match v.get("segments").and_then(Value::as_array).filter(|s| !s.is_empty()) {
        Some(list) => list
            .iter()
            .filter_map(|s| {
                Some(PieceSegment {
                    start_ms: ms(s.get("start")?)?.max(0),
                    end_ms: Some(ms(s.get("end")?)?),
                    text: s.get("text")?.as_str()?.trim().to_string(),
                })
            })
            .collect(),
        None => v
            .get("text")
            .and_then(Value::as_str)
            .map(|t| vec![PieceSegment { start_ms: 0, end_ms: None, text: t.trim().to_string() }])
            .unwrap_or_default(),
    };
    PieceAnswer {
        segments: segments.into_iter().filter(|s| !s.text.is_empty()).collect(),
        seconds: v.pointer("/usage/seconds").and_then(Value::as_f64).or_else(|| v.get("duration").and_then(Value::as_f64)),
        cost: v.pointer("/usage/cost").and_then(Value::as_f64),
    }
}

/// Splits a recording (given by its 100 ms envelope) into pieces of at most `max_ms`, each
/// ending at the quietest window of its last `search_ms`. Returns (from_ms, to_ms) pairs.
fn pieces(envelope: &[f32], max_ms: i64, search_ms: i64) -> Vec<(i64, i64)> {
    let total = envelope.len() as i64 * WINDOW_MS;
    let mut out = Vec::new();
    let mut from = 0;
    while total - from > max_ms {
        let hi = ((from + max_ms) / WINDOW_MS) as usize;
        let lo = ((from + max_ms - search_ms) / WINDOW_MS) as usize;
        // The quietest window; ties go to the latest, so pieces stay long.
        let cut = (lo..hi).rev().min_by(|&a, &b| envelope[a].total_cmp(&envelope[b])).unwrap_or(hi);
        let to = (cut as i64 * WINDOW_MS + WINDOW_MS / 2).max(from + WINDOW_MS);
        out.push((from, to));
        from = to;
    }
    out.push((from, total));
    out
}

/// Runs `f` on a helper thread and returns early with [`Error::Cancelled`] when `cancel` is set.
/// An abandoned request finishes or times out on its own; its answer is dropped.
fn cancellable<T: Send + 'static>(cancel: &AtomicBool, f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(r) => return r,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(Error::Stt("Transkription abgebrochen".into())),
        }
    }
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

/// Whether any 100 ms of `pcm` is above the silence gate.
pub(crate) fn has_speech(pcm: &[i16]) -> bool {
    loudness_envelope(pcm).iter().any(|&db| db > SILENCE_DBFS)
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

pub(crate) fn is_hallucination(text: &str) -> bool {
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
    fn session_recordings_go_to_the_cloud_only_with_consent() {
        use SttTarget::{Cloud, Local};
        for mode in [SttMode::Local, SttMode::Cloud, SttMode::Fallback] {
            let c = SttConfig { mode, ..Default::default() };
            for ready in [true, false] {
                assert_eq!(choose(&c, SourceKind::SessionAudio, ready, false, None).unwrap(), Local, "{mode:?} {ready}");
            }
            // Asking for the cloud in the session doesn't get around the missing consent.
            let err = choose(&c, SourceKind::SessionAudio, true, false, Some(Cloud)).unwrap_err().to_string();
            assert!(err.contains("Einwilligung"), "{err}");
            assert_eq!(choose(&c, SourceKind::SessionAudio, true, true, Some(Cloud)).unwrap(), Cloud);
            assert_eq!(choose(&c, SourceKind::Dictation, false, false, Some(Local)).unwrap(), Local, "explicit local wins");
        }
        let c = |mode| SttConfig { mode, ..Default::default() };
        assert_eq!(choose(&c(SttMode::Cloud), SourceKind::SessionAudio, true, true, None).unwrap(), Cloud);
        assert_eq!(choose(&c(SttMode::Local), SourceKind::Dictation, false, false, None).unwrap(), Local);
        assert_eq!(choose(&c(SttMode::Cloud), SourceKind::Dictation, true, false, None).unwrap(), Cloud);
        assert_eq!(choose(&c(SttMode::Fallback), SourceKind::Dictation, true, false, None).unwrap(), Local);
        assert_eq!(choose(&c(SttMode::Fallback), SourceKind::Dictation, false, false, None).unwrap(), Cloud);
        assert_eq!(choose(&c(SttMode::Fallback), SourceKind::SessionAudio, false, true, None).unwrap(), Cloud);
    }

    #[test]
    fn config_defaults_and_validation() {
        let c: SttConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(c, SttConfig::default());
        assert_eq!(c.model().as_deref(), Some(CLOUD_DEFAULT_MODEL));
        let custom = SttConfig { cloud_provider: ProviderId::Custom, ..Default::default() };
        assert_eq!(custom.model(), None, "a custom endpoint names its model");
        assert!(SttConfig { cloud_provider: ProviderId::Local, ..Default::default() }.validate().is_err());
        assert!(SttConfig { cloud_provider: ProviderId::Requesty, ..Default::default() }.validate().is_err());
        assert!(CLOUD_MODELS.iter().any(|m| m.id == CLOUD_DEFAULT_MODEL));
        assert!(CLOUD_MODELS.iter().all(|m| !m.id.starts_with("mistralai/")));
    }

    #[test]
    fn pieces_end_at_the_quietest_window() {
        // 10 s of speech with one quiet 100 ms window at 7.3 s; pieces of at most 8 s.
        let mut env = vec![-20.0f32; 100];
        env[73] = -70.0;
        assert_eq!(pieces(&env, 8_000, 2_000), vec![(0, 7_350), (7_350, 10_000)]);
        // No quiet window: still at most `max_ms`, cut inside the search range.
        let p = pieces(&vec![-20.0f32; 100], 8_000, 2_000);
        assert!(p[0].1 <= 8_000 && p[0].1 >= 6_000, "{p:?}");
        assert_eq!(pieces(&vec![-20.0f32; 30], 8_000, 2_000), vec![(0, 3_000)], "short = one piece");
    }

    #[test]
    fn cloud_answers_with_and_without_segments() {
        let a = parse_cloud(&json!({"segments": [{"start": 0.5, "end": 1.25, "text": " Hallo "}, {"start": 2, "end": 3, "text": " "}], "duration": 3.0}));
        assert_eq!((a.segments.len(), a.segments[0].start_ms, a.segments[0].end_ms, a.seconds), (1, 500, Some(1250), Some(3.0)));
        let b = parse_cloud(&json!({"text": "Nur Text", "usage": {"seconds": 4.0, "cost": 0.001}}));
        assert_eq!((b.segments[0].end_ms, b.seconds, b.cost), (None, Some(4.0), Some(0.001)));
        assert!(parse_cloud(&json!({})).segments.is_empty());
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
