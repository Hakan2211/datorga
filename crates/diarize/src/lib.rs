//! Who speaks when in a session recording, and what a voice sounds like (a speaker embedding),
//! through the C API of sherpa-onnx (pyannote segmentation + a speaker embedding model).
//!
//! The library is loaded at runtime from a folder next to the app, so building needs nothing
//! from sherpa-onnx. It runs in its own process (the app starts itself with `--diarize`): the
//! diarization call can't be interrupted, so cancelling means killing that process.
//!
//! Protocol: the caller writes the audio as raw 16 kHz mono i16 (little-endian) and a JSON
//! [`Request`], runs [`main`] with the request's path, reads progress lines (`progress N`) from
//! stderr and the JSON [`Output`] from `request.out`.

use std::ffi::{c_char, c_void, CString};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const SAMPLE_RATE: i32 = 16_000;
/// The library the app loads; it pulls in `onnxruntime.dll` from the same folder.
pub const LIBRARY: &str = if cfg!(windows) { "sherpa-onnx-c-api.dll" } else { "libsherpa-onnx-c-api.so" };

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub mode: Mode,
    /// Raw 16 kHz mono i16 samples, little-endian.
    pub pcm: PathBuf,
    /// Where the [`Output`] JSON goes.
    pub out: PathBuf,
    /// Folder holding [`LIBRARY`] and onnxruntime.
    pub lib_dir: PathBuf,
    pub segmentation: PathBuf,
    pub embedding: PathBuf,
    pub threads: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Mode {
    /// Turns per speaker plus one voice print per speaker. `speakers` = 0: the clustering
    /// threshold decides how many there are.
    Diarize { speakers: i32 },
    /// One voice print of the whole recording (a dictation: one speaker).
    Embed,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Output {
    /// Sorted by start.
    pub turns: Vec<Turn>,
    /// One per speaker that has turns (embed mode: one, speaker 0).
    pub speakers: Vec<Print>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: i32,
}

/// A speaker's voice: the duration-weighted mean of unit-length embeddings, itself unit length.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Print {
    pub speaker: i32,
    pub embedding: Vec<f32>,
    /// Speaking time in seconds (all turns, not just the ones embedded).
    pub seconds: f32,
}

/// Entry point of the `--diarize <request.json>` process. Returns the exit code.
pub fn main(request: &Path) -> i32 {
    let result = std::fs::read(request)
        .map_err(|e| format!("request: {e}"))
        .and_then(|b| serde_json::from_slice::<Request>(&b).map_err(|e| format!("request: {e}")))
        .and_then(|req| {
            let out = run(&req, |p| eprintln!("progress {p}"))?;
            let json = serde_json::to_vec(&out).map_err(|e| e.to_string())?;
            std::fs::write(&req.out, json).map_err(|e| format!("output: {e}"))
        });
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

pub fn run(req: &Request, mut progress: impl FnMut(u8)) -> Result<Output, String> {
    let bytes = std::fs::read(&req.pcm).map_err(|e| format!("audio: {e}"))?;
    let samples: Vec<f32> = bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
    drop(bytes);
    if samples.len() < SAMPLE_RATE as usize {
        return Err("Die Aufnahme ist kürzer als eine Sekunde".into());
    }
    let api = Api::load(&req.lib_dir)?;
    let threads = req.threads.max(1);
    let out = match req.mode {
        Mode::Diarize { speakers } => {
            let turns = diarize(&api, req, threads, speakers, &samples, &mut |p| progress((p as u32 * 85 / 100) as u8))?;
            let ex = Extractor::new(&api, &req.embedding, threads)?;
            let speakers = speaker_prints(&ex, &samples, &turns, &mut |p| progress((85 + p as u32 * 15 / 100) as u8))?;
            Output { turns, speakers }
        }
        Mode::Embed => {
            let ex = Extractor::new(&api, &req.embedding, threads)?;
            let windows = voiced_windows(&samples);
            let seconds = windows.iter().map(|(a, b)| (b - a) as f32).sum::<f32>() / SAMPLE_RATE as f32;
            let embedding = ex.mean(&samples, &windows, &mut progress)?;
            Output { turns: Vec::new(), speakers: embedding.map(|e| Print { speaker: 0, embedding: e, seconds }).into_iter().collect() }
        }
    };
    progress(100);
    Ok(out)
}

// ---------- voice prints ----------

/// Longest piece of a turn that goes into one embedding.
const MAX_PIECE: usize = 15 * SAMPLE_RATE as usize;
/// Shorter turns say little about a voice; they're used only when a speaker has nothing longer.
const MIN_PIECE: usize = SAMPLE_RATE as usize;
/// Speaking time per speaker that goes into its print (the longest turns first).
const PRINT_BUDGET: usize = 90 * SAMPLE_RATE as usize;

fn speaker_prints(ex: &Extractor, samples: &[f32], turns: &[Turn], progress: &mut dyn FnMut(u8)) -> Result<Vec<Print>, String> {
    let mut ids: Vec<i32> = turns.iter().map(|t| t.speaker).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut prints = Vec::new();
    for (n, &id) in ids.iter().enumerate() {
        let mut pieces: Vec<(usize, usize)> = turns
            .iter()
            .filter(|t| t.speaker == id)
            .map(|t| (ms_to_sample(t.start_ms).min(samples.len()), ms_to_sample(t.end_ms).min(samples.len())))
            .filter(|(a, b)| b > a)
            .collect();
        let seconds = pieces.iter().map(|(a, b)| (b - a) as f32).sum::<f32>() / SAMPLE_RATE as f32;
        pieces.sort_by_key(|(a, b)| std::cmp::Reverse(b - a));
        if pieces.iter().any(|(a, b)| b - a >= MIN_PIECE) {
            pieces.retain(|(a, b)| b - a >= MIN_PIECE);
        }
        let (mut chosen, mut total) = (Vec::new(), 0);
        for (a, b) in pieces {
            if total >= PRINT_BUDGET {
                break;
            }
            let b = b.min(a + MAX_PIECE);
            total += b - a;
            chosen.push((a, b));
        }
        if let Some(embedding) = ex.mean(samples, &chosen, &mut |_| {})? {
            prints.push(Print { speaker: id, embedding, seconds });
        }
        progress(((n + 1) * 100 / ids.len()) as u8);
    }
    Ok(prints)
}

/// 4-second windows loud enough to be speech (a dictation's pauses would dilute the print), at
/// most 120 s of them.
fn voiced_windows(samples: &[f32]) -> Vec<(usize, usize)> {
    let len = 4 * SAMPLE_RATE as usize;
    let windows: Vec<(usize, usize, f32)> = (0..samples.len())
        .step_by(len)
        .map(|a| {
            let b = (a + len).min(samples.len());
            let rms = (samples[a..b].iter().map(|s| s * s).sum::<f32>() / (b - a) as f32).sqrt();
            (a, b, rms)
        })
        .filter(|(a, b, _)| b - a >= MIN_PIECE)
        .collect();
    let mut levels: Vec<f32> = windows.iter().map(|w| w.2).collect();
    levels.sort_by(f32::total_cmp);
    let Some(&loud) = levels.get(levels.len() * 3 / 4) else { return Vec::new() };
    windows.into_iter().filter(|w| w.2 >= loud * 0.4 && w.2 > 0.003).map(|(a, b, _)| (a, b)).take(30).collect()
}

fn ms_to_sample(ms: i64) -> usize {
    (ms.max(0) as usize) * (SAMPLE_RATE as usize / 1000)
}

/// Unit-length copy (cosine similarity is then a dot product).
pub fn normalize(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 { v.iter().map(|x| x / n).collect() } else { v.to_vec() }
}

// ---------- sherpa-onnx ----------

fn diarize(api: &Api, req: &Request, threads: i32, speakers: i32, samples: &[f32], progress: &mut dyn FnMut(u8)) -> Result<Vec<Turn>, String> {
    let seg_model = cstr(&req.segmentation)?;
    let emb_model = cstr(&req.embedding)?;
    let cpu = CString::new("cpu").expect("no NUL");
    let config = DiarizationConfig {
        segmentation: SegmentationConfig {
            pyannote: PyannoteConfig { model: seg_model.as_ptr(), window_shift_ratio: 0.0 },
            num_threads: threads,
            debug: 0,
            provider: cpu.as_ptr(),
        },
        embedding: EmbeddingConfig { model: emb_model.as_ptr(), num_threads: threads, debug: 0, provider: cpu.as_ptr() },
        clustering: ClusteringConfig { num_clusters: speakers, threshold: 0.5, compute_confidence: 0 },
        min_duration_on: 0.3,
        min_duration_off: 0.5,
    };
    // SAFETY: the config's strings outlive the call; the handle is destroyed below.
    let sd = unsafe { (api.create_diarization)(&config) };
    if sd.is_null() {
        return Err("Sprechertrennung startet nicht (Modelldateien prüfen)".into());
    }
    let mut report = |done: i32, total: i32| {
        if total > 0 {
            progress((done.clamp(0, total) * 100 / total) as u8);
        }
    };
    let mut report: &mut dyn FnMut(i32, i32) = &mut report;
    // SAFETY: `samples` lives through the call; the callback gets a pointer to `report`.
    let result = unsafe {
        (api.process)(sd, samples.as_ptr(), samples.len() as i32, progress_trampoline, &mut report as *mut _ as *mut c_void)
    };
    let mut turns = Vec::new();
    if !result.is_null() {
        // SAFETY: the array has exactly `n` entries and is freed right after copying.
        unsafe {
            let n = (api.num_segments)(result).max(0) as usize;
            let segs = (api.sort_by_start)(result);
            if !segs.is_null() {
                for s in std::slice::from_raw_parts(segs, n) {
                    turns.push(Turn {
                        start_ms: (s.start as f64 * 1000.0).round() as i64,
                        end_ms: (s.end as f64 * 1000.0).round() as i64,
                        speaker: s.speaker,
                    });
                }
                (api.destroy_segments)(segs);
            }
            (api.destroy_result)(result);
        }
    }
    // SAFETY: created above, not used afterwards.
    unsafe { (api.destroy_diarization)(sd) };
    if result.is_null() {
        return Err("Sprechertrennung lieferte kein Ergebnis".into());
    }
    Ok(turns)
}

extern "C" fn progress_trampoline(done: i32, total: i32, arg: *mut c_void) -> i32 {
    // SAFETY: `arg` is the `&mut dyn FnMut` passed to `process` in `diarize`.
    let f = unsafe { &mut *(arg as *mut &mut dyn FnMut(i32, i32)) };
    f(done, total);
    0
}

struct Extractor<'a> {
    api: &'a Api,
    handle: *const c_void,
}

impl<'a> Extractor<'a> {
    fn new(api: &'a Api, model: &Path, threads: i32) -> Result<Self, String> {
        let model = cstr(model)?;
        let cpu = CString::new("cpu").expect("no NUL");
        let config = EmbeddingConfig { model: model.as_ptr(), num_threads: threads, debug: 0, provider: cpu.as_ptr() };
        // SAFETY: the strings outlive the call.
        let handle = unsafe { (api.create_extractor)(&config) };
        if handle.is_null() {
            return Err("Stimmerkennung startet nicht (Modelldatei prüfen)".into());
        }
        Ok(Self { api, handle })
    }

    /// Unit-length embedding of `samples`, or `None` when the model wants more audio.
    fn embed(&self, samples: &[f32]) -> Option<Vec<f32>> {
        let api = self.api;
        // SAFETY: the stream and the embedding are freed before returning; the embedding has
        // `dim` floats.
        unsafe {
            let stream = (api.create_stream)(self.handle);
            if stream.is_null() {
                return None;
            }
            (api.accept_waveform)(stream, SAMPLE_RATE, samples.as_ptr(), samples.len() as i32);
            (api.input_finished)(stream);
            let mut out = None;
            if (api.is_ready)(self.handle, stream) != 0 {
                let dim = (api.dim)(self.handle).max(0) as usize;
                let v = (api.compute)(self.handle, stream);
                if !v.is_null() {
                    out = Some(normalize(std::slice::from_raw_parts(v, dim)));
                    (api.destroy_embedding)(v);
                }
            }
            (api.destroy_stream)(stream);
            out
        }
    }

    /// Duration-weighted mean over `pieces` (sample ranges), unit length.
    fn mean(&self, samples: &[f32], pieces: &[(usize, usize)], progress: &mut dyn FnMut(u8)) -> Result<Option<Vec<f32>>, String> {
        let mut sum: Vec<f32> = Vec::new();
        for (i, &(a, b)) in pieces.iter().enumerate() {
            if let Some(e) = self.embed(&samples[a..b]) {
                if sum.is_empty() {
                    sum = vec![0.0; e.len()];
                }
                let w = (b - a) as f32 / SAMPLE_RATE as f32;
                sum.iter_mut().zip(&e).for_each(|(s, x)| *s += x * w);
            }
            progress(((i + 1) * 100 / pieces.len()) as u8);
        }
        Ok(if sum.is_empty() { None } else { Some(normalize(&sum)) })
    }
}

impl Drop for Extractor<'_> {
    fn drop(&mut self) {
        // SAFETY: created in `new`, dropped once.
        unsafe { (self.api.destroy_extractor)(self.handle) }
    }
}

fn cstr(p: &Path) -> Result<CString, String> {
    CString::new(p.to_string_lossy().as_bytes()).map_err(|_| format!("invalid path {}", p.display()))
}

// Layouts from sherpa-onnx v2.0.1 `c-api.h` (the pinned version; see scripts/fetch-sherpa.ps1).

#[repr(C)]
struct PyannoteConfig {
    model: *const c_char,
    window_shift_ratio: f32,
}

#[repr(C)]
struct SegmentationConfig {
    pyannote: PyannoteConfig,
    num_threads: i32,
    debug: i32,
    provider: *const c_char,
}

#[repr(C)]
struct EmbeddingConfig {
    model: *const c_char,
    num_threads: i32,
    debug: i32,
    provider: *const c_char,
}

#[repr(C)]
struct ClusteringConfig {
    num_clusters: i32,
    threshold: f32,
    compute_confidence: i32,
}

#[repr(C)]
struct DiarizationConfig {
    segmentation: SegmentationConfig,
    embedding: EmbeddingConfig,
    clustering: ClusteringConfig,
    min_duration_on: f32,
    min_duration_off: f32,
}

#[repr(C)]
struct RawSegment {
    start: f32,
    end: f32,
    speaker: i32,
    confidence: f32,
}

type Handle = *const c_void;
type ProgressFn = extern "C" fn(i32, i32, *mut c_void) -> i32;

struct Api {
    _lib: libloading::Library,
    create_diarization: unsafe extern "C" fn(*const DiarizationConfig) -> Handle,
    destroy_diarization: unsafe extern "C" fn(Handle),
    process: unsafe extern "C" fn(Handle, *const f32, i32, ProgressFn, *mut c_void) -> Handle,
    num_segments: unsafe extern "C" fn(Handle) -> i32,
    sort_by_start: unsafe extern "C" fn(Handle) -> *const RawSegment,
    destroy_segments: unsafe extern "C" fn(*const RawSegment),
    destroy_result: unsafe extern "C" fn(Handle),
    create_extractor: unsafe extern "C" fn(*const EmbeddingConfig) -> Handle,
    destroy_extractor: unsafe extern "C" fn(Handle),
    dim: unsafe extern "C" fn(Handle) -> i32,
    create_stream: unsafe extern "C" fn(Handle) -> Handle,
    accept_waveform: unsafe extern "C" fn(Handle, i32, *const f32, i32),
    input_finished: unsafe extern "C" fn(Handle),
    is_ready: unsafe extern "C" fn(Handle, Handle) -> i32,
    compute: unsafe extern "C" fn(Handle, Handle) -> *const f32,
    destroy_embedding: unsafe extern "C" fn(*const f32),
    destroy_stream: unsafe extern "C" fn(Handle),
}

impl Api {
    fn load(dir: &Path) -> Result<Self, String> {
        let path = std::path::absolute(dir.join(LIBRARY)).map_err(|e| e.to_string())?;
        // SAFETY: loading runs the library's initializers; it is the pinned sherpa-onnx build.
        let lib = unsafe { open(&path) }.map_err(|e| format!("{}: {e}", path.display()))?;
        macro_rules! sym {
            ($name:literal) => {
                // SAFETY: the signature matches the declaration in c-api.h.
                *unsafe { lib.get(concat!($name, "\0").as_bytes()) }.map_err(|e| format!("{}: {e}", $name))?
            };
        }
        Ok(Self {
            create_diarization: sym!("SherpaOnnxCreateOfflineSpeakerDiarization"),
            destroy_diarization: sym!("SherpaOnnxDestroyOfflineSpeakerDiarization"),
            process: sym!("SherpaOnnxOfflineSpeakerDiarizationProcessWithCallback"),
            num_segments: sym!("SherpaOnnxOfflineSpeakerDiarizationResultGetNumSegments"),
            sort_by_start: sym!("SherpaOnnxOfflineSpeakerDiarizationResultSortByStartTime"),
            destroy_segments: sym!("SherpaOnnxOfflineSpeakerDiarizationDestroySegment"),
            destroy_result: sym!("SherpaOnnxOfflineSpeakerDiarizationDestroyResult"),
            create_extractor: sym!("SherpaOnnxCreateSpeakerEmbeddingExtractor"),
            destroy_extractor: sym!("SherpaOnnxDestroySpeakerEmbeddingExtractor"),
            dim: sym!("SherpaOnnxSpeakerEmbeddingExtractorDim"),
            create_stream: sym!("SherpaOnnxSpeakerEmbeddingExtractorCreateStream"),
            accept_waveform: sym!("SherpaOnnxOnlineStreamAcceptWaveform"),
            input_finished: sym!("SherpaOnnxOnlineStreamInputFinished"),
            is_ready: sym!("SherpaOnnxSpeakerEmbeddingExtractorIsReady"),
            compute: sym!("SherpaOnnxSpeakerEmbeddingExtractorComputeEmbedding"),
            destroy_embedding: sym!("SherpaOnnxSpeakerEmbeddingExtractorDestroyEmbedding"),
            destroy_stream: sym!("SherpaOnnxDestroyOnlineStream"),
            _lib: lib,
        })
    }
}

/// Windows looks for onnxruntime.dll next to the library only with the altered search path.
#[cfg(windows)]
unsafe fn open(path: &Path) -> Result<libloading::Library, libloading::Error> {
    use libloading::os::windows::{Library, LOAD_WITH_ALTERED_SEARCH_PATH};
    Library::load_with_flags(path, LOAD_WITH_ALTERED_SEARCH_PATH).map(Into::into)
}

#[cfg(not(windows))]
unsafe fn open(path: &Path) -> Result<libloading::Library, libloading::Error> {
    libloading::Library::new(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips() {
        let req = Request {
            mode: Mode::Diarize { speakers: 2 },
            pcm: "a.pcm".into(),
            out: "o.json".into(),
            lib_dir: "lib".into(),
            segmentation: "s.onnx".into(),
            embedding: "e.onnx".into(),
            threads: 4,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""mode":{"kind":"diarize","speakers":2}"#), "{json}");
        let back: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(back.mode, req.mode);
        assert_eq!(serde_json::from_str::<Mode>(r#"{"kind":"embed"}"#).unwrap(), Mode::Embed);
    }

    #[test]
    fn voiced_windows_skip_silence() {
        let sr = SAMPLE_RATE as usize;
        let mut s = vec![0.0f32; 20 * sr];
        for (i, x) in s[4 * sr..12 * sr].iter_mut().enumerate() {
            *x = 0.2 * ((i as f32) * 0.05).sin();
        }
        assert_eq!(voiced_windows(&s), vec![(4 * sr, 8 * sr), (8 * sr, 12 * sr)]);
        assert!(voiced_windows(&vec![0.0; 10 * sr]).is_empty(), "digital silence gives nothing");
    }

    #[test]
    fn normalize_gives_unit_length() {
        let v = normalize(&[3.0, 4.0]);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        assert_eq!(normalize(&[0.0, 0.0]), vec![0.0, 0.0]);
    }

    /// Against the real library and models: `THERA_DIARIZE_DIR` = a folder laid out like
    /// `crates/app/diarize` (scripts/fetch-sherpa.ps1), `THERA_DIARIZE_PCM` = raw 16 kHz audio
    /// with two speakers.
    #[test]
    #[ignore]
    fn real_two_speakers() {
        let dir = PathBuf::from(std::env::var("THERA_DIARIZE_DIR").expect("THERA_DIARIZE_DIR"));
        let pcm = PathBuf::from(std::env::var("THERA_DIARIZE_PCM").expect("THERA_DIARIZE_PCM"));
        let req = Request {
            mode: Mode::Diarize { speakers: 2 },
            pcm,
            out: std::env::temp_dir().join("thera-diarize-test.json"),
            lib_dir: dir.clone(),
            segmentation: dir.join("segmentation.onnx"),
            embedding: std::env::var("THERA_DIARIZE_EMBEDDING").map_or(dir.join("embedding.onnx"), PathBuf::from),
            threads: 4,
        };
        let out = run(&req, |_| {}).unwrap();
        for t in &out.turns {
            eprintln!("{:>8} {:>8} {}", t.start_ms, t.end_ms, t.speaker);
        }
        assert_eq!(out.speakers.len(), 2);
        let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
        eprintln!("cross-speaker cosine {:.3}", dot(&out.speakers[0].embedding, &out.speakers[1].embedding));
        // Optional: a one-speaker recording (a dictation) to compare with both speakers.
        if let Ok(profile) = std::env::var("THERA_DIARIZE_PROFILE_PCM") {
            let p = run(&Request { mode: Mode::Embed, pcm: profile.into(), ..req }, |_| {}).unwrap();
            for s in &out.speakers {
                eprintln!("profile vs speaker {}: {:.3} ({:.0} s)", s.speaker, dot(&p.speakers[0].embedding, &s.embedding), s.seconds);
            }
        }
    }
}
