//! Read-aloud against a local HTTP mock: request shape, pieces joined, errors.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use thera_core::llm::{Endpoint, ProviderId};
use thera_core::pseudo::{Dictionary, Pseudonymizer};
use thera_core::tts;
use thera_core::Error;

/// Answers every request with `reply(n)` → (status, content type, body) and keeps the request bodies.
fn serve(reply: impl Fn(usize) -> (u16, &'static str, Vec<u8>) + Send + 'static) -> (String, Arc<Mutex<Vec<(String, String)>>>) {
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
            log.lock().unwrap().push((head, String::from_utf8(body).unwrap()));
            let (status, ctype, answer) = reply(n);
            let mut w = stream;
            let _ = write!(w, "HTTP/1.1 {status} X\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", answer.len());
            let _ = w.write_all(&answer);
        }
    });
    (base, seen)
}

fn ep(provider: ProviderId, base: &str) -> Endpoint {
    Endpoint { provider, base_url: base.into(), api_key: Some("sk-test".into()) }
}

#[test]
fn speaks_and_joins_pieces() {
    let (base, seen) = serve(|n| (200, "audio/mpeg", vec![0xFF, 0xFB, n as u8]));
    let mut p = Pseudonymizer::new(Dictionary::new());
    let long = format!("{} Ende.", "Ein Satz zum Vorlesen. ".repeat(250));
    let text = p.pseudonymize(&long).unwrap();
    let speech = tts::speak(&ep(ProviderId::Openrouter, &base), "openai/gpt-4o-mini-tts-2025-12-15", "sage", &text).unwrap();
    assert_eq!(speech.audio, vec![0xFF, 0xFB, 0, 0xFF, 0xFB, 1]);
    assert_eq!(speech.chars, long.trim().chars().count());
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert!(seen[0].0.starts_with("POST /v1/audio/speech"));
    assert!(seen[0].0.contains("Bearer sk-test"));
    let body: serde_json::Value = serde_json::from_str(&seen[0].1).unwrap();
    assert_eq!(body["voice"], "sage");
    assert_eq!(body["provider"]["zdr"], true);
    assert!(body["input"].as_str().unwrap().ends_with("Vorlesen."));
}

#[test]
fn errors_come_back_readable() {
    let (base, _) = serve(|_| (400, "application/json", br#"{"error":{"message":"No endpoints found matching your data policy"}}"#.to_vec()));
    let e = tts::sample(&ep(ProviderId::Openrouter, &base), "x/y", "coral").unwrap_err();
    assert!(matches!(&e, Error::Llm(m) if m.contains("data policy")), "{e:?}");

    let (base, _) = serve(|_| (200, "application/json", br#"{"error":{"message":"kaputt"}}"#.to_vec()));
    assert!(matches!(tts::sample(&ep(ProviderId::Custom, &base), "kokoro", "coral"), Err(Error::Llm(_))));

    assert!(matches!(tts::sample(&ep(ProviderId::Openai, "http://example.com/v1"), "gpt-4o-mini-tts", "coral"), Err(Error::Invalid(_))));
    assert!(matches!(tts::sample(&ep(ProviderId::Openai, "https://x/v1"), " ", "coral"), Err(Error::Invalid(_))));
}
