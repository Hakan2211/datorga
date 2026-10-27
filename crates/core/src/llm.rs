//! LLM access over the OpenAI-compatible chat API (OpenRouter, Requesty, any custom endpoint).
//!
//! Egress invariant: [`chat`] takes only [`Pseudonymized`] message text, so raw patient text
//! can't be sent. The response comes back tokenized; the caller restores it with its `TokenMap`.
//! The API key lives in the OS keychain (app side). It's never stored in the DB or audit log.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::pseudo::{Dictionary, Pseudonymized, Pseudonymizer};
use crate::{Error, Result};

/// Setting key holding the [`LlmConfig`] JSON.
pub const CONFIG_SETTING: &str = "llm_config";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProviderId {
    Openrouter,
    Requesty,
    /// Any other OpenAI-compatible endpoint (Mistral, a practice gateway, a self-run server).
    Custom,
    /// The bundled llama.cpp server with a downloaded model ([`crate::local`]). Nothing leaves
    /// the machine. Its address and key are new on every start, so they aren't configured.
    Local,
}

/// Fixed facts about a provider, shown in Settings.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ProviderInfo {
    pub id: ProviderId,
    pub label: String,
    pub default_base_url: String,
    /// Where the therapist creates a key.
    pub key_url: Option<String>,
    /// A US company sits between the app and the model: shown as a warning (PLAN-AMENDMENTS A6).
    pub us_intermediary: bool,
    pub needs_key: bool,
}

pub fn providers() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo {
            id: ProviderId::Openrouter,
            label: "OpenRouter".into(),
            default_base_url: "https://openrouter.ai/api/v1".into(),
            key_url: Some("https://openrouter.ai/settings/keys".into()),
            us_intermediary: true,
            needs_key: true,
        },
        ProviderInfo {
            id: ProviderId::Requesty,
            label: "Requesty (EU-Endpunkt)".into(),
            // Frankfurt gateway. Inference stays in the EU only with an EU-region model.
            default_base_url: "https://router.eu.requesty.ai/v1".into(),
            key_url: Some("https://app.requesty.ai/api-keys".into()),
            us_intermediary: true,
            needs_key: true,
        },
        ProviderInfo {
            id: ProviderId::Custom,
            label: "Eigener Endpunkt (OpenAI-kompatibel)".into(),
            default_base_url: String::new(),
            key_url: None,
            us_intermediary: false,
            needs_key: false,
        },
        ProviderInfo {
            id: ProviderId::Local,
            label: "Lokales Modell (auf diesem Computer)".into(),
            default_base_url: String::new(),
            key_url: None,
            us_intermediary: false,
            needs_key: false,
        },
    ]
}

pub fn provider(id: ProviderId) -> ProviderInfo {
    providers().into_iter().find(|p| p.id == id).expect("every ProviderId is listed")
}

/// Per-provider choices. The key is not here (keychain). For [`ProviderId::Local`], `model` is
/// a [`crate::local::MODELS`] id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProviderSettings {
    #[serde(default)]
    pub model: Option<String>,
    /// Overrides `default_base_url` (e.g. OpenRouter's EU host on a business plan).
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LlmConfig {
    /// `None` = AI drafting is off.
    #[serde(default)]
    pub active: Option<ProviderId>,
    #[serde(default)]
    pub providers: BTreeMap<ProviderId, ProviderSettings>,
}

impl LlmConfig {
    pub fn settings(&self, id: ProviderId) -> ProviderSettings {
        self.providers.get(&id).cloned().unwrap_or_default()
    }

    pub fn base_url(&self, id: ProviderId) -> String {
        self.settings(id)
            .base_url
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| provider(id).default_base_url)
    }
}

/// Where a request goes. Built by the app from the config plus the keychain key.
#[derive(Clone)]
pub struct Endpoint {
    pub provider: ProviderId,
    pub base_url: String,
    pub api_key: Option<String>,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .finish()
    }
}

impl Endpoint {
    fn url(&self, path: &str) -> Result<String> {
        let base = self.base_url.trim().trim_end_matches('/');
        let local = is_loopback(base);
        if !(base.starts_with("https://") || (local && base.starts_with("http://"))) {
            return Err(Error::Invalid("Endpunkt muss mit https:// beginnen (http nur für localhost)".into()));
        }
        Ok(format!("{base}{path}"))
    }

    fn agent(&self, timeout: Duration) -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
            .into()
    }

    fn auth<B>(&self, req: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        let req = match &self.api_key {
            Some(k) if !k.is_empty() => req.header("Authorization", format!("Bearer {k}")),
            _ => req,
        };
        match self.provider {
            // OpenRouter attributes traffic to the app with these two headers (no user data).
            ProviderId::Openrouter => req.header("HTTP-Referer", "https://datorga.de").header("X-Title", "thera"),
            _ => req,
        }
    }
}

fn is_loopback(base: &str) -> bool {
    let rest = base.split("://").nth(1).unwrap_or("");
    let host = if rest.starts_with("[::1]") { "[::1]" } else { rest.split(['/', ':']).next().unwrap_or("") };
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

// ---------- models ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct RemoteModel {
    pub id: String,
    pub name: Option<String>,
    pub context_length: Option<u32>,
}

/// `GET /models`. Sends no patient data.
pub fn list_models(ep: &Endpoint) -> Result<Vec<RemoteModel>> {
    let mut resp = ep
        .auth(ep.agent(Duration::from_secs(30)).get(&ep.url("/models")?))
        .call()
        .map_err(net)?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_to_string().map_err(net)?;
    if status != 200 {
        return Err(Error::Llm(api_error(status, &body)));
    }
    let v: Value = serde_json::from_str(&body)?;
    let list = v.get("data").and_then(Value::as_array).ok_or_else(|| Error::Llm("Unerwartete Antwort (kein data-Feld)".into()))?;
    let mut models: Vec<RemoteModel> = list
        .iter()
        .filter_map(|m| {
            Some(RemoteModel {
                id: m.get("id")?.as_str()?.to_string(),
                name: m.get("name").and_then(Value::as_str).map(str::to_string),
                context_length: m.get("context_length").and_then(Value::as_u64).map(|n| n.min(u32::MAX as u64) as u32),
            })
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

// ---------- chat ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy)]
pub struct ChatOptions {
    pub temperature: f32,
    pub max_tokens: u32,
    pub timeout: Duration,
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self { temperature: 0.2, max_tokens: 2048, timeout: Duration::from_secs(180) }
    }
}

/// A model answer, still tokenized (`[PERSON_1]`…).
#[derive(Debug, Clone)]
pub struct Completion {
    pub text: String,
    /// The model that answered (routers may report a more specific id).
    pub model: String,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub latency_ms: u64,
}

/// The request body. Message text must have passed the pseudonymizer.
pub fn chat_body(ep: &Endpoint, model: &str, messages: &[(Role, &Pseudonymized<String>)], opts: &ChatOptions) -> Value {
    let msgs: Vec<Value> = messages.iter().map(|(r, t)| json!({ "role": r, "content": t.get() })).collect();
    let mut body = json!({
        "model": model,
        "messages": msgs,
        "temperature": opts.temperature,
        "max_tokens": opts.max_tokens,
        "stream": false,
    });
    if ep.provider == ProviderId::Openrouter {
        // Only zero-retention endpoints that don't collect data. No match = error, not a fallback.
        body["provider"] = json!({ "zdr": true, "data_collection": "deny" });
    }
    body
}

/// `POST /chat/completions`.
pub fn chat(ep: &Endpoint, model: &str, messages: &[(Role, &Pseudonymized<String>)], opts: &ChatOptions) -> Result<Completion> {
    if model.trim().is_empty() {
        return Err(Error::Invalid("Kein Modell gewählt".into()));
    }
    let body = chat_body(ep, model, messages, opts).to_string();
    let started = Instant::now();
    let mut resp = ep
        .auth(ep.agent(opts.timeout).post(&ep.url("/chat/completions")?))
        .header("Content-Type", "application/json")
        .send(body)
        .map_err(net)?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().with_config().limit(16 << 20).read_to_string().map_err(net)?;
    let latency_ms = started.elapsed().as_millis() as u64;
    if status != 200 {
        return Err(Error::Llm(api_error(status, &text)));
    }
    let v: Value = serde_json::from_str(&text)?;
    // Some routers answer 200 with an error object.
    if v.get("error").is_some() {
        return Err(Error::Llm(api_error(status, &text)));
    }
    let content = v
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Llm("Antwort ohne Text".into()))?;
    let usage = |k: &str| v.pointer(&format!("/usage/{k}")).and_then(Value::as_u64).map(|n| n as u32);
    Ok(Completion {
        text: content.to_string(),
        model: v.get("model").and_then(Value::as_str).unwrap_or(model).to_string(),
        prompt_tokens: usage("prompt_tokens"),
        completion_tokens: usage("completion_tokens"),
        latency_ms,
    })
}

/// Connection test with a fixed German prompt (no patient data).
pub fn ping(ep: &Endpoint, model: &str) -> Result<Completion> {
    let mut p = Pseudonymizer::new(Dictionary::new());
    let msg = p.pseudonymize("Antworte nur mit dem Wort: OK")?;
    chat(ep, model, &[(Role::User, &msg)], &ChatOptions { max_tokens: 16, timeout: Duration::from_secs(60), ..Default::default() })
}

fn net(e: ureq::Error) -> Error {
    Error::Llm(format!("Verbindung fehlgeschlagen: {e}"))
}

/// A short German message from an error response. Never echoes request content.
fn api_error(status: u16, body: &str) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("message"))
                .or_else(|| v.get("error"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    let what = match status {
        401 | 403 => "API-Schlüssel ungültig oder ohne Berechtigung",
        402 => "Guthaben beim Anbieter aufgebraucht",
        404 => "Modell oder Endpunkt nicht gefunden",
        429 => "Zu viele Anfragen, bitte später erneut",
        500..=599 => "Fehler beim Anbieter",
        _ => "Anfrage abgelehnt",
    };
    let detail: String = detail.chars().take(300).collect();
    if detail.is_empty() {
        format!("{what} (HTTP {status})")
    } else {
        format!("{what} (HTTP {status}): {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(provider: ProviderId, base: &str) -> Endpoint {
        Endpoint { provider, base_url: base.into(), api_key: Some("sk-test".into()) }
    }

    #[test]
    fn config_defaults_and_overrides() {
        let mut c = LlmConfig::default();
        assert_eq!(c.base_url(ProviderId::Requesty), "https://router.eu.requesty.ai/v1");
        c.providers.insert(ProviderId::Openrouter, ProviderSettings { model: None, base_url: Some("  ".into()) });
        assert_eq!(c.base_url(ProviderId::Openrouter), "https://openrouter.ai/api/v1");
        c.providers.insert(ProviderId::Openrouter, ProviderSettings { model: None, base_url: Some("https://eu.openrouter.ai/api/v1".into()) });
        assert_eq!(c.base_url(ProviderId::Openrouter), "https://eu.openrouter.ai/api/v1");
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(serde_json::from_value::<LlmConfig>(json).unwrap(), c);
    }

    #[test]
    fn plain_http_only_for_localhost() {
        assert!(ep(ProviderId::Custom, "http://example.com/v1").url("/models").is_err());
        assert!(ep(ProviderId::Custom, "ftp://x").url("/models").is_err());
        assert_eq!(ep(ProviderId::Custom, "http://127.0.0.1:8080/v1/").url("/models").unwrap(), "http://127.0.0.1:8080/v1/models");
        assert!(ep(ProviderId::Custom, "http://localhost:8080/v1").url("/models").is_ok());
        assert!(ep(ProviderId::Custom, "https://api.mistral.ai/v1").url("/models").is_ok());
    }

    #[test]
    fn openrouter_body_demands_zero_retention() {
        let mut p = Pseudonymizer::new(Dictionary::new());
        let m = p.pseudonymize("Hallo").unwrap();
        let b = chat_body(&ep(ProviderId::Openrouter, "https://openrouter.ai/api/v1"), "x/y", &[(Role::User, &m)], &ChatOptions::default());
        assert_eq!(b["provider"]["zdr"], true);
        assert_eq!(b["provider"]["data_collection"], "deny");
        assert_eq!(b["messages"][0]["role"], "user");
        let b = chat_body(&ep(ProviderId::Requesty, "https://router.eu.requesty.ai/v1"), "x/y", &[(Role::User, &m)], &ChatOptions::default());
        assert!(b.get("provider").is_none());
    }

    #[test]
    fn errors_are_short_and_german() {
        assert_eq!(api_error(401, r#"{"error":{"message":"No auth"}}"#), "API-Schlüssel ungültig oder ohne Berechtigung (HTTP 401): No auth");
        assert_eq!(api_error(500, "<html>"), "Fehler beim Anbieter (HTTP 500)");
    }

    #[test]
    fn debug_hides_the_key() {
        assert!(!format!("{:?}", ep(ProviderId::Openrouter, "https://x")).contains("sk-test"));
    }
}
