//! Near-live dictation (Phase 3, slice 24): the therapist's own voice, transcribed piece by piece
//! while she speaks, for the editor's voice mode and the Diktat-Werkstatt.
//!
//! The microphone feeds a [`Splitter`], which cuts utterance-sized pieces at pauses (about 600 ms
//! of silence). Each piece is transcribed on its own, locally by a `whisper-server` sidecar that
//! keeps the speech model loaded, or in the cloud (a dictation is the therapist alone, so the
//! cloud is allowed, see [`crate::stt::cloud_allowed`]). Audio never touches the disk: pieces
//! live in memory and are wiped once transcribed.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::Value;

use crate::audio;
use crate::stt::{self, CloudStt, CloudUsage};
use crate::{Error, Result};

/// Level windows of 100 ms, like the silence gate in [`crate::stt`].
pub const WINDOW_MS: u64 = 100;
const WINDOW_SAMPLES: usize = audio::SAMPLE_RATE as usize * WINDOW_MS as usize / 1000;
/// A pause this long ends a piece.
pub const END_SILENCE_MS: u64 = 600;
/// A piece shorter than this ends only at a longer pause: whisper does badly on fragments (a
/// comma in a reverberant room is easily 600 ms "quiet").
const SHORT_PIECE_MS: u64 = 4_000;
const SHORT_END_SILENCE_MS: u64 = 1_000;
/// Less voiced time than this is a click or a cough, not speech.
const MIN_SPEECH_MS: u64 = 300;
/// Audio kept from before the first voiced window, so soft word onsets aren't cut.
const PRE_ROLL_MS: u64 = 500;
/// Trailing silence kept after the last voiced window.
const TAIL_MS: u64 = 200;
/// Without a pause a piece is cut at its quietest window within the last [`CUT_SEARCH_MS`].
pub const MAX_PIECE_MS: u64 = 20_000;
const CUT_SEARCH_MS: u64 = 5_000;
/// Never speech below this, however quiet the room.
const SPEECH_FLOOR_DBFS: f32 = -50.0;
/// A piece starts this much above the tracked noise floor…
const SPEECH_MARGIN_DB: f32 = 12.0;
/// …and keeps going while sound stays this much above it (soft consonants, reverb tails).
const CONTINUE_MARGIN_DB: f32 = 6.0;

/// One utterance, mono 16 kHz. Wiped when dropped.
pub struct Piece {
    pub seq: u32,
    /// Offset from the start of the dictation.
    pub start_ms: u64,
    pub pcm: Vec<i16>,
}

impl Piece {
    pub fn duration_ms(&self) -> u64 {
        self.pcm.len() as u64 * 1000 / audio::SAMPLE_RATE as u64
    }
}

impl Drop for Piece {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.pcm);
    }
}

struct Current {
    start_ms: u64,
    pcm: Vec<i16>,
    /// dBFS per window of `pcm`.
    levels: Vec<f32>,
    voiced_ms: u64,
    /// Windows since the last voiced one.
    silent: u64,
}

/// Cuts a stream of 16 kHz samples into utterances at pauses (a level-based voice activity
/// detector with a slowly adapting noise floor).
pub struct Splitter {
    partial: Vec<i16>,
    pos_ms: u64,
    noise: f32,
    pre: VecDeque<Vec<i16>>,
    current: Option<Current>,
    seq: u32,
}

impl Default for Splitter {
    fn default() -> Self {
        Self::new()
    }
}

impl Splitter {
    pub fn new() -> Self {
        Self { partial: Vec::with_capacity(WINDOW_SAMPLES), pos_ms: 0, noise: -60.0, pre: VecDeque::new(), current: None, seq: 0 }
    }

    /// Whether an utterance is in progress.
    pub fn speaking(&self) -> bool {
        self.current.is_some()
    }

    /// Feeds samples; returns the pieces they completed.
    pub fn push(&mut self, pcm: &[i16]) -> Vec<Piece> {
        let mut out = Vec::new();
        let mut rest = pcm;
        while !rest.is_empty() {
            let take = (WINDOW_SAMPLES - self.partial.len()).min(rest.len());
            self.partial.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.partial.len() == WINDOW_SAMPLES {
                let window = std::mem::replace(&mut self.partial, Vec::with_capacity(WINDOW_SAMPLES));
                out.extend(self.window(window));
            }
        }
        out
    }

    /// Ends the dictation: the utterance in progress, if it holds speech.
    pub fn finish(&mut self) -> Option<Piece> {
        let partial = std::mem::take(&mut self.partial);
        if let Some(cur) = self.current.as_mut() {
            cur.pcm.extend_from_slice(&partial);
        }
        for mut w in self.pre.drain(..) {
            zeroize::Zeroize::zeroize(&mut w);
        }
        let cur = self.current.take()?;
        self.emit(cur)
    }

    fn window(&mut self, window: Vec<i16>) -> Option<Piece> {
        let level = dbfs(&window);
        let voiced = level > SPEECH_FLOOR_DBFS.max(self.noise + SPEECH_MARGIN_DB);
        // The floor follows quiet stretches quickly and rises slowly while voiced (time constant
        // ~7 s), so a fan switched on mid-dictation stops counting as speech after a few seconds,
        // while the short pauses of real speech keep pulling it back down.
        let rate = if level < self.noise { 0.3 } else if voiced { 0.015 } else { 0.05 };
        self.noise += (level - self.noise) * rate;
        self.pos_ms += WINDOW_MS;

        let Some(cur) = self.current.as_mut() else {
            if voiced {
                let pre: Vec<i16> = self.pre.drain(..).flatten().collect();
                let pre_windows = pre.len() / WINDOW_SAMPLES;
                let mut pcm = pre;
                pcm.extend_from_slice(&window);
                let mut levels = vec![SPEECH_FLOOR_DBFS - 10.0; pre_windows];
                levels.push(level);
                self.current = Some(Current {
                    start_ms: self.pos_ms - WINDOW_MS * (pre_windows as u64 + 1),
                    pcm,
                    levels,
                    voiced_ms: WINDOW_MS,
                    silent: 0,
                });
            } else {
                self.pre.push_back(window);
                while self.pre.len() as u64 * WINDOW_MS > PRE_ROLL_MS {
                    if let Some(mut w) = self.pre.pop_front() {
                        zeroize::Zeroize::zeroize(&mut w);
                    }
                }
            }
            return None;
        };
        cur.pcm.extend_from_slice(&window);
        cur.levels.push(level);
        if voiced {
            cur.voiced_ms += WINDOW_MS;
        }
        if level > SPEECH_FLOOR_DBFS.max(self.noise + CONTINUE_MARGIN_DB) {
            cur.silent = 0;
        } else {
            cur.silent += 1;
        }
        let spoken_ms = (cur.levels.len() as u64 - cur.silent) * WINDOW_MS;
        let end_after = if spoken_ms < SHORT_PIECE_MS { SHORT_END_SILENCE_MS } else { END_SILENCE_MS };
        if cur.silent * WINDOW_MS >= end_after {
            let cur = self.current.take().expect("in a piece");
            return self.emit(cur);
        }
        if cur.levels.len() as u64 * WINDOW_MS >= MAX_PIECE_MS {
            // Cut at the quietest window of the last few seconds; the rest starts the next piece.
            let n = cur.levels.len();
            let lo = n - (CUT_SEARCH_MS / WINDOW_MS) as usize;
            let cut = (lo..n).rev().min_by(|&a, &b| cur.levels[a].total_cmp(&cur.levels[b])).unwrap_or(n - 1) + 1;
            let rest_pcm = cur.pcm.split_off(cut * WINDOW_SAMPLES);
            let rest_levels = cur.levels.split_off(cut);
            let rest = Current {
                start_ms: cur.start_ms + cut as u64 * WINDOW_MS,
                voiced_ms: rest_levels.iter().filter(|&&l| l > SPEECH_FLOOR_DBFS).count() as u64 * WINDOW_MS,
                silent: 0,
                pcm: rest_pcm,
                levels: rest_levels,
            };
            let mut head = self.current.replace(rest).expect("in a piece");
            head.silent = 0;
            return self.emit(head);
        }
        None
    }

    fn emit(&mut self, mut cur: Current) -> Option<Piece> {
        if cur.voiced_ms < MIN_SPEECH_MS {
            zeroize::Zeroize::zeroize(&mut cur.pcm);
            return None;
        }
        // Keep a short tail of the closing pause.
        let keep_windows = (cur.silent.saturating_sub(TAIL_MS / WINDOW_MS)) as usize;
        let len = cur.pcm.len().saturating_sub(keep_windows * WINDOW_SAMPLES);
        let mut dropped = cur.pcm.split_off(len);
        zeroize::Zeroize::zeroize(&mut dropped);
        self.seq += 1;
        Some(Piece { seq: self.seq, start_ms: cur.start_ms, pcm: std::mem::take(&mut cur.pcm) })
    }
}

fn dbfs(pcm: &[i16]) -> f32 {
    if pcm.is_empty() {
        return -120.0;
    }
    let mean_sq = pcm.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / pcm.len() as f64;
    (20.0 * (mean_sq.sqrt() / 32768.0).max(1e-9).log10()) as f32
}

// ---------- engines ----------

/// Where the pieces are transcribed.
pub enum LiveEngine<'a> {
    Local(&'a WhisperServer),
    Cloud(&'a CloudStt),
}

/// Transcribes one piece. `previous`: the text of the piece before (local engine only, as
/// whisper's prompt, so casing and punctuation carry over). `Ok(None)`: nothing was said.
pub fn transcribe_piece(engine: &LiveEngine, piece: &Piece, previous: Option<&str>, cancel: &AtomicBool) -> Result<(Option<String>, Option<CloudUsage>)> {
    if !stt::has_speech(&piece.pcm) {
        return Ok((None, None));
    }
    let (raw, usage) = match engine {
        LiveEngine::Local(server) => (server.transcribe(&piece.pcm, previous)?, None),
        LiveEngine::Cloud(cloud) => {
            let (text, usage) = cloud.transcribe_piece(&piece.pcm, cancel)?;
            (text, Some(usage))
        }
    };
    Ok((clean_text(&raw), usage))
}

// Whisper's non-speech markers: "[BLANK_AUDIO]", "(Musik)", "*Husten*".
static MARKER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[^\]]*\]|\([^)]*\)|\*[^*]*\*").expect("valid regex"));

/// The recognized text without non-speech markers and known hallucinations; `None` if nothing is left.
pub fn clean_text(raw: &str) -> Option<String> {
    let text = MARKER.replace_all(raw, " ");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() || !text.chars().any(char::is_alphanumeric) || stt::is_hallucination(&text) {
        return None;
    }
    Some(without_repeats(&text))
}

static SENTENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^.!?]+[.!?]*\s*").expect("valid regex"));

/// Whisper sometimes says a short piece twice ("Sie kam. Sie kam."); a sentence repeating the one
/// before it is dropped.
fn without_repeats(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for s in SENTENCE.find_iter(text).map(|m| m.as_str().trim()) {
        if out.last() != Some(&s) {
            out.push(s);
        }
    }
    out.join(" ")
}

/// Encoder frames beyond the piece's length (see [`WhisperServer::transcribe`]).
const AUDIO_CTX_MARGIN: usize = 512;

/// Debug builds take `THERA_AUDIO_CTX_MARGIN` (latency/quality measurements).
fn audio_ctx_margin() -> usize {
    #[cfg(debug_assertions)]
    if let Some(m) = std::env::var("THERA_AUDIO_CTX_MARGIN").ok().and_then(|v| v.parse().ok()) {
        return m;
    }
    AUDIO_CTX_MARGIN
}

/// whisper.cpp's `whisper-server`: the speech model stays loaded between pieces, so a piece is
/// back within a second or two. Audio is posted from memory (no `--convert`, so it writes no
/// temporary file). Listens on a free loopback port only.
pub struct WhisperServer {
    child: Child,
    port: u16,
    pub model_id: String,
    pub language: String,
    log: Arc<Mutex<VecDeque<String>>>,
}

impl WhisperServer {
    pub fn spawn(exe: &Path, model_path: &Path, model_id: &str, language: &str, threads: usize) -> Result<Self> {
        if !model_path.exists() {
            return Err(Error::Stt("Kein Sprachmodell installiert (Einstellungen → Transkription)".into()));
        }
        let port = TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port();
        let mut cmd = Command::new(exe);
        cmd.arg("-m").arg(model_path)
            .arg("-l").arg(language)
            .arg("-t").arg(threads.to_string())
            .arg("--host").arg("127.0.0.1")
            .arg("--port").arg(port.to_string())
            .arg("-nt")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| Error::Stt(format!("Diktat-Erkennung startet nicht: {e}")))?;
        let log = Arc::new(Mutex::new(VecDeque::new()));
        let stderr = child.stderr.take().expect("piped");
        let sink = log.clone();
        // Drains the pipe for the life of the process; a full pipe would block the server.
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(std::io::Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(mut l) = sink.lock() {
                    l.push_back(line);
                    if l.len() > 8 {
                        l.pop_front();
                    }
                }
            }
        });
        Ok(Self { child, port, model_id: model_id.to_string(), language: language.to_string(), log })
    }

    pub fn child(&self) -> &Child {
        &self.child
    }

    /// Waits until the model is loaded (the server listens only then).
    pub fn wait_ready(&mut self, timeout: Duration, cancelled: impl Fn() -> bool) -> Result<()> {
        let started = Instant::now();
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        loop {
            if cancelled() {
                self.kill();
                return Err(Error::Cancelled);
            }
            if let Some(status) = self.child.try_wait()? {
                return Err(Error::Stt(format!("Diktat-Erkennung beendet ({status}): {}", self.last_log())));
            }
            if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
                return Ok(());
            }
            if started.elapsed() > timeout {
                self.kill();
                return Err(Error::Stt("Diktat-Erkennung lädt zu lange".into()));
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn last_log(&self) -> String {
        self.log.lock().map(|l| l.iter().cloned().collect::<Vec<_>>().join(" | ")).unwrap_or_default().chars().take(300).collect()
    }

    /// Posts one piece as WAV and returns the raw text.
    pub fn transcribe(&self, pcm: &[i16], prompt: Option<&str>) -> Result<String> {
        let mut wav = audio::wav_bytes(pcm);
        let boundary = format!("thera-{}", uuid::Uuid::new_v4().simple());
        let mut body = Vec::with_capacity(wav.len() + 1024);
        // Whisper encodes 30 s windows; a piece is a few seconds, so the encoder gets only the
        // frames it needs (50 per second, plus a margin). This cuts the time per piece several-fold.
        let audio_ctx = (pcm.len() * 50 / audio::SAMPLE_RATE as usize + audio_ctx_margin()).min(1500).to_string();
        let mut fields = vec![("temperature", "0.0"), ("response_format", "json"), ("audio_ctx", audio_ctx.as_str())];
        if let Some(p) = prompt.filter(|p| !p.trim().is_empty()) {
            fields.push(("prompt", p));
        }
        for (name, value) in fields {
            body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
        }
        body.extend(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"piece.wav\"\r\nContent-Type: audio/wav\r\n\r\n")
                .as_bytes(),
        );
        body.extend(&wav);
        zeroize::Zeroize::zeroize(&mut wav);
        body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
        let agent: ureq::Agent =
            ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(120))).http_status_as_error(false).build().into();
        let result = agent
            .post(&format!("http://127.0.0.1:{}/inference", self.port))
            .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
            .send(&body[..]);
        zeroize::Zeroize::zeroize(&mut body);
        let mut resp = result.map_err(|e| Error::Stt(format!("Diktat-Erkennung nicht erreichbar: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().map_err(|e| Error::Stt(e.to_string()))?;
        if status != 200 {
            return Err(Error::Stt(format!("Diktat-Erkennung: HTTP {status}")));
        }
        parse_server(&text)
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for WhisperServer {
    fn drop(&mut self) {
        self.kill();
    }
}

/// `{"text": "…"}`, or `{"error": "…"}`.
fn parse_server(body: &str) -> Result<String> {
    let v: Value = serde_json::from_str(body).map_err(|e| Error::Stt(format!("unerwartete Antwort der Diktat-Erkennung: {e}")))?;
    if let Some(e) = v.get("error") {
        return Err(Error::Stt(format!("Diktat-Erkennung: {}", e.as_str().unwrap_or("Fehler"))));
    }
    Ok(v.get("text").and_then(Value::as_str).unwrap_or_default().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: usize = audio::SAMPLE_RATE as usize;

    fn silence(ms: usize) -> Vec<i16> {
        // Faint room noise around -66 dBFS.
        (0..ms * SR / 1000).map(|i| if i % 2 == 0 { 15 } else { -15 }).collect()
    }

    fn speech(ms: usize) -> Vec<i16> {
        (0..ms * SR / 1000).map(|i| ((i as f32 / SR as f32 * 220.0 * std::f32::consts::TAU).sin() * 6_000.0) as i16).collect()
    }

    fn run(parts: &[Vec<i16>], block: usize) -> (Vec<(u32, u64, u64)>, Splitter) {
        let all: Vec<i16> = parts.concat();
        let mut s = Splitter::new();
        let mut out: Vec<(u32, u64, u64)> = Vec::new();
        for b in all.chunks(block) {
            out.extend(s.push(b).iter().map(|p| (p.seq, p.start_ms, p.duration_ms())));
        }
        (out, s)
    }

    #[test]
    fn pauses_end_pieces_and_short_blips_are_ignored() {
        let (pieces, mut s) = run(
            &[silence(1_000), speech(2_000), silence(1_200), speech(80), silence(1_200), speech(1_500), silence(300)],
            997,
        );
        // The first (short) piece ends 1 s into the pause; the 80 ms click makes no piece.
        assert_eq!(pieces.len(), 1, "{pieces:?}");
        let (seq, start, dur) = pieces[0];
        assert_eq!(seq, 1);
        assert_eq!(start, 500, "500 ms pre-roll before the speech at 1 s");
        // pre-roll + speech + 200 ms tail
        assert_eq!(dur, 500 + 2_000 + 200);
        assert!(s.speaking(), "the last utterance is still open (300 ms pause < 600 ms)");
        let last = s.finish().unwrap();
        assert_eq!(last.seq, 2);
        assert!(last.duration_ms() >= 1_500 && last.duration_ms() <= 2_300, "{}", last.duration_ms());
        assert!(!s.speaking());
        assert!(s.finish().is_none());
    }

    #[test]
    fn a_short_phrase_waits_for_a_longer_pause_a_long_one_does_not() {
        // 2.5 s, a 700 ms comma pause, 1.5 s: one piece.
        let (pieces, mut s) = run(&[silence(500), speech(2_500), silence(700), speech(1_500), silence(1_100)], 1_600);
        assert_eq!(pieces.len(), 1, "{pieces:?}");
        assert!(pieces[0].2 >= 4_700, "{pieces:?}");
        assert!(s.finish().is_none());
        // 5 s of speech, then 700 ms: ended.
        let (pieces, s) = run(&[silence(500), speech(5_000), silence(700)], 1_600);
        assert_eq!(pieces.len(), 1, "{pieces:?}");
        assert!(!s.speaking());
    }

    #[test]
    fn long_speech_is_cut_at_a_quiet_spot() {
        // Phrases of 2.75 s with 250 ms breaths (too short to end a piece), 27 s in all.
        let mut parts = vec![silence(500)];
        for _ in 0..9 {
            parts.push(speech(2_750));
            parts.push(silence(250));
        }
        let (pieces, mut s) = run(&parts, 1_600);
        assert_eq!(pieces.len(), 1, "{pieces:?}");
        let (_, start, dur) = pieces[0];
        assert_eq!(start, 0);
        assert!((15_000..=MAX_PIECE_MS).contains(&dur), "{dur}");
        // The cut lands in a breath.
        let r = (start + dur - 500) % 3_000;
        assert!(r == 0 || r > 2_750, "cut at {}", start + dur);
        let rest = s.finish().unwrap();
        assert!(rest.duration_ms() >= 5_000, "{}", rest.duration_ms());
    }

    #[test]
    fn steady_noise_becomes_the_floor() {
        // A fan at about -35 dBFS for 15 s: after a few seconds it is not speech any more.
        let fan: Vec<i16> = (0..15 * SR).map(|i| if i % 2 == 0 { 580 } else { -580 }).collect();
        let (pieces, mut s) = run(&[fan], 1_600);
        assert_eq!(pieces.len(), 1, "{pieces:?}");
        assert!(pieces[0].2 <= 11_000, "fan counted as {} ms of speech", pieces[0].2);
        assert!(s.finish().is_none());
    }

    #[test]
    fn clean_text_drops_markers_and_hallucinations() {
        assert_eq!(clean_text(" [BLANK_AUDIO] "), None);
        assert_eq!(clean_text("(Musik)"), None);
        assert_eq!(clean_text("Untertitel im Auftrag des ZDF, 2021"), None);
        assert_eq!(clean_text("  Pat. berichtet *räuspert sich* von  Schlafproblemen. ").as_deref(), Some("Pat. berichtet von Schlafproblemen."));
        assert_eq!(clean_text(" ... "), None);
        assert_eq!(
            clean_text("Die Woche war belastend. Die Woche war belastend. Sie schläft schlecht. Ja. Ja!").as_deref(),
            Some("Die Woche war belastend. Sie schläft schlecht. Ja. Ja!")
        );
        assert_eq!(clean_text("ohne Punkt am Ende").as_deref(), Some("ohne Punkt am Ende"));
    }

    #[test]
    fn server_answers() {
        assert_eq!(parse_server(r#"{"text":" Hallo Welt.\n"}"#).unwrap(), " Hallo Welt.\n");
        assert!(parse_server(r#"{"error":"failed to read audio"}"#).is_err());
    }
}
