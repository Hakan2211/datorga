//! The sidecar runner against a fake `whisper-cli`: arguments, progress, cancel, cleanup.
//! `real_whisper` runs the actual binary when THERA_WHISPER_CLI / _MODEL / _WAV are set.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use thera_core::audio::{AudioKey, ChunkWriter};
use thera_core::stt::{self, WhisperCli};
use thera_core::Error;

fn recording(dir: &Path, seconds: usize) -> AudioKey {
    let key = AudioKey::generate().unwrap();
    let mut w = ChunkWriter::create(dir, &key).unwrap();
    w.write(&vec![300i16; seconds * 16_000]).unwrap();
    key
}

fn cli(model: &str) -> WhisperCli {
    WhisperCli {
        exe: env!("CARGO_BIN_EXE_thera-fake-whisper").into(),
        model: model.into(),
        language: "de".into(),
        threads: 2,
    }
}

#[test]
fn transcribes_reports_progress_and_removes_plaintext() {
    let tmp = tempfile::tempdir().unwrap();
    let key = recording(&tmp.path().join("audio"), 2);
    let work = tmp.path().join("work");
    let mut seen = vec![];
    let segs = stt::transcribe(&tmp.path().join("audio"), &key, &cli("model"), &work, |p| seen.push(p), &AtomicBool::new(false)).unwrap();
    assert_eq!(segs, vec![stt::Segment { start_ms: 0, end_ms: 2000, text: "Hallo aus dem Test.".into() }]);
    assert_eq!(seen, vec![0, 50, 100]);
    assert!(!work.exists(), "WAV and JSON are deleted");
}

#[test]
fn segments_over_silence_are_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let key = AudioKey::generate().unwrap();
    ChunkWriter::create(&tmp.path().join("audio"), &key).unwrap().write(&vec![0i16; 32_000]).unwrap();
    let segs = stt::transcribe(&tmp.path().join("audio"), &key, &cli("model"), &tmp.path().join("work"), |_| {}, &AtomicBool::new(false)).unwrap();
    assert!(segs.is_empty(), "the fake's text over digital silence is discarded");
}

#[test]
fn failure_and_missing_binary_still_clean_up() {
    let tmp = tempfile::tempdir().unwrap();
    let key = recording(&tmp.path().join("audio"), 1);
    let work = tmp.path().join("work");
    let err = stt::transcribe(&tmp.path().join("audio"), &key, &cli("fail"), &work, |_| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(err.to_string().contains("failed to load model"), "{err}");
    assert!(!work.exists());

    let mut missing = cli("model");
    missing.exe = tmp.path().join("nope.exe");
    assert!(matches!(stt::transcribe(&tmp.path().join("audio"), &key, &missing, &work, |_| {}, &AtomicBool::new(false)), Err(Error::Stt(_))));
    assert!(!work.exists());
}

#[test]
fn cancel_kills_the_sidecar() {
    let tmp = tempfile::tempdir().unwrap();
    let key = recording(&tmp.path().join("audio"), 1);
    let work = tmp.path().join("work");
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    let start = std::time::Instant::now();
    let err = stt::transcribe(&tmp.path().join("audio"), &key, &cli("slow"), &work, move |p| {
        if p == 50 {
            c.store(true, Ordering::Relaxed);
        }
    }, &cancel)
    .unwrap_err();
    assert!(matches!(err, Error::Cancelled));
    assert!(start.elapsed().as_secs() < 30, "did not wait for the sidecar");
    assert!(!work.exists());
}

#[test]
#[ignore = "needs THERA_WHISPER_CLI, THERA_WHISPER_MODEL and THERA_WHISPER_WAV (16 kHz mono s16 WAV)"]
fn real_whisper() {
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} not set"));
    let wav = std::fs::read(var("THERA_WHISPER_WAV")).unwrap();
    let pcm: Vec<i16> = wav[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    let tmp = tempfile::tempdir().unwrap();
    let key = AudioKey::generate().unwrap();
    ChunkWriter::create(&tmp.path().join("audio"), &key).unwrap().write(&pcm).unwrap();
    let cli = WhisperCli { exe: var("THERA_WHISPER_CLI").into(), model: var("THERA_WHISPER_MODEL").into(), language: "de".into(), threads: 8 };
    let t = std::time::Instant::now();
    let segs = stt::transcribe(&tmp.path().join("audio"), &key, &cli, &tmp.path().join("work"), |p| eprintln!("{p}%"), &AtomicBool::new(false)).unwrap();
    eprintln!("{:.1}s for {:.1}s audio", t.elapsed().as_secs_f32(), pcm.len() as f32 / 16_000.0);
    for s in &segs {
        eprintln!("[{} - {}] {}", s.start_ms, s.end_ms, s.text);
    }
    assert!(!segs.is_empty());
}
