//! The cloud speech engine against a local HTTP mock: request shape per provider, segments with
//! piece offsets, the plain-text fallback, splitting, silence, cancel and the dictation-only rule.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use thera_core::audio::{AudioKey, ChunkWriter};
use thera_core::domain::SourceKind;
use thera_core::llm::{Endpoint, ProviderId};
use thera_core::stt::{self, CloudStt, SttEngine};
use thera_core::Error;

struct Seen {
    head: String,
    body: Vec<u8>,
}

/// Answers every request with `reply(n, body)` → (status, JSON) and keeps what it received.
fn serve(reply: impl Fn(usize, &[u8]) -> (u16, String) + Send + 'static) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}/v1", listener.local_addr().unwrap().port());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    thread::spawn(move || {
        for (n, stream) in listener.incoming().flatten().enumerate() {
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let (mut head, mut len) = (String::new(), 0usize);
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                head.push_str(&line);
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let (status, answer) = reply(n, &body);
            log.lock().unwrap().push(Seen { head, body });
            let mut w = stream;
            let _ = write!(w, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}", answer.len());
        }
    });
    (base, seen)
}

/// `seconds` of speech-level signal (a square wave), as an encrypted recording.
fn recording(dir: &Path, seconds: usize) -> AudioKey {
    let key = AudioKey::generate().unwrap();
    let pcm: Vec<i16> = (0..seconds * 16_000).map(|i| if i % 2 == 0 { 3_000 } else { -3_000 }).collect();
    ChunkWriter::create(dir, &key).unwrap().write(&pcm).unwrap();
    key
}

fn engine(provider: ProviderId, base: String) -> SttEngine {
    SttEngine::Cloud(CloudStt {
        endpoint: Endpoint { provider, base_url: base, api_key: Some("sk-secret".into()) },
        model: "openai/whisper-large-v3-turbo".into(),
        language: "de".into(),
    })
}

fn run(kind: SourceKind, audio: &Path, key: &AudioKey, engine: &SttEngine, cancel: &AtomicBool) -> thera_core::Result<stt::Transcribed> {
    let work = audio.parent().unwrap().join("work");
    let r = stt::transcribe(kind, audio, key, engine, &work, |_| {}, cancel);
    assert!(!work.exists(), "the cloud engine writes nothing to disk");
    r
}

#[test]
fn openrouter_gets_base64_json_with_zero_retention_and_segments_come_back_timed() {
    let (base, seen) = serve(|_, _| {
        (200, r#"{"text":"x","segments":[{"start":0.0,"end":2.5,"text":" Die Patientin schläft schlecht."},{"start":2.5,"end":4.0,"text":" Vielen Dank fürs Zuschauen"}],"usage":{"seconds":5.0,"cost":0.0004}}"#.into())
    });
    let tmp = tempfile::tempdir().unwrap();
    let key = recording(&tmp.path().join("audio"), 5);
    let t = run(SourceKind::Dictation, &tmp.path().join("audio"), &key, &engine(ProviderId::Openrouter, base), &AtomicBool::new(false)).unwrap();

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].head.starts_with("POST /v1/audio/transcriptions"));
    assert!(seen[0].head.to_ascii_lowercase().contains("authorization: bearer sk-secret"));
    let body: serde_json::Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(body["model"], "openai/whisper-large-v3-turbo");
    assert_eq!(body["language"], "de");
    assert_eq!(body["response_format"], "verbose_json");
    assert_eq!(body["provider"]["zdr"], true);
    assert_eq!(body["input_audio"]["format"], "wav");
    use base64::Engine as _;
    let wav = base64::engine::general_purpose::STANDARD.decode(body["input_audio"]["data"].as_str().unwrap()).unwrap();
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(wav.len(), 44 + 5 * 32_000);

    assert_eq!(t.segments, vec![stt::Segment { start_ms: 0, end_ms: 2500, text: "Die Patientin schläft schlecht.".into() }], "hallucination dropped");
    let usage = t.cloud.unwrap();
    assert_eq!((usage.requests, usage.seconds, usage.cost), (1, 5.0, Some(0.0004)));
}

#[test]
fn a_custom_endpoint_gets_multipart() {
    let (base, seen) = serve(|_, _| (200, r#"{"text":"Kurzes Diktat."}"#.into()));
    let tmp = tempfile::tempdir().unwrap();
    let key = recording(&tmp.path().join("audio"), 2);
    let t = run(SourceKind::Dictation, &tmp.path().join("audio"), &key, &engine(ProviderId::Custom, base), &AtomicBool::new(false)).unwrap();
    let seen = seen.lock().unwrap();
    assert!(seen[0].head.to_ascii_lowercase().contains("content-type: multipart/form-data; boundary="));
    let body = String::from_utf8_lossy(&seen[0].body);
    assert!(body.contains("name=\"file\"; filename=\"audio.wav\""));
    assert!(body.contains("name=\"language\"\r\n\r\nde\r\n"));
    assert!(body.contains("RIFF"));
    // Text only: one segment over the whole piece.
    assert_eq!(t.segments, vec![stt::Segment { start_ms: 0, end_ms: 2000, text: "Kurzes Diktat.".into() }]);
    assert_eq!(t.cloud.unwrap().cost, None);
}

#[test]
fn a_model_without_segments_is_asked_again_for_text() {
    let (base, seen) = serve(|n, _| match n {
        0 => (400, r#"{"error":{"message":"verbose_json is not supported"}}"#.into()),
        _ => (200, r#"{"text":"Nur Text."}"#.into()),
    });
    let tmp = tempfile::tempdir().unwrap();
    let key = recording(&tmp.path().join("audio"), 2);
    let t = run(SourceKind::Dictation, &tmp.path().join("audio"), &key, &engine(ProviderId::Openrouter, base), &AtomicBool::new(false)).unwrap();
    let seen = seen.lock().unwrap();
    let second: serde_json::Value = serde_json::from_slice(&seen[1].body).unwrap();
    assert_eq!(second["response_format"], "json");
    assert_eq!(t.segments[0].text, "Nur Text.");
}

#[test]
fn long_dictations_are_split_and_silent_pieces_are_not_sent() {
    // 30 s speech, 500 s digital silence, 30 s speech: three pieces, the middle one silent.
    let tmp = tempfile::tempdir().unwrap();
    let key = AudioKey::generate().unwrap();
    let tone = |s: usize| (0..s * 16_000).map(|i| if i % 2 == 0 { 3_000i16 } else { -3_000 }).collect::<Vec<_>>();
    let mut pcm = tone(30);
    pcm.extend(vec![0i16; 500 * 16_000]);
    pcm.extend(tone(30));
    ChunkWriter::create(&tmp.path().join("audio"), &key).unwrap().write(&pcm).unwrap();
    // The last piece starts near 480 s; its speech begins at 530 s.
    let (base, seen) = serve(|n, _| match n {
        0 => (200, r#"{"segments":[{"start":1.0,"end":3.0,"text":"Anfang."}]}"#.into()),
        _ => (200, r#"{"segments":[{"start":0.5,"end":2.0,"text":"Über Stille."},{"start":52.0,"end":54.0,"text":"Ende."}]}"#.into()),
    });
    let t = run(SourceKind::Dictation, &tmp.path().join("audio"), &key, &engine(ProviderId::Openrouter, base), &AtomicBool::new(false)).unwrap();

    let sizes: Vec<usize> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|s| serde_json::from_slice::<serde_json::Value>(&s.body).unwrap()["input_audio"]["data"].as_str().unwrap().len())
        .collect();
    assert_eq!(sizes.len(), 2, "the silent piece was skipped: {sizes:?}");
    assert!(sizes.iter().all(|&b| b < 25 << 20), "under the upload limit");
    // Segment times are offset by where each piece starts; the first piece is at most 4 min.
    let texts: Vec<&str> = t.segments.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, ["Anfang.", "Ende."], "the segment over silence is gated");
    assert_eq!(t.segments[0].start_ms, 1000);
    assert!(t.segments[1].start_ms > 480_000, "{:?}", t.segments[1]);
}

#[test]
fn session_recordings_never_go_to_the_cloud() {
    let (base, seen) = serve(|_, _| (200, r#"{"text":"darf nicht passieren"}"#.into()));
    let tmp = tempfile::tempdir().unwrap();
    let key = recording(&tmp.path().join("audio"), 2);
    let err = run(SourceKind::SessionAudio, &tmp.path().join("audio"), &key, &engine(ProviderId::Openrouter, base), &AtomicBool::new(false)).unwrap_err();
    assert!(err.to_string().contains("nur lokal"), "{err}");
    thread::sleep(Duration::from_millis(100));
    assert!(seen.lock().unwrap().is_empty(), "nothing was sent");
}

#[test]
fn cancel_does_not_wait_for_the_provider() {
    let (base, _) = serve(|_, _| {
        thread::sleep(Duration::from_secs(5));
        (200, r#"{"text":"zu spät"}"#.into())
    });
    let tmp = tempfile::tempdir().unwrap();
    let key = recording(&tmp.path().join("audio"), 2);
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        c.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let err = run(SourceKind::Dictation, &tmp.path().join("audio"), &key, &engine(ProviderId::Openrouter, base), &cancel).unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn provider_errors_are_readable() {
    let (base, _) = serve(|_, _| (402, r#"{"error":{"message":"Insufficient credits"}}"#.into()));
    let tmp = tempfile::tempdir().unwrap();
    let key = recording(&tmp.path().join("audio"), 2);
    let err = run(SourceKind::Dictation, &tmp.path().join("audio"), &key, &engine(ProviderId::Openrouter, base), &AtomicBool::new(false)).unwrap_err().to_string();
    assert!(err.contains("Guthaben") && err.contains("Insufficient credits"), "{err}");
}
