//! LLM access over the OpenAI-compatible chat API (OpenRouter, Requesty, any custom endpoint).
//!
//! Egress invariant: [`chat`] takes only [`Pseudonymized`] message text, so raw patient text
//! can't be sent. The response comes back tokenized; the caller restores it with its `TokenMap`.
//! The API key lives in the OS keychain (app side). It's never stored in the DB or audit log.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
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
    /// Any other OpenAI-compatible endpoint (a practice gateway, a self-run server).
    Custom,
    /// The bundled llama.cpp server with a downloaded model ([`crate::local`]). Nothing leaves
    /// the machine. Its address and key are new on every start, so they aren't configured.
    Local,
    /// OpenAI directly, with its own key: only for voice (Realtime prep, read-aloud), never
    /// for drafting. A US provider; an EU data-residency project uses [`OPENAI_EU_BASE_URL`].
    Openai,
}

pub const OPENAI_EU_BASE_URL: &str = "https://eu.api.openai.com/v1";

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
    /// Offered for drafting and extraction ([`LlmConfig::active`]).
    pub drafting: bool,
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
            drafting: true,
        },
        ProviderInfo {
            id: ProviderId::Requesty,
            label: "Requesty (EU-Endpunkt)".into(),
            // Frankfurt gateway. Inference stays in the EU only with an EU-region model.
            default_base_url: "https://router.eu.requesty.ai/v1".into(),
            key_url: Some("https://app.requesty.ai/api-keys".into()),
            us_intermediary: true,
            needs_key: true,
            drafting: true,
        },
        ProviderInfo {
            id: ProviderId::Custom,
            label: "Eigener Endpunkt (OpenAI-kompatibel)".into(),
            default_base_url: String::new(),
            key_url: None,
            us_intermediary: false,
            needs_key: false,
            drafting: true,
        },
        ProviderInfo {
            id: ProviderId::Local,
            label: "Lokales Modell (auf diesem Computer)".into(),
            default_base_url: String::new(),
            key_url: None,
            us_intermediary: false,
            needs_key: false,
            drafting: true,
        },
        ProviderInfo {
            id: ProviderId::Openai,
            label: "OpenAI (direkt)".into(),
            default_base_url: "https://api.openai.com/v1".into(),
            key_url: Some("https://platform.openai.com/api-keys".into()),
            // Not an intermediary but the US provider itself; Settings warns separately.
            us_intermediary: false,
            needs_key: true,
            drafting: false,
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
    pub fn validate(&self) -> Result<()> {
        match self.active {
            Some(id) if !provider(id).drafting => Err(Error::Invalid(format!("{} ist nicht für Entwürfe vorgesehen", provider(id).label))),
            _ => Ok(()),
        }
    }

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
    pub(crate) fn url(&self, path: &str) -> Result<String> {
        let base = self.base_url.trim().trim_end_matches('/');
        let local = is_loopback(base);
        if !(base.starts_with("https://") || (local && base.starts_with("http://"))) {
            return Err(Error::Invalid("Endpunkt muss mit https:// beginnen (http nur für localhost)".into()));
        }
        Ok(format!("{base}{path}"))
    }

    pub(crate) fn agent(&self, timeout: Duration) -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
            .into()
    }

    pub(crate) fn auth<B>(&self, req: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
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
    /// Takes `tools` (from `supported_parameters`); `None` when the endpoint doesn't say.
    pub tools: Option<bool>,
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
                tools: m
                    .get("supported_parameters")
                    .and_then(Value::as_array)
                    .map(|ps| ps.iter().any(|p| p.as_str() == Some("tools"))),
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
    Tool,
}

/// A tool call the model asked for. `arguments` is the model's JSON text, still tokenized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// An earlier tool call sent back with the conversation. The arguments pass the pseudonymizer
/// again like every other text (they are stored restored).
#[derive(Debug, Clone)]
pub struct SentCall {
    pub id: String,
    pub name: String,
    pub arguments: Pseudonymized<String>,
}

/// One message of a conversation with tools ([`chat_stream_tools`]).
#[derive(Debug, Clone)]
pub enum Message {
    Text(Role, Pseudonymized<String>),
    /// An assistant turn that called tools.
    Calls { content: Option<Pseudonymized<String>>, calls: Vec<SentCall> },
    /// What a tool returned.
    ToolResult { call_id: String, content: Pseudonymized<String> },
}

impl Message {
    fn json(&self) -> Value {
        match self {
            Message::Text(role, text) => json!({ "role": role, "content": text.get() }),
            Message::Calls { content, calls } => json!({
                "role": "assistant",
                "content": content.as_ref().map(|c| c.get()),
                "tool_calls": calls.iter().map(|c| json!({
                    "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.arguments.get() },
                })).collect::<Vec<_>>(),
            }),
            Message::ToolResult { call_id, content } => json!({ "role": "tool", "tool_call_id": call_id, "content": content.get() }),
        }
    }
}

/// How much the model may think before answering. Set per task by the app, never by the therapist.
/// Reasoning tokens are billed as output and come out of `max_tokens` (OpenRouter gives low ≈ 20 %,
/// medium ≈ 50 %, high ≈ 80 % of it), so tasks size `max_tokens` with that in mind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy)]
pub struct ChatOptions {
    pub temperature: f32,
    pub max_tokens: u32,
    pub timeout: Duration,
    /// Sent to OpenRouter only. `None` leaves the model's default, which for some models is "max".
    pub reasoning: Option<Effort>,
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self { temperature: 0.2, max_tokens: 2048, timeout: Duration::from_secs(180), reasoning: None }
    }
}

/// A model answer, still tokenized (`[PERSON_1]`…).
#[derive(Debug, Clone)]
pub struct Completion {
    pub text: String,
    /// The model that answered (routers may report a more specific id).
    pub model: String,
    pub prompt_tokens: Option<u32>,
    /// Includes reasoning tokens.
    pub completion_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
    /// What the router billed in USD (OpenRouter reports it; others don't).
    pub cost: Option<f64>,
    pub latency_ms: u64,
    /// Tools the model wants run (only with [`chat_stream_tools`]); `text` may then be empty.
    pub tool_calls: Vec<ToolCall>,
}

/// The request body. Message text must have passed the pseudonymizer.
pub fn chat_body(ep: &Endpoint, model: &str, messages: &[(Role, &Pseudonymized<String>)], opts: &ChatOptions) -> Value {
    let msgs: Vec<Value> = messages.iter().map(|(r, t)| json!({ "role": r, "content": t.get() })).collect();
    request_body(ep, model, msgs, opts)
}

/// The request body with tools. `tools` are the app's own definitions (no patient data).
pub fn tools_body(ep: &Endpoint, model: &str, messages: &[Message], tools: &Value, opts: &ChatOptions) -> Value {
    let mut body = request_body(ep, model, messages.iter().map(Message::json).collect(), opts);
    // No `provider.require_parameters` for OpenRouter: it demands every parameter, and models that
    // reject `temperature` (Claude, GPT reasoning models) then have no endpoint at all (404). The
    // UI warns when the chosen model doesn't list `tools` instead.
    body["tools"] = tools.clone();
    body
}

fn request_body(ep: &Endpoint, model: &str, msgs: Vec<Value>, opts: &ChatOptions) -> Value {
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
        if let Some(wanted) = opts.reasoning {
            // The reasoning text isn't needed and shouldn't travel back; it is billed either way.
            let effort = crate::catalog::effort_for(model, wanted);
            body["reasoning"] = json!({ "effort": effort, "exclude": true });
        }
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
    json_completion(status, &text, model, started)
}

/// A complete (non-streamed) answer.
fn json_completion(status: u16, text: &str, model: &str, started: Instant) -> Result<Completion> {
    let latency_ms = started.elapsed().as_millis() as u64;
    if status != 200 {
        return Err(Error::Llm(api_error(status, text)));
    }
    let v: Value = serde_json::from_str(text)?;
    // Some routers answer 200 with an error object.
    if v.get("error").is_some() {
        return Err(Error::Llm(api_error(status, text)));
    }
    let content = v.pointer("/choices/0/message/content").and_then(Value::as_str).unwrap_or("");
    let finish = v.pointer("/choices/0/finish_reason").and_then(Value::as_str);
    let calls = v
        .pointer("/choices/0/message/tool_calls")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|c| ToolCall {
                    id: c.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
                    name: c.pointer("/function/name").and_then(Value::as_str).unwrap_or_default().to_string(),
                    arguments: c.pointer("/function/arguments").and_then(Value::as_str).unwrap_or_default().to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    completion(content, finish, calls, &v, model, latency_ms)
}

/// Checks the answer text and reads model and usage from `v` (the response, or the stream chunk
/// that carried the usage).
fn completion(content: &str, finish: Option<&str>, tool_calls: Vec<ToolCall>, v: &Value, model: &str, latency_ms: u64) -> Result<Completion> {
    if content.trim().is_empty() && tool_calls.is_empty() {
        return Err(Error::Llm(if finish == Some("length") {
            "Antwort abgeschnitten: das Modell hat sein Token-Budget mit Nachdenken aufgebraucht".into()
        } else {
            "Antwort ohne Text".into()
        }));
    }
    let usage = |k: &str| v.pointer(&format!("/usage/{k}")).and_then(Value::as_u64).map(|n| n as u32);
    Ok(Completion {
        text: content.to_string(),
        model: v.get("model").and_then(Value::as_str).unwrap_or(model).to_string(),
        prompt_tokens: usage("prompt_tokens"),
        completion_tokens: usage("completion_tokens"),
        reasoning_tokens: usage("completion_tokens_details/reasoning_tokens"),
        cost: v.pointer("/usage/cost").and_then(Value::as_f64),
        latency_ms,
        tool_calls,
    })
}

enum StreamEvent {
    Delta(String),
    Done(Result<Completion>),
}

/// `POST /chat/completions` with `stream: true` (server-sent events). `on_delta` gets each piece
/// of answer text as it arrives; the result is the same [`Completion`] [`chat`] returns.
///
/// Setting `cancel` returns [`Error::Cancelled`] within ~100 ms, also while the server is still
/// silent (prompt processing). The request runs on its own thread, which drops the connection at
/// the next line it reads, so the provider stops generating. An endpoint that ignores `stream`
/// and answers with plain JSON works too.
pub fn chat_stream(
    ep: &Endpoint,
    model: &str,
    messages: &[(Role, &Pseudonymized<String>)],
    opts: &ChatOptions,
    cancel: &AtomicBool,
    on_delta: impl FnMut(&str),
) -> Result<Completion> {
    if model.trim().is_empty() {
        return Err(Error::Invalid("Kein Modell gewählt".into()));
    }
    stream_body(ep, model, chat_body(ep, model, messages, opts), opts, cancel, on_delta)
}

/// [`chat_stream`] with tools: the answer is text, [`Completion::tool_calls`], or both.
pub fn chat_stream_tools(
    ep: &Endpoint,
    model: &str,
    messages: &[Message],
    tools: &Value,
    opts: &ChatOptions,
    cancel: &AtomicBool,
    on_delta: impl FnMut(&str),
) -> Result<Completion> {
    if model.trim().is_empty() {
        return Err(Error::Invalid("Kein Modell gewählt".into()));
    }
    stream_body(ep, model, tools_body(ep, model, messages, tools, opts), opts, cancel, on_delta)
}

fn stream_body(ep: &Endpoint, model: &str, mut body: Value, opts: &ChatOptions, cancel: &AtomicBool, mut on_delta: impl FnMut(&str)) -> Result<Completion> {
    body["stream"] = json!(true);
    // Usage (and OpenRouter's cost) come in a last chunk only when asked for.
    body["stream_options"] = json!({ "include_usage": true });
    let url = ep.url("/chat/completions")?;
    let (ep, model_id, timeout) = (ep.clone(), model.to_string(), opts.timeout);
    let gone = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    {
        let gone = gone.clone();
        std::thread::spawn(move || {
            let result = stream_request(&ep, &url, body.to_string(), &model_id, timeout, &gone, &tx);
            let _ = tx.send(StreamEvent::Done(result));
        });
    }
    let result = loop {
        if cancel.load(Ordering::Relaxed) {
            break Err(Error::Cancelled);
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(StreamEvent::Delta(t)) => on_delta(&t),
            Ok(StreamEvent::Done(r)) => break r,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break Err(Error::Llm("Verbindung abgebrochen".into())),
        }
    };
    gone.store(true, Ordering::Relaxed);
    result
}

/// The streaming request itself, on the helper thread of [`chat_stream`].
fn stream_request(
    ep: &Endpoint,
    url: &str,
    body: String,
    model: &str,
    timeout: Duration,
    gone: &AtomicBool,
    tx: &mpsc::Sender<StreamEvent>,
) -> Result<Completion> {
    let started = Instant::now();
    let mut resp = ep
        .auth(ep.agent(timeout).post(url))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .send(body)
        .map_err(net)?;
    let status = resp.status().as_u16();
    if status != 200 || resp.body().mime_type() != Some("text/event-stream") {
        let text = resp.body_mut().with_config().limit(16 << 20).read_to_string().map_err(net)?;
        let c = json_completion(status, &text, model, started)?;
        let _ = tx.send(StreamEvent::Delta(c.text.clone()));
        return Ok(c);
    }
    let mut reader = BufReader::new(resp.body_mut().with_config().limit(16 << 20).reader());
    let (mut text, mut finish, mut meta) = (String::new(), None::<String>, Value::Null);
    // Tool calls arrive in pieces keyed by `index`: id and name first, the arguments in fragments.
    let mut calls: Vec<ToolCall> = Vec::new();
    let mut raw = Vec::new();
    loop {
        raw.clear();
        if reader.read_until(b'\n', &mut raw).map_err(|e| Error::Llm(format!("Verbindung fehlgeschlagen: {e}")))? == 0 {
            break;
        }
        if gone.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let line = String::from_utf8_lossy(&raw);
        // Other lines are event separators or keep-alive comments (": OPENROUTER PROCESSING").
        let Some(data) = line.trim_end().strip_prefix("data:").map(str::trim_start) else { continue };
        if data == "[DONE]" {
            break;
        }
        let v: Value = serde_json::from_str(data)?;
        // An error after the stream started comes as a chunk with an error object.
        if v.get("error").is_some() {
            return Err(Error::Llm(api_error(status, data)));
        }
        if let Some(t) = v.pointer("/choices/0/delta/content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
            text.push_str(t);
            let _ = tx.send(StreamEvent::Delta(t.to_string()));
        }
        for d in v.pointer("/choices/0/delta/tool_calls").and_then(Value::as_array).into_iter().flatten() {
            let i = d.get("index").and_then(Value::as_u64).map_or(calls.len().saturating_sub(1), |i| i as usize);
            if i >= calls.len() {
                calls.resize(i + 1, ToolCall { id: String::new(), name: String::new(), arguments: String::new() });
            }
            let call = &mut calls[i];
            if let Some(id) = d.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                call.id = id.to_string();
            }
            if let Some(n) = d.pointer("/function/name").and_then(Value::as_str) {
                call.name.push_str(n);
            }
            if let Some(a) = d.pointer("/function/arguments").and_then(Value::as_str) {
                call.arguments.push_str(a);
            }
        }
        if let Some(f) = v.pointer("/choices/0/finish_reason").and_then(Value::as_str) {
            finish = Some(f.to_string());
        }
        if meta.is_null() || v.get("usage").is_some_and(|u| !u.is_null()) {
            meta = v;
        }
    }
    calls.retain(|c| !c.name.is_empty());
    completion(&text, finish.as_deref(), calls, &meta, model, started.elapsed().as_millis() as u64)
}

/// Connection test with a fixed German prompt (no patient data).
pub fn ping(ep: &Endpoint, model: &str) -> Result<Completion> {
    let mut p = Pseudonymizer::new(Dictionary::new());
    let msg = p.pseudonymize("Antworte nur mit dem Wort: OK")?;
    chat(ep, model, &[(Role::User, &msg)], &ChatOptions {
        // Some models always reason (Anthropic needs a budget of at least 1024), so 16 tokens would come back empty.
        max_tokens: 2048,
        timeout: Duration::from_secs(60),
        reasoning: Some(Effort::Low),
        ..Default::default()
    })
}

pub(crate) fn net(e: ureq::Error) -> Error {
    Error::Llm(format!("Verbindung fehlgeschlagen: {e}"))
}

/// A short German message from an error response. Never echoes request content.
pub(crate) fn api_error(status: u16, body: &str) -> String {
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
        assert!(c.validate().is_ok());
        c.active = Some(ProviderId::Openai);
        assert!(c.validate().is_err(), "OpenAI is for voice only");
        c.active = Some(ProviderId::Requesty);
        assert!(c.validate().is_ok());
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(serde_json::from_value::<LlmConfig>(json).unwrap(), c);
    }

    #[test]
    fn plain_http_only_for_localhost() {
        assert!(ep(ProviderId::Custom, "http://example.com/v1").url("/models").is_err());
        assert!(ep(ProviderId::Custom, "ftp://x").url("/models").is_err());
        assert_eq!(ep(ProviderId::Custom, "http://127.0.0.1:8080/v1/").url("/models").unwrap(), "http://127.0.0.1:8080/v1/models");
        assert!(ep(ProviderId::Custom, "http://localhost:8080/v1").url("/models").is_ok());
        assert!(ep(ProviderId::Custom, "https://api.example.eu/v1").url("/models").is_ok());
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
    fn reasoning_effort_goes_to_openrouter_only() {
        let mut p = Pseudonymizer::new(Dictionary::new());
        let m = p.pseudonymize("Hallo").unwrap();
        let opts = ChatOptions { reasoning: Some(Effort::Medium), ..Default::default() };
        let b = chat_body(&ep(ProviderId::Openrouter, "https://openrouter.ai/api/v1"), "x/y", &[(Role::User, &m)], &opts);
        assert_eq!(b["reasoning"]["effort"], "medium");
        assert_eq!(b["reasoning"]["exclude"], true);
        let b = chat_body(&ep(ProviderId::Openrouter, "https://openrouter.ai/api/v1"), "x/y", &[(Role::User, &m)], &ChatOptions::default());
        assert!(b.get("reasoning").is_none(), "no effort set = model default");
        for other in [ProviderId::Requesty, ProviderId::Custom, ProviderId::Local] {
            let b = chat_body(&ep(other, "http://127.0.0.1:1/v1"), "x/y", &[(Role::User, &m)], &opts);
            assert!(b.get("reasoning").is_none(), "{other:?}");
        }
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
