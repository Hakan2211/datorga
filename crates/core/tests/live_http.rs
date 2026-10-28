//! GPT-Live session creation against a local HTTP mock: request shape, SDP answer, errors.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use thera_core::live;
use thera_core::llm::{Endpoint, ProviderId};
use thera_core::pseudo::{Dictionary, Pseudonymizer};
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
    Endpoint { provider, base_url: base.into(), api_key: Some("sk-live".into()) }
}

#[test]
fn creates_a_session_and_returns_the_answer() {
    let (base, seen) = serve(|_| (201, "application/json", br#"{"session":{"id":"live_123"},"transport":{"type":"webrtc","sdp":"v=0\r\nanswer"}}"#.to_vec()));
    let brief = Pseudonymizer::new(Dictionary::new()).pseudonymize("Letzte Sitzung:\nVerlauf: Pat. schläft besser.").unwrap();
    let s = live::create_session(&ep(ProviderId::Openai, &base), "juni", &brief, "v=0\r\noffer").unwrap();
    assert_eq!((s.id.as_str(), s.sdp.as_str()), ("live_123", "v=0\r\nanswer"));
    let seen = seen.lock().unwrap();
    assert!(seen[0].0.starts_with("POST /v1/live/sessions"));
    assert!(seen[0].0.contains("Bearer sk-live"));
    let body: serde_json::Value = serde_json::from_str(&seen[0].1).unwrap();
    assert_eq!(body["transport"]["sdp"], "v=0\r\noffer");
    assert_eq!(body["session"]["audio"]["output"]["voice"], "juni");
    assert!(body["session"]["instructions"].as_str().unwrap().contains("Pat. schläft besser."));
}

#[test]
fn refuses_and_reports_errors() {
    let brief = Pseudonymizer::new(Dictionary::new()).pseudonymize("x").unwrap();
    let (base, _) = serve(|_| (403, "application/json", br#"{"error":{"message":"Project does not have access to gpt-live-1"}}"#.to_vec()));
    let e = live::create_session(&ep(ProviderId::Openai, &base), "nuri", &brief, "v=0").unwrap_err();
    assert!(matches!(&e, Error::Llm(m) if m.contains("gpt-live-1")), "{e:?}");
    let (base, _) = serve(|_| (201, "application/json", b"{}".to_vec()));
    assert!(matches!(live::create_session(&ep(ProviderId::Openai, &base), "nuri", &brief, "v=0"), Err(Error::Llm(_))));
    assert!(matches!(live::create_session(&ep(ProviderId::Openrouter, &base), "nuri", &brief, "v=0"), Err(Error::Invalid(_))));
    assert!(matches!(live::create_session(&ep(ProviderId::Openai, &base), "nuri", &brief, "garbage"), Err(Error::Invalid(_))));
}
