//! The llama-server runner against a fake server: start, readiness, key check, chat, failures.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use thera_core::llm::{self, ChatOptions, Role};
use thera_core::local::{self, LlamaServer};
use thera_core::pseudo::{Dictionary, Pseudonymizer};
use thera_core::Error;

fn exe() -> PathBuf {
    env!("CARGO_BIN_EXE_thera-fake-llama").into()
}

fn model_file(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, b"GGUF").unwrap();
    p
}

fn qwen() -> &'static local::LocalModel {
    local::model("qwen3-14b").unwrap()
}

#[test]
fn starts_waits_for_the_model_and_answers_only_with_the_key() {
    let tmp = tempfile::tempdir().unwrap();
    let mut server = LlamaServer::spawn(&exe(), &model_file(tmp.path(), "ok"), qwen()).unwrap();
    server.wait_ready(Duration::from_secs(20), &AtomicBool::new(false)).unwrap();
    assert!(server.alive());

    let ep = server.endpoint();
    assert!(ep.base_url.starts_with("http://127.0.0.1:"));
    let mut p = Pseudonymizer::new(Dictionary::new());
    let msg = p.pseudonymize("Hallo").unwrap();
    let c = llm::chat(&ep, "qwen3-14b", &[(Role::User, &msg)], &ChatOptions::default()).unwrap();
    assert_eq!(c.text, "Echo: Hallo");
    let mut pieces = 0;
    let c = llm::chat_stream(&ep, "qwen3-14b", &[(Role::User, &msg)], &ChatOptions::default(), &AtomicBool::new(false), |_| pieces += 1).unwrap();
    assert_eq!((c.text.as_str(), c.completion_tokens), ("Echo: Hallo", Some(3)));
    assert!(pieces > 1, "streamed in pieces");
    assert_eq!(llm::list_models(&ep).unwrap()[0].id, "qwen3-14b");

    let mut stranger = ep.clone();
    stranger.api_key = Some("guessed".into());
    let err = llm::list_models(&stranger).unwrap_err().to_string();
    assert!(err.contains("HTTP 401"), "{err}");

    drop(server);
    assert!(llm::list_models(&ep).is_err(), "server is gone after drop");
}

#[test]
fn a_failed_load_reports_the_server_log() {
    let tmp = tempfile::tempdir().unwrap();
    let mut server = LlamaServer::spawn(&exe(), &model_file(tmp.path(), "crash"), qwen()).unwrap();
    let err = server.wait_ready(Duration::from_secs(20), &AtomicBool::new(false)).unwrap_err().to_string();
    assert!(err.contains("unable to allocate CPU buffer"), "{err}");
}

#[test]
fn cancel_and_timeout_kill_the_server() {
    let tmp = tempfile::tempdir().unwrap();
    let path = model_file(tmp.path(), "hang");
    let mut server = LlamaServer::spawn(&exe(), &path, qwen()).unwrap();
    assert!(matches!(server.wait_ready(Duration::from_secs(20), &AtomicBool::new(true)), Err(Error::Cancelled)));
    assert!(!server.alive());

    let mut server = LlamaServer::spawn(&exe(), &path, qwen()).unwrap();
    let err = server.wait_ready(Duration::from_millis(800), &AtomicBool::new(false)).unwrap_err().to_string();
    assert!(err.contains("zu lange"), "{err}");
    assert!(!server.alive());
}

#[test]
fn a_missing_model_file_is_refused_before_start() {
    let err = LlamaServer::spawn(&exe(), Path::new("does-not-exist.gguf"), qwen()).err().unwrap().to_string();
    assert!(err.contains("nicht installiert"), "{err}");
}
