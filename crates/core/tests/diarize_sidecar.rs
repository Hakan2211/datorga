//! The diarization runner against a fake `--diarize` process: request, progress, labels from a
//! voice profile, cancel, cleanup.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use thera_core::audio::{AudioKey, ChunkWriter};
use thera_core::diarize::{self, Sidecar};
use thera_core::stt::Segment;
use thera_core::Error;

fn recording(dir: &Path, seconds: usize) -> AudioKey {
    let key = AudioKey::generate().unwrap();
    let mut w = ChunkWriter::create(dir, &key).unwrap();
    w.write(&vec![300i16; seconds * 16_000]).unwrap();
    key
}

fn sidecar(dir: &str) -> Sidecar {
    Sidecar {
        exe: env!("CARGO_BIN_EXE_thera-fake-diarize").into(),
        args: vec!["--diarize".into()],
        dir: PathBuf::from(dir),
        threads: 2,
        on_spawn: None,
    }
}

#[test]
fn diarizes_labels_with_the_profile_and_removes_plaintext() {
    let tmp = tempfile::tempdir().unwrap();
    let audio = tmp.path().join("audio");
    let key = recording(&audio, 4);
    let work = tmp.path().join("work");
    let mut seen = vec![];
    let out = diarize::diarize_source(&audio, &key, &sidecar("lib"), &work, |p| seen.push(p), &AtomicBool::new(false)).unwrap();
    assert_eq!(seen, vec![40, 100]);
    assert!(!work.exists(), "audio and request are deleted");
    assert_eq!(out.turns.len(), 4);

    let segs: Vec<Segment> = (0..4).map(|s| Segment { start_ms: s * 1000 + 100, end_ms: s * 1000 + 800, text: "x".into() }).collect();
    // The profile sounds like speaker 1, who speaks second.
    let d = diarize::label(&segs, &out, Some(&[0.0, 1.0]), "fake");
    let labels: Vec<_> = d.labels.iter().map(|l| l.as_deref().unwrap()).collect();
    assert_eq!(labels, ["patient", "therapist", "patient", "therapist"]);
    let d = diarize::label(&segs, &out, None, "fake");
    assert_eq!(d.labels[0].as_deref(), Some("speaker_1"));

    let print = diarize::embed_source(&audio, &key, &sidecar("lib"), &work, &AtomicBool::new(false)).unwrap().unwrap();
    assert_eq!((print.embedding, print.seconds), (vec![0.0, 1.0], 4.0));
    assert!(!work.exists());
}

#[test]
fn failure_and_cancel_clean_up() {
    let tmp = tempfile::tempdir().unwrap();
    let audio = tmp.path().join("audio");
    let key = recording(&audio, 2);
    let work = tmp.path().join("work");
    let err = diarize::diarize_source(&audio, &key, &sidecar("fail"), &work, |_| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(err.to_string().contains("Modelldateien"), "{err}");
    assert!(!work.exists());

    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    let spawned = Arc::new(AtomicBool::new(false));
    let s = spawned.clone();
    let mut slow = sidecar("slow");
    slow.on_spawn = Some(Arc::new(move |_| s.store(true, Ordering::SeqCst)));
    let t = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        c.store(true, Ordering::SeqCst);
    });
    let started = std::time::Instant::now();
    let err = diarize::diarize_source(&audio, &key, &slow, &work, |_| {}, &cancel).unwrap_err();
    t.join().unwrap();
    assert!(matches!(err, Error::Cancelled));
    assert!(started.elapsed().as_secs() < 10);
    assert!(spawned.load(Ordering::SeqCst), "the process is handed out (job object)");
    assert!(!work.exists());
}
