//! Stand-in for `llama-server` in tests. Behaviour depends on the model file name: `crash` exits
//! like an out-of-memory load, `hang` never becomes ready, anything else loads for ~0.5 s and
//! then echoes the last chat message. Every route but `/health` needs `LLAMA_API_KEY`.
//! An extraction prompt gets one "Fakt: …" per transcript line in `verlauf`, citing that line,
//! plus one fact with an invented token that the app has to drop.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

fn arg(args: &[String], flag: &str) -> String {
    let i = args.iter().position(|a| a == flag).unwrap_or_else(|| panic!("missing {flag}"));
    args[i + 1].clone()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model = arg(&args, "-m");
    let alias = arg(&args, "--alias");
    assert_eq!(arg(&args, "--host"), "127.0.0.1");
    let key = std::env::var("LLAMA_API_KEY").expect("api key in the environment");
    if model.ends_with("crash") {
        eprintln!("llama_model_load: error loading model: unable to allocate CPU buffer");
        eprintln!("main: exiting due to model loading error");
        std::process::exit(1);
    }
    let listener = TcpListener::bind(("127.0.0.1", arg(&args, "--port").parse::<u16>().unwrap())).unwrap();
    let ready_at = Instant::now() + if model.ends_with("hang") { Duration::from_secs(3600) } else { Duration::from_millis(500) };
    for stream in listener.incoming().flatten() {
        handle(stream, &key, &alias, Instant::now() >= ready_at);
    }
}

fn handle(stream: TcpStream, key: &str, alias: &str, ready: bool) {
    let mut r = BufReader::new(stream.try_clone().unwrap());
    let mut first = String::new();
    if r.read_line(&mut first).is_err() {
        return;
    }
    let (mut len, mut auth) = (0usize, String::new());
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
        if lower.starts_with("authorization:") {
            auth = line["authorization:".len()..].trim().to_string();
        }
    }
    let mut body = vec![0; len];
    let _ = r.read_exact(&mut body);
    let path = first.split_whitespace().nth(1).unwrap_or("");
    let (status, reply) = if path == "/health" {
        if ready { (200, json!({"status": "ok"})) } else { (503, json!({"error": {"message": "Loading model"}})) }
    } else if auth != format!("Bearer {key}") {
        (401, json!({"error": {"message": "Invalid API Key"}}))
    } else if path == "/v1/models" {
        (200, json!({"data": [{"id": alias}]}))
    } else if path == "/v1/chat/completions" {
        let req: Value = serde_json::from_slice(&body).unwrap();
        let msgs = req["messages"].as_array().cloned().unwrap_or_default();
        let last = msgs.last().and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
        let system = msgs.first().and_then(|m| m["content"].as_str()).unwrap_or("");
        let content = if system.contains("Verlaufsdokumentation") { extraction(&last) } else { format!("Echo: {last}") };
        (200, json!({"model": alias, "choices": [{"message": {"role": "assistant", "content": content}}], "usage": {"prompt_tokens": 5, "completion_tokens": 3}}))
    } else {
        (404, json!({"error": {"message": "not found"}}))
    };
    let reply = reply.to_string();
    let mut w = stream;
    let _ = write!(w, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
}

/// Answer to an extraction request: `[S12] (Diktat, 00:05) text` lines become facts.
fn extraction(user: &str) -> String {
    let mut facts = Vec::new();
    for line in user.lines().filter(|l| l.starts_with("[S")) {
        let Some((num, rest)) = line[2..].split_once(']') else { continue };
        let text = rest.split_once(") ").map(|(_, t)| t).unwrap_or(rest).trim();
        let short: String = text.chars().take(80).collect();
        facts.push(json!({"section": "verlauf", "text": format!("Fakt: {short}"), "segments": [num.parse::<u64>().unwrap_or(0)]}));
    }
    if let Some(first) = facts.first().cloned() {
        facts.push(json!({"section": "verlauf", "text": "[PERSON_99] erfunden", "segments": first["segments"]}));
    }
    format!("```json\n{}\n```", json!({"facts": facts}))
}
