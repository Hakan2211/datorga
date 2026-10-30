//! Reading answers aloud over the OpenAI-compatible `POST /audio/speech` (OpenRouter, OpenAI
//! directly, or a custom endpoint). Optional and off by default.
//!
//! Egress invariant as in [`crate::llm`]: [`speak`] takes only [`Pseudonymized`] text. The audio
//! comes back as MP3 bytes and is played from memory, never written to disk.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

use crate::llm::{api_error, net, Endpoint, ProviderId};
use crate::pseudo::{Dictionary, Pseudonymized, Pseudonymizer};
use crate::{Error, Result};

/// Setting key holding the [`TtsConfig`] JSON.
pub const CONFIG_SETTING: &str = "tts_config";

/// OpenAI's limit per request is 4096 characters; longer text goes in pieces cut at sentences.
pub const MAX_CHARS: usize = 4000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TtsConfig {
    #[serde(default)]
    pub enabled: bool,
    /// OpenRouter, OpenAI or the custom endpoint of the AI settings (same key, same base URL).
    #[serde(default = "default_provider")]
    pub provider: ProviderId,
    /// `None` = the provider's [`default_model`]; a custom endpoint needs one.
    #[serde(default)]
    pub model: Option<String>,
    /// `None` = [`DEFAULT_VOICE`].
    #[serde(default)]
    pub voice: Option<String>,
}

fn default_provider() -> ProviderId {
    ProviderId::Openrouter
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self { enabled: false, provider: default_provider(), model: None, voice: None }
    }
}

impl TtsConfig {
    pub fn validate(&self) -> Result<()> {
        if !matches!(self.provider, ProviderId::Openrouter | ProviderId::Openai | ProviderId::Custom) {
            return Err(Error::Invalid("Vorlesen gibt es nur über OpenRouter, OpenAI oder einen eigenen Endpunkt".into()));
        }
        Ok(())
    }

    /// The model to send, if any.
    pub fn model(&self) -> Option<String> {
        match self.model.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
            Some(m) => Some(m.to_string()),
            None => default_model(self.provider).map(str::to_string),
        }
    }

    pub fn voice(&self) -> String {
        self.voice.as_deref().map(str::trim).filter(|v| !v.is_empty()).unwrap_or(DEFAULT_VOICE).to_string()
    }
}

/// OpenAI's small speech model, on OpenRouter (sent with `zdr: true`) or directly.
pub fn default_model(provider: ProviderId) -> Option<&'static str> {
    match provider {
        ProviderId::Openrouter => Some("openai/gpt-4o-mini-tts-2025-12-15"),
        ProviderId::Openai => Some("gpt-4o-mini-tts"),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TtsVoice {
    pub id: &'static str,
    pub label: &'static str,
}

pub const DEFAULT_VOICE: &str = "coral";

/// OpenAI's voices that sound natural in German. Other models may name theirs differently;
/// Settings also takes a free voice id.
pub const VOICES: &[TtsVoice] = &[
    TtsVoice { id: "coral", label: "Coral (warm, weiblich)" },
    TtsVoice { id: "sage", label: "Sage (ruhig, weiblich)" },
    TtsVoice { id: "marin", label: "Marin (natürlich, weiblich)" },
    TtsVoice { id: "cedar", label: "Cedar (natürlich, männlich)" },
    TtsVoice { id: "ash", label: "Ash (klar, männlich)" },
    TtsVoice { id: "alloy", label: "Alloy (neutral)" },
];

/// Delivery hint for models that take one (OpenAI `gpt-4o-mini-tts`); others ignore it.
const INSTRUCTIONS: &str = "Sprich ruhig, deutlich und in normalem Tempo auf Deutsch.";

/// Synthesized speech, MP3.
pub struct Speech {
    pub audio: Vec<u8>,
    /// Characters sent (what the providers bill by).
    pub chars: usize,
    pub latency_ms: u64,
}

impl std::fmt::Debug for Speech {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Speech").field("bytes", &self.audio.len()).field("chars", &self.chars).finish()
    }
}

/// The request body for one piece. The text must have passed the pseudonymizer.
pub fn speech_body(ep: &Endpoint, model: &str, voice: &str, text: &str) -> serde_json::Value {
    let mut body = json!({ "model": model, "input": text, "voice": voice, "response_format": "mp3" });
    if model.contains("gpt-4o-mini-tts") {
        body["instructions"] = json!(INSTRUCTIONS);
    }
    if ep.provider == ProviderId::Openrouter {
        // Only zero-retention endpoints that don't collect data. No match = error, not a fallback.
        body["provider"] = json!({ "zdr": true, "data_collection": "deny" });
    }
    body
}

/// `POST /audio/speech`, one request per [`MAX_CHARS`] piece; the MP3s are joined (MP3 frames
/// play back to back).
pub fn speak(ep: &Endpoint, model: &str, voice: &str, text: &Pseudonymized<String>) -> Result<Speech> {
    if model.trim().is_empty() {
        return Err(Error::Invalid("Kein Sprachmodell gewählt".into()));
    }
    let text = text.get().trim();
    if text.is_empty() {
        return Err(Error::Invalid("Kein Text zum Vorlesen".into()));
    }
    let started = Instant::now();
    let url = ep.url("/audio/speech")?;
    let mut audio = Vec::new();
    for piece in pieces(text, MAX_CHARS) {
        let body = speech_body(ep, model, voice, piece).to_string();
        let mut resp = ep
            .auth(ep.agent(Duration::from_secs(60)).post(&url))
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(net)?;
        let status = resp.status().as_u16();
        let bytes = resp.body_mut().with_config().limit(32 << 20).read_to_vec().map_err(net)?;
        if status != 200 {
            return Err(Error::Llm(api_error(status, &String::from_utf8_lossy(&bytes))));
        }
        // A router may answer 200 with a JSON error instead of audio.
        if bytes.first() == Some(&b'{') {
            return Err(Error::Llm(api_error(status, &String::from_utf8_lossy(&bytes))));
        }
        audio.extend(bytes);
    }
    Ok(Speech { audio, chars: text.chars().count(), latency_ms: started.elapsed().as_millis() as u64 })
}

/// A fixed German sentence (no patient data) to try a voice in Settings.
pub fn sample(ep: &Endpoint, model: &str, voice: &str) -> Result<Speech> {
    let mut p = Pseudonymizer::new(Dictionary::new());
    let text = p.pseudonymize("Guten Tag. So klingt die Vorlesestimme in Theralo.")?;
    speak(ep, model, voice, &text)
}

/// Splits `text` into pieces of at most `max` characters, cut after a sentence end where
/// possible, else at a space, else hard.
pub fn pieces(text: &str, max: usize) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while rest.chars().count() > max {
        let limit = rest.char_indices().nth(max).map_or(rest.len(), |(i, _)| i);
        let head = &rest[..limit];
        let cut = head
            .rmatch_indices(['.', '!', '?', '\n'])
            .map(|(i, m)| i + m.len())
            .find(|&i| i > limit / 2)
            .or_else(|| head.rfind(' ').filter(|&i| i > 0))
            .unwrap_or(limit);
        out.push(rest[..cut].trim());
        rest = rest[cut..].trim_start();
    }
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(provider: ProviderId) -> Endpoint {
        Endpoint { provider, base_url: "https://x/v1".into(), api_key: None }
    }

    #[test]
    fn config_defaults_and_validation() {
        let c = TtsConfig::default();
        assert!(!c.enabled);
        assert_eq!(c.model().as_deref(), Some("openai/gpt-4o-mini-tts-2025-12-15"));
        assert_eq!(c.voice(), "coral");
        let openai = TtsConfig { provider: ProviderId::Openai, ..Default::default() };
        assert_eq!(openai.model().as_deref(), Some("gpt-4o-mini-tts"));
        let custom = TtsConfig { provider: ProviderId::Custom, ..Default::default() };
        assert_eq!(custom.model(), None);
        assert!(custom.validate().is_ok());
        for bad in [ProviderId::Local, ProviderId::Requesty] {
            assert!(TtsConfig { provider: bad, ..Default::default() }.validate().is_err());
        }
        let blank = TtsConfig { model: Some(" ".into()), voice: Some("".into()), ..Default::default() };
        assert_eq!(blank.model(), c.model());
        assert_eq!(blank.voice(), "coral");
    }

    #[test]
    fn body_per_provider() {
        let b = speech_body(&ep(ProviderId::Openrouter), "openai/gpt-4o-mini-tts-2025-12-15", "sage", "Hallo");
        assert_eq!(b["provider"]["zdr"], true);
        assert_eq!(b["response_format"], "mp3");
        assert_eq!(b["voice"], "sage");
        assert!(b["instructions"].is_string());
        let b = speech_body(&ep(ProviderId::Openai), "gpt-4o-mini-tts", "sage", "Hallo");
        assert!(b.get("provider").is_none());
        let b = speech_body(&ep(ProviderId::Custom), "kokoro", "x", "Hallo");
        assert!(b.get("instructions").is_none() && b.get("provider").is_none());
    }

    #[test]
    fn pieces_cut_at_sentences() {
        assert_eq!(pieces("  Kurz.  ", 10), vec!["Kurz."]);
        assert_eq!(pieces("Eins zwei. Drei vier. Fünf.", 12), vec!["Eins zwei.", "Drei vier.", "Fünf."]);
        assert_eq!(pieces("aaaa bbbb cccc", 10), vec!["aaaa bbbb", "cccc"]);
        assert_eq!(pieces("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        let long = "Ä".repeat(9000);
        assert!(pieces(&long, MAX_CHARS).iter().all(|p| p.chars().count() <= MAX_CHARS));
        assert_eq!(pieces(&long, MAX_CHARS).concat(), long);
    }
}
