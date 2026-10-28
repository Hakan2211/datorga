//! `llm` against a one-shot local HTTP server: request shape, response parsing, error mapping,
//! and SSE streaming with cancel.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use thera_core::domain::Patient;
use thera_core::llm::{self, ChatOptions, Endpoint, ProviderId, Role};
use thera_core::pseudo::{Dictionary, Pseudonymizer};

struct Seen {
    head: String,
    body: String,
}

/// Serves one request with `status` + `body` and hands back what it received.
fn serve_once(status: u16, body: &'static str) -> (String, thread::JoinHandle<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}/v1", listener.local_addr().unwrap().port());
    let h = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut r = BufReader::new(stream.try_clone().unwrap());
        let mut head = String::new();
        let mut len = 0usize;
        loop {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap();
            }
            if line == "\r\n" {
                break;
            }
            head.push_str(&line);
        }
        let mut buf = vec![0; len];
        r.read_exact(&mut buf).unwrap();
        let mut w = stream;
        write!(w, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        Seen { head, body: String::from_utf8(buf).unwrap() }
    });
    (base, h)
}

fn ep(base: String) -> Endpoint {
    Endpoint { provider: ProviderId::Custom, base_url: base, api_key: Some("sk-secret".into()) }
}

fn patient() -> Patient {
    Patient {
        id: "p1".into(),
        external_ref: None,
        first_name: "Johanna".into(),
        last_name: "Brenner".into(),
        birth_date: Some("1984-03-12".into()),
        chiffre: None,
        pronouns: None,
        consent_recording: false,
        status: "active".into(),
        created_at: String::new(),
        archived_at: None,
    }
}

#[test]
fn chat_sends_only_tokens_and_restores_the_answer() {
    let (base, h) = serve_once(
        200,
        r#"{"model":"m-1","choices":[{"message":{"role":"assistant","content":"[PERSON_1] berichtet von Schlafproblemen."}}],"usage":{"prompt_tokens":21,"completion_tokens":7}}"#,
    );
    let mut p = Pseudonymizer::new(Dictionary::from_patient(&patient()));
    let sys = p.pseudonymize("Fasse sachlich zusammen.").unwrap();
    let user = p.pseudonymize("Frau Brenner berichtet, sie schlafe schlecht.").unwrap();
    let c = llm::chat(&ep(base), "m-1", &[(Role::System, &sys), (Role::User, &user)], &ChatOptions::default()).unwrap();
    let seen = h.join().unwrap();

    assert!(seen.head.starts_with("POST /v1/chat/completions"));
    assert!(seen.head.contains("authorization: Bearer sk-secret") || seen.head.contains("Authorization: Bearer sk-secret"));
    assert!(!seen.body.contains("Brenner"), "patient name left the machine: {}", seen.body);
    assert!(seen.body.contains("[PERSON_1]"));
    assert_eq!(c.model, "m-1");
    assert_eq!((c.prompt_tokens, c.completion_tokens), (Some(21), Some(7)));
    assert_eq!(p.map().restore(&c.text).text, "Brenner berichtet von Schlafproblemen.");
}

#[test]
fn cost_and_reasoning_tokens_are_read() {
    let (base, h) = serve_once(
        200,
        r#"{"model":"m","choices":[{"message":{"content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":300,"completion_tokens_details":{"reasoning_tokens":290},"cost":0.00042}}"#,
    );
    let c = llm::ping(&ep(base), "m").unwrap();
    h.join().unwrap();
    assert_eq!((c.completion_tokens, c.reasoning_tokens), (Some(300), Some(290)));
    assert_eq!(c.cost, Some(0.00042));
}

#[test]
fn a_budget_spent_on_thinking_is_reported_not_returned_empty() {
    let (base, h) = serve_once(
        200,
        r#"{"model":"m","choices":[{"message":{"content":""},"finish_reason":"length"}],"usage":{"prompt_tokens":10,"completion_tokens":2048}}"#,
    );
    let err = llm::ping(&ep(base), "m").unwrap_err().to_string();
    h.join().unwrap();
    assert!(err.contains("Token-Budget"), "{err}");
}

#[test]
fn models_are_listed_sorted() {
    let (base, h) = serve_once(200, r#"{"data":[{"id":"z/b","name":"B","context_length":32000},{"id":"a/a"},{"bad":1}]}"#);
    let models = llm::list_models(&ep(base)).unwrap();
    assert!(h.join().unwrap().head.starts_with("GET /v1/models"));
    assert_eq!(models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["a/a", "z/b"]);
    assert_eq!(models[1].context_length, Some(32000));
}

#[test]
fn api_errors_become_readable_messages() {
    let (base, h) = serve_once(401, r#"{"error":{"message":"Invalid API key"}}"#);
    let err = llm::list_models(&ep(base)).unwrap_err().to_string();
    h.join().unwrap();
    assert!(err.contains("API-Schlüssel ungültig") && err.contains("Invalid API key"), "{err}");

    // 200 with an error object (OpenRouter style when no ZDR endpoint matches).
    let (base, h) = serve_once(200, r#"{"error":{"message":"No endpoints found matching your data policy"}}"#);
    let err = llm::ping(&ep(base), "x").unwrap_err().to_string();
    h.join().unwrap();
    assert!(err.contains("data policy"), "{err}");
}

#[test]
fn unreachable_endpoint_fails_cleanly() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let err = llm::list_models(&ep(format!("http://127.0.0.1:{port}/v1"))).unwrap_err().to_string();
    assert!(err.starts_with("Verbindung fehlgeschlagen"), "{err}");
}

// ---------- streaming ----------

/// Serves one request as an SSE stream: each part is written and flushed, then the server waits
/// `pause` before the next one. Hands back the request body.
fn serve_stream(parts: &'static [&'static str], pause: Duration) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}/v1", listener.local_addr().unwrap().port());
    let h = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut r = BufReader::new(stream.try_clone().unwrap());
        let mut len = 0usize;
        loop {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap();
            }
            if line == "\r\n" {
                break;
            }
        }
        let mut buf = vec![0; len];
        r.read_exact(&mut buf).unwrap();
        let mut w = stream;
        write!(w, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").unwrap();
        for p in parts {
            // A cancelled client has hung up; stop quietly.
            if w.write_all(p.as_bytes()).and_then(|_| w.flush()).is_err() {
                break;
            }
            thread::sleep(pause);
        }
        String::from_utf8(buf).unwrap()
    });
    (base, h)
}

fn hello() -> thera_core::pseudo::Pseudonymized<String> {
    Pseudonymizer::new(Dictionary::new()).pseudonymize("Hallo").unwrap()
}

#[test]
fn stream_joins_deltas_and_reads_usage_from_the_last_chunk() {
    let (base, h) = serve_stream(
        &[
            ": OPENROUTER PROCESSING\n\n",
            "data: {\"model\":\"m-2\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}\n\n",
            "data: {\"model\":\"m-2\",\"choices\":[{\"delta\":{\"content\":\"Schlaf\"}}]}\n\n",
            "data: {\"model\":\"m-2\",\"choices\":[{\"delta\":{\"content\":\"störung \u{fc}ber\"}}]}\n\ndata: {\"model\":\"m-2\",\"choices\":[{\"delta\":{\"content\":\" Wochen\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"model\":\"m-2\",\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":4,\"cost\":0.0001}}\n\n",
            "data: [DONE]\n\n",
        ],
        Duration::from_millis(5),
    );
    let mut deltas = Vec::new();
    let msg = hello();
    let c = llm::chat_stream(&ep(base), "m", &[(Role::User, &msg)], &ChatOptions::default(), &AtomicBool::new(false), |t| {
        deltas.push(t.to_string())
    })
    .unwrap();
    let body: serde_json::Value = serde_json::from_str(&h.join().unwrap()).unwrap();
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    assert_eq!(deltas, ["Schlaf", "störung über", " Wochen"]);
    assert_eq!(c.text, "Schlafstörung über Wochen");
    assert_eq!(c.model, "m-2");
    assert_eq!((c.prompt_tokens, c.completion_tokens, c.cost), (Some(9), Some(4), Some(0.0001)));
}

#[test]
fn cancel_ends_a_stream_quickly() {
    // One delta, then the server would take 5 s per part.
    let (base, h) = serve_stream(
        &["data: {\"choices\":[{\"delta\":{\"content\":\"Teil\"}}]}\n\n", "data: {\"choices\":[{\"delta\":{\"content\":\" zwei\"}}]}\n\n"],
        Duration::from_secs(5),
    );
    let cancel = AtomicBool::new(false);
    let msg = hello();
    let started = Instant::now();
    let err = llm::chat_stream(&ep(base), "m", &[(Role::User, &msg)], &ChatOptions::default(), &cancel, |_| {
        cancel.store(true, Ordering::Relaxed)
    })
    .unwrap_err();
    assert!(matches!(err, thera_core::Error::Cancelled), "{err}");
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
    drop(h); // the server thread ends on its own once its sleeps are over
}

#[test]
fn cancel_works_before_the_first_byte() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}/v1", listener.local_addr().unwrap().port());
    // Accepts and never answers (a model still reading the prompt).
    let _h = thread::spawn(move || {
        let conn = listener.accept();
        thread::sleep(Duration::from_secs(5));
        drop(conn);
    });
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let c2 = cancel.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        c2.store(true, Ordering::Relaxed);
    });
    let msg = hello();
    let started = Instant::now();
    let err = llm::chat_stream(&ep(base), "m", &[(Role::User, &msg)], &ChatOptions::default(), &cancel, |_| {}).unwrap_err();
    assert!(matches!(err, thera_core::Error::Cancelled), "{err}");
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
}

#[test]
fn an_error_mid_stream_is_reported() {
    let (base, h) = serve_stream(
        &[
            "data: {\"choices\":[{\"delta\":{\"content\":\"Anfang\"}}]}\n\n",
            "data: {\"error\":{\"message\":\"Provider overloaded\"},\"choices\":[{\"delta\":{\"content\":\"\"},\"finish_reason\":\"error\"}]}\n\n",
        ],
        Duration::from_millis(5),
    );
    let msg = hello();
    let err = llm::chat_stream(&ep(base), "m", &[(Role::User, &msg)], &ChatOptions::default(), &AtomicBool::new(false), |_| {})
        .unwrap_err()
        .to_string();
    h.join().unwrap();
    assert!(err.contains("Provider overloaded"), "{err}");
}

#[test]
fn a_stream_with_only_thinking_is_reported_as_cut() {
    let (base, h) = serve_stream(
        &["data: {\"choices\":[{\"delta\":{\"content\":\"\"},\"finish_reason\":\"length\"}]}\n\n", "data: [DONE]\n\n"],
        Duration::from_millis(5),
    );
    let msg = hello();
    let err = llm::chat_stream(&ep(base), "m", &[(Role::User, &msg)], &ChatOptions::default(), &AtomicBool::new(false), |_| {})
        .unwrap_err()
        .to_string();
    h.join().unwrap();
    assert!(err.contains("Token-Budget"), "{err}");
}

#[test]
fn an_endpoint_that_ignores_stream_still_works() {
    let (base, h) = serve_once(200, r#"{"model":"m","choices":[{"message":{"content":"Ganz"}}]}"#);
    let mut deltas = Vec::new();
    let msg = hello();
    let c = llm::chat_stream(&ep(base), "m", &[(Role::User, &msg)], &ChatOptions::default(), &AtomicBool::new(false), |t| {
        deltas.push(t.to_string())
    })
    .unwrap();
    h.join().unwrap();
    assert_eq!((c.text.as_str(), deltas), ("Ganz", vec!["Ganz".to_string()]));

    let (base, h) = serve_once(429, r#"{"error":{"message":"slow down"}}"#);
    let err = llm::chat_stream(&ep(base), "m", &[(Role::User, &msg)], &ChatOptions::default(), &AtomicBool::new(false), |_| {})
        .unwrap_err()
        .to_string();
    h.join().unwrap();
    assert!(err.contains("Zu viele Anfragen") && err.contains("slow down"), "{err}");
}
