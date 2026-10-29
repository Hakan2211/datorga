//! thera core: the portable product. No tauri, no UI.

pub mod agent;
pub mod audio;
pub mod backup;
pub mod audit;
pub mod calendar;
pub mod catalog;
pub mod context;
pub mod consent;
pub mod crypto;
pub mod diarize;
pub mod dictate;
pub mod domain;
pub mod draft;
pub mod extract;
pub mod llm;
pub mod live;
pub mod local;
pub mod models;
pub mod photo;
pub mod pii;
pub mod pseudo;
pub mod render;
pub mod store;
pub mod stt;
pub mod summary;
pub mod templates;
pub mod tts;
pub mod workshop;

#[cfg(not(any(feature = "sqlcipher", feature = "plain-sqlite")))]
compile_error!("enable feature `sqlcipher` (production) or `plain-sqlite` (dev only)");

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("crypto: {0}")]
    Crypto(String),
    #[error("template: {0}")]
    Template(String),
    #[error("render: {0}")]
    Render(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Stt(String),
    #[error("{0}")]
    Llm(String),
    #[error("Abgebrochen")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, Error>;
