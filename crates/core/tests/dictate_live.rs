//! Live dictation pieces: the cloud engine against a local HTTP mock (request shape, usage,
//! silence not sent, markers dropped), and `real_whisper_server` with the actual sidecar when
//! THERA_WHISPER_SERVER / _MODEL / _WAV are set.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::Value;
use thera_core::dictate::{self, LiveEngine, Piece, Splitter};
use thera_core::llm::{Endpoint, ProviderId};
use thera_core::stt::CloudStt;

/// Answers each request with the next of `answers` and keeps the bodies.
fn serve(answers: Vec<&'static str>) -> (String, Arc<Mutex<Vec<Vec<u8>>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}/v1", listener.local_addr().unwrap().port());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    thread::spawn(move || {
        for (n, stream) in listener.incoming().flatten().enumerate() {
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            log.lock().unwrap().push(body);
            let answer = answers[n.min(answers.len() - 1)];
            let mut w = stream;
            let _ = write!(w, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}", answer.len());
        }
    });
    (base, seen)
}

fn piece(seq: u32, pcm: Vec<i16>) -> Piece {
    Piece { seq, start_ms: 0, pcm }
}

fn speech(ms: usize) -> Vec<i16> {
    (0..ms * 16).map(|i| if i % 40 < 20 { 4_000 } else { -4_000 }).collect()
}

#[test]
fn cloud_pieces_go_out_with_zero_retention_and_come_back_as_text() {
    let (base, seen) = serve(vec![
        r#"{"text":" Pat. berichtet von Schlafproblemen. ","usage":{"seconds":1.5,"cost":0.0002}}"#,
        r#"{"text":"[BLANK_AUDIO]"}"#,
    ]);
    let cloud = CloudStt {
        endpoint: Endpoint { provider: ProviderId::Openrouter, base_url: base, api_key: Some("k".into()) },
        model: "openai/whisper-large-v3-turbo".into(),
        language: "de".into(),
    };
    let engine = LiveEngine::Cloud(&cloud);
    let cancel = AtomicBool::new(false);

    let (text, usage) = dictate::transcribe_piece(&engine, &piece(1, speech(1_500)), Some("vorher"), &cancel).unwrap();
    assert_eq!(text.as_deref(), Some("Pat. berichtet von Schlafproblemen."));
    let usage = usage.unwrap();
    assert_eq!((usage.requests, usage.seconds, usage.cost), (1, 1.5, Some(0.0002)));
    let body: Value = serde_json::from_slice(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(body["provider"]["zdr"], true);
    assert_eq!(body["input_audio"]["format"], "wav");
    assert!(body.get("prompt").is_none(), "no earlier text goes to the cloud");

    // Digital silence isn't sent at all; a marker-only answer is no text.
    let (text, usage) = dictate::transcribe_piece(&engine, &piece(2, vec![0; 16_000]), None, &cancel).unwrap();
    assert!(text.is_none() && usage.is_none());
    assert_eq!(seen.lock().unwrap().len(), 1);
    let (text, _) = dictate::transcribe_piece(&engine, &piece(3, speech(800)), None, &cancel).unwrap();
    assert!(text.is_none());
}

#[test]
#[ignore = "needs THERA_WHISPER_SERVER, THERA_WHISPER_MODEL, THERA_WHISPER_WAV"]
fn real_whisper_server() {
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} not set"));
    let wav = std::fs::read(var("THERA_WHISPER_WAV")).unwrap();
    let pcm: Vec<i16> = wav[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    let t = std::time::Instant::now();
    let mut server = dictate::WhisperServer::spawn(var("THERA_WHISPER_SERVER").as_ref(), var("THERA_WHISPER_MODEL").as_ref(), "test", "de", 8).unwrap();
    server.wait_ready(std::time::Duration::from_secs(120), || false).unwrap();
    eprintln!("ready after {:.1}s", t.elapsed().as_secs_f32());
    let mut split = Splitter::new();
    // As the microphone delivers it: 100 ms buffers.
    let mut pieces: Vec<Piece> = pcm.chunks(1_600).flat_map(|b| split.push(b)).collect();
    pieces.extend(split.finish());
    let cancel = AtomicBool::new(false);
    let mut previous: Option<String> = None;
    for p in &pieces {
        let t = std::time::Instant::now();
        let (text, _) = dictate::transcribe_piece(&LiveEngine::Local(&server), p, previous.as_deref(), &cancel).unwrap();
        eprintln!("#{} at {:.1}s, {:.1}s audio → {:.2}s: {:?}", p.seq, p.start_ms as f32 / 1000.0, p.duration_ms() as f32 / 1000.0, t.elapsed().as_secs_f32(), text);
        previous = text.or(previous);
    }
    assert!(!pieces.is_empty());
}
