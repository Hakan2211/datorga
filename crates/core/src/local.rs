//! Local language models: a pinned GGUF catalog, a recommendation from the machine's RAM and GPU,
//! and the llama.cpp `llama-server` sidecar that serves the chosen model.
//!
//! The server speaks the same OpenAI-compatible API as the cloud providers, so every request
//! still goes through the [`llm::chat`](crate::llm::chat) egress gate. It listens on 127.0.0.1
//! only, on a free port, and demands an API key that is new for every start, so other programs
//! on this machine can't use it. Prompts stay in its memory: no prompt cache, no slot dump, no
//! web UI, no log file.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use ts_rs::TS;

use crate::llm::{Endpoint, ProviderId};
use crate::models::ModelInfo;
use crate::{Error, Result};

pub const ENGINE: &str = "llama.cpp";

const GIB: u64 = 1 << 30;

// ---------- catalog ----------

/// A GGUF model the app can download and serve.
#[derive(Debug, Clone, Copy)]
pub struct LocalModel {
    pub file: ModelInfo,
    pub label: &'static str,
    /// Context the server is started with (tokens). A 50-minute session is ~12k tokens.
    pub ctx: u32,
    /// KV cache at `ctx` (f16): 2 × layers × KV heads × head dim × 2 bytes × ctx.
    pub kv_bytes: u64,
    /// Hybrid thinking model: started with thinking off (drafts don't need it, and it is slow).
    pub reasoning: bool,
    /// Fast enough without a GPU. Dense 24B models answer at ~2 tokens/s on a laptop CPU.
    pub cpu_ok: bool,
    /// Noticeably weaker German and instruction following: drafts need more correction.
    pub experimental: bool,
}

/// Biggest first: [`recommend`] takes the first one that fits.
pub const MODELS: &[LocalModel] = &[
    LocalModel {
        file: ModelInfo {
            id: "mistral-small-3.2-24b",
            file: "Mistral-Small-3.2-24B-Instruct-2506-Q4_K_M.gguf",
            url: "https://huggingface.co/unsloth/Mistral-Small-3.2-24B-Instruct-2506-GGUF/resolve/main/Mistral-Small-3.2-24B-Instruct-2506-Q4_K_M.gguf",
            sha256: "a3cc56310807ed0d145eaf9f018ccda9ae7ad8edb41ec870aa2454b0d4700b3c",
            bytes: 14_333_922_848,
        },
        label: "Mistral Small 3.2 (24B)",
        ctx: 16_384,
        kv_bytes: 2 * 40 * 8 * 128 * 2 * 16_384,
        reasoning: false,
        cpu_ok: false,
        experimental: false,
    },
    LocalModel {
        file: ModelInfo {
            id: "qwen3-14b",
            file: "Qwen3-14B-Q4_K_M.gguf",
            url: "https://huggingface.co/Qwen/Qwen3-14B-GGUF/resolve/main/Qwen3-14B-Q4_K_M.gguf",
            sha256: "500a8806e85ee9c83f3ae08420295592451379b4f8cf2d0f41c15dffeb6b81f0",
            bytes: 9_001_752_960,
        },
        label: "Qwen3 (14B)",
        ctx: 16_384,
        kv_bytes: 2 * 40 * 8 * 128 * 2 * 16_384,
        reasoning: true,
        cpu_ok: true,
        experimental: false,
    },
    LocalModel {
        file: ModelInfo {
            id: "qwen3-8b",
            file: "Qwen3-8B-Q4_K_M.gguf",
            url: "https://huggingface.co/Qwen/Qwen3-8B-GGUF/resolve/main/Qwen3-8B-Q4_K_M.gguf",
            sha256: "d98cdcbd03e17ce47681435b5150e34c1417f50b5c0019dd560e4882c5745785",
            bytes: 5_027_783_488,
        },
        label: "Qwen3 (8B)",
        ctx: 16_384,
        kv_bytes: 2 * 36 * 8 * 128 * 2 * 16_384,
        reasoning: true,
        cpu_ok: true,
        experimental: true,
    },
];

pub fn model(id: &str) -> Result<&'static LocalModel> {
    MODELS.iter().find(|m| m.file.id == id).ok_or_else(|| Error::Invalid(format!("unknown model {id}")))
}

// ---------- hardware fit ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Gpu {
    pub name: String,
    /// Dedicated video memory. Integrated GPUs report little or none.
    #[ts(type = "number")]
    pub vram_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Hardware {
    #[ts(type = "number")]
    pub ram_bytes: u64,
    pub cpu_threads: u32,
    pub gpus: Vec<Gpu>,
}

/// Where a model would run on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Fit {
    /// Weights and context fit into the GPU's memory: fast.
    Gpu,
    /// Part on the GPU, the rest on the CPU.
    Partial,
    /// CPU and RAM only: slow.
    Cpu,
    TooBig,
}

/// RAM left for Windows, thera, a browser and the speech model.
const RAM_HEADROOM: u64 = 6 * GIB;
/// VRAM kept free for the display and the compute buffers.
const VRAM_MARGIN: u64 = GIB;
/// Below this a GPU isn't worth offloading part of a model to.
const PARTIAL_MIN_VRAM: u64 = 4 * GIB;
/// "32 GB" machines report ~31.7 GiB (some is reserved for hardware). Below this, local drafting
/// is experimental (PLAN-AMENDMENTS A5) unless the model runs fully on the GPU.
const RECOMMENDED_RAM: u64 = 30 * GIB;

pub fn fit(hw: &Hardware, m: &LocalModel) -> Fit {
    let need = m.file.bytes + m.kv_bytes;
    let vram = hw.gpus.iter().map(|g| g.vram_bytes).max().unwrap_or(0);
    if need + VRAM_MARGIN <= vram && hw.ram_bytes >= 8 * GIB {
        Fit::Gpu
    } else if need + RAM_HEADROOM <= hw.ram_bytes {
        if vram >= PARTIAL_MIN_VRAM {
            Fit::Partial
        } else {
            Fit::Cpu
        }
    } else {
        Fit::TooBig
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Recommendation {
    /// `None`: no catalog model runs acceptably here; use a cloud provider.
    pub model: Option<String>,
    /// Works, but expect slow answers or weaker drafts.
    pub experimental: bool,
}

/// The biggest model that fits the GPU entirely; otherwise the biggest one fast enough on the CPU.
pub fn recommend(hw: &Hardware) -> Recommendation {
    let pick = MODELS
        .iter()
        .find(|m| fit(hw, m) == Fit::Gpu)
        .or_else(|| MODELS.iter().find(|m| m.cpu_ok && matches!(fit(hw, m), Fit::Partial | Fit::Cpu)));
    match pick {
        Some(m) => Recommendation {
            model: Some(m.file.id.to_string()),
            experimental: m.experimental || (fit(hw, m) != Fit::Gpu && hw.ram_bytes < RECOMMENDED_RAM),
        },
        None => Recommendation { model: None, experimental: false },
    }
}

// ---------- sidecar ----------

/// Unloads the model after this much idle time; the next request loads it again.
const SLEEP_IDLE_SECS: u32 = 300;

/// `llama-server` arguments. The API key goes through the environment, not the command line.
pub fn server_args(model_path: &Path, m: &LocalModel, port: u16) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["-m".into(), model_path.into()];
    for a in [
        "--alias", m.file.id,
        "--host", "127.0.0.1",
        "--port", &port.to_string(),
        "--ctx-size", &m.ctx.to_string(),
        "--parallel", "1",
        "--no-webui",
        "--no-slots",
        "--offline",
        // The host-side prompt cache would keep earlier prompts in RAM.
        "--cache-ram", "0",
        "--sleep-idle-seconds", &SLEEP_IDLE_SECS.to_string(),
    ] {
        args.push(a.into());
    }
    if m.reasoning {
        args.extend(["--reasoning-budget".into(), "0".into()]);
    }
    args
}

/// A running `llama-server`. Dropping it kills the process.
pub struct LlamaServer {
    child: Child,
    port: u16,
    api_key: String,
    pub model_id: &'static str,
    /// Last lines of the server log, for error messages. Kept in memory only.
    log: Arc<Mutex<VecDeque<String>>>,
}

impl LlamaServer {
    /// Starts the server on a free loopback port. Call [`wait_ready`](Self::wait_ready) next.
    pub fn spawn(exe: &Path, model_path: &Path, m: &'static LocalModel) -> Result<Self> {
        if !model_path.exists() {
            return Err(Error::Llm("Das lokale Modell ist nicht installiert".into()));
        }
        let port = TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port();
        let mut raw = [0u8; 24];
        getrandom::fill(&mut raw).map_err(|e| Error::Crypto(e.to_string()))?;
        let api_key = hex::encode(raw);

        let mut cmd = Command::new(exe);
        cmd.args(server_args(model_path, m, port));
        // Inherited LLAMA_ARG_* variables would override our flags (host, log file, …).
        for (k, _) in std::env::vars_os() {
            if k.to_string_lossy().to_ascii_uppercase().starts_with("LLAMA_") {
                cmd.env_remove(&k);
            }
        }
        cmd.env("LLAMA_API_KEY", &api_key).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| Error::Llm(format!("Lokaler KI-Server startet nicht: {e}")))?;

        let log = Arc::new(Mutex::new(VecDeque::new()));
        let stderr = child.stderr.take().expect("piped");
        let sink = log.clone();
        // Drains the pipe for the life of the process; a full pipe would block the server.
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(std::io::Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(mut l) = sink.lock() {
                    l.push_back(line);
                    if l.len() > 8 {
                        l.pop_front();
                    }
                }
            }
        });
        Ok(Self { child, port, api_key, model_id: m.file.id, log })
    }

    /// The process, e.g. to put it into a job object that dies with the app.
    pub fn child(&self) -> &Child {
        &self.child
    }

    /// Waits until the model is loaded (`GET /health` answers 200). Kills the server on timeout,
    /// on `cancel`, or reports its last log lines when it exits by itself.
    pub fn wait_ready(&mut self, timeout: Duration, cancel: &AtomicBool) -> Result<()> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(2)))
            .http_status_as_error(false)
            .build()
            .into();
        let url = format!("http://127.0.0.1:{}/health", self.port);
        let started = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                self.kill();
                return Err(Error::Cancelled);
            }
            if let Some(status) = self.child.try_wait()? {
                // Give the log reader a moment to collect the final lines.
                std::thread::sleep(Duration::from_millis(200));
                return Err(Error::Llm(format!("Lokales Modell konnte nicht geladen werden ({status}): {}", self.log_tail())));
            }
            if agent.get(&url).call().is_ok_and(|r| r.status() == 200) {
                return Ok(());
            }
            if started.elapsed() > timeout {
                self.kill();
                return Err(Error::Llm("Das lokale Modell lädt zu lange; bitte erneut versuchen".into()));
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    pub fn endpoint(&self) -> Endpoint {
        Endpoint {
            provider: ProviderId::Local,
            base_url: format!("http://127.0.0.1:{}/v1", self.port),
            api_key: Some(self.api_key.clone()),
        }
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn log_tail(&self) -> String {
        let lines = self.log.lock().map(|l| l.iter().cloned().collect::<Vec<_>>().join(" | ")).unwrap_or_default();
        let n = lines.chars().count();
        lines.chars().skip(n.saturating_sub(300)).collect()
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for LlamaServer {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hw(ram_gib: u64, vram_gib: &[u64]) -> Hardware {
        Hardware {
            ram_bytes: ram_gib * GIB,
            cpu_threads: 8,
            gpus: vram_gib.iter().map(|&v| Gpu { name: "GPU".into(), vram_bytes: v * GIB }).collect(),
        }
    }

    fn rec(h: &Hardware) -> (Option<&str>, bool) {
        let r = recommend(h);
        (MODELS.iter().map(|m| m.file.id).find(|id| r.model.as_deref() == Some(*id)), r.experimental)
    }

    #[test]
    fn catalog_is_pinned_and_biggest_first() {
        assert!(MODELS.iter().all(|m| m.file.sha256.len() == 64 && m.file.url.ends_with(m.file.file) && m.file.file.ends_with(".gguf")));
        assert!(MODELS.windows(2).all(|w| w[0].file.bytes > w[1].file.bytes));
        assert!(model("qwen3-14b").is_ok() && model("nope").is_err());
    }

    #[test]
    fn recommendation_follows_ram_and_vram() {
        // 32 GB laptop without a GPU: 14B on the CPU, not the slow 24B.
        assert_eq!(rec(&hw(31, &[])), (Some("qwen3-14b"), false));
        // 64 GB without a GPU: still the 14B (24B is too slow on a CPU).
        assert_eq!(rec(&hw(64, &[])), (Some("qwen3-14b"), false));
        // 24 GB GPU: the 24B fits entirely.
        assert_eq!(rec(&hw(32, &[24])), (Some("mistral-small-3.2-24b"), false));
        // 12 GB GPU: the 14B fits entirely, fast even with 16 GB RAM.
        assert_eq!(rec(&hw(16, &[12])), (Some("qwen3-14b"), false));
        // 16 GB laptop: only the 8B, experimental.
        assert_eq!(rec(&hw(16, &[])), (Some("qwen3-8b"), true));
        // 8 GB: nothing.
        assert_eq!(rec(&hw(8, &[])), (None, false));
    }

    #[test]
    fn fit_levels() {
        let m14 = model("qwen3-14b").unwrap();
        assert_eq!(fit(&hw(32, &[16]), m14), Fit::Gpu);
        assert_eq!(fit(&hw(32, &[8]), m14), Fit::Partial);
        assert_eq!(fit(&hw(32, &[2]), m14), Fit::Cpu);
        assert_eq!(fit(&hw(16, &[]), m14), Fit::TooBig);
    }

    #[test]
    fn server_listens_on_loopback_without_caches_or_ui() {
        let args: Vec<String> = server_args(Path::new("m.gguf"), model("qwen3-14b").unwrap(), 4321)
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let joined = args.join(" ");
        for want in ["-m m.gguf", "--host 127.0.0.1", "--port 4321", "--no-webui", "--no-slots", "--cache-ram 0", "--reasoning-budget 0"] {
            assert!(joined.contains(want), "{want} missing in {joined}");
        }
        assert!(!joined.contains("api-key"), "the key goes through the environment");
        let mistral = server_args(Path::new("m.gguf"), model("mistral-small-3.2-24b").unwrap(), 1);
        assert!(!mistral.iter().any(|a| a == "--reasoning-budget"));
    }
}
