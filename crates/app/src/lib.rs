//! Tauri shell: thin command wrappers over `thera_core`. No business logic lives here.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};
use thera_capture::power::{self, KeepAwake};
use thera_capture::system::{self, ProcessJob};
use thera_capture::{InputDevice, Recorder};
use thera_core::audio::{self, ChunkWriter};
use thera_core::backup::{self, BackupSummary, Unlocked};
use thera_core::consent::{self, ConsentText};
use thera_core::crypto::{self, KeyStore, MasterKey, Unlock};
use thera_core::diarize;
use thera_core::domain::*;
use thera_core::catalog::{self, CloudModel, Tier};
use thera_core::draft;
use thera_core::extract;
use thera_core::llm::{self, Endpoint, LlmConfig, ProviderId, ProviderInfo, RemoteModel};
use thera_core::local::{self, Fit, Gpu, Hardware, LlamaServer, Recommendation};
use thera_core::models::ModelInfo;
use thera_core::pii::{self, CheckItem};
use thera_core::pseudo::EntityKind;
use thera_core::render;
use thera_core::store::{AutopilotRun, SessionSummary, Store, TimelineGroup};
use thera_core::summary;
use thera_core::stt::{self, CloudStt, CloudSttModel, SttConfig, SttEngine, WhisperCli};
use thera_core::templates::{Context, Template};
use thera_core::tts::{self, TtsConfig};
use thera_core::context;
use thera_core::live::{self, LiveConfig};
use thera_core::dictate;
use thera_core::workshop;
use thera_core::agent::{self, Host};
use thera_core::autopilot;
use thera_core::contingent::Contingent;

const KEYRING_SERVICE: &str = "de.datorga.thera";
const KEYRING_USER: &str = "master-key";

// ---------- error ----------

#[derive(Debug, thiserror::Error)]
enum CmdError {
    #[error(transparent)]
    Core(#[from] thera_core::Error),
    #[error("{0}")]
    Other(String),
}

impl Serialize for CmdError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

type CmdResult<T> = Result<T, CmdError>;

// ---------- keychain ----------

/// Keychain entry `KEYRING_SERVICE`/`user`. `user` is `KEYRING_USER` except in debug builds
/// started with `THERA_KEYRING_USER` (isolated first-run E2E; never touches the real entry).
struct OsKeyStore<'a> {
    user: &'a str,
}

impl KeyStore for OsKeyStore<'_> {
    fn load(&self) -> thera_core::Result<Option<MasterKey>> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, self.user).map_err(kr)?;
        match entry.get_password() {
            Ok(hex) => Ok(Some(MasterKey::from_hex(&hex)?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(kr(e)),
        }
    }

    fn store(&self, key: &MasterKey) -> thera_core::Result<()> {
        keyring::Entry::new(KEYRING_SERVICE, self.user).map_err(kr)?.set_password(&key.to_hex()).map_err(kr)
    }
}

fn kr(e: keyring::Error) -> thera_core::Error {
    thera_core::Error::Crypto(format!("keychain: {e}"))
}

// ---------- state ----------

struct AppState {
    data_dir: PathBuf,
    keyring_user: String,
    /// Bundled resources: the whisper.cpp sidecar in `whisper/`, llama-server in `llama/`.
    resource_dir: PathBuf,
    store: Mutex<Option<Store>>,
    recording: Mutex<Option<ActiveRecording>>,
    /// Set when there may be queued jobs; the worker waits on it.
    work: (Mutex<bool>, Condvar),
    transcribing: Mutex<Option<RunningJob>>,
    /// The app is closing: a killed job stays `running` and is re-queued on the next unlock.
    exiting: AtomicBool,
    /// Model downloads/imports in progress (or failed), by model id.
    model_transfers: Mutex<HashMap<String, Transfer>>,
    /// Sidecars are assigned to it, so they die with the app, also on a crash.
    job: Option<Arc<ProcessJob>>,
    /// The running local model server (provider `local`).
    llama: Mutex<Option<LlamaServer>>,
    /// Held while a server starts, so two requests don't load two models.
    llama_start: Mutex<()>,
    /// The model id being loaded right now, for the Settings status.
    llama_loading: Mutex<Option<String>>,
    /// Running extractions and drafts by the request id the UI chose, for `llm_cancel`.
    llm_runs: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// The live dictation (voice mode, Werkstatt), if one runs.
    dictation: Mutex<Option<ActiveDictation>>,
    /// whisper-server for live dictation, kept loaded between dictations for a while.
    whisper_server: Mutex<Option<dictate::WhisperServer>>,
    whisper_last_used: Mutex<Option<std::time::Instant>>,
}

struct RunningJob {
    job_id: String,
    /// Whose data the job reads (erasing that patient stops it).
    patient_id: String,
    cancel: Arc<AtomicBool>,
    /// Set once the job runs in the cloud, with the patient whose consent it relies on.
    cloud_patient: Option<String>,
}

#[derive(Clone, Serialize)]
struct Transfer {
    received: u64,
    total: u64,
    error: Option<String>,
}

/// The one recording that may run at a time.
struct ActiveRecording {
    source_id: String,
    session_id: String,
    patient_id: String,
    kind: SourceKind,
    recorder: Recorder,
    writer: Arc<Mutex<ChunkWriter>>,
    /// Last measured free space on the audio volume (`u64::MAX` = unknown), updated by the guard.
    free_bytes: Arc<AtomicU64>,
}

/// 16 kHz mono i16 on disk: 32 kB per second, ~115 MB per hour.
const AUDIO_BYTES_PER_SEC: u64 = 32_000;
/// Free space needed to start a recording (~4 h of audio).
const DISK_START_MIN: u64 = 500 << 20;
/// Below this a running recording is stopped cleanly (`end_reason = disk_full`).
const DISK_FLOOR: u64 = 200 << 20;
/// The UI warns once fewer minutes than this are left before the automatic stop.
const DISK_WARN_MINUTES: u64 = 60;

/// (start minimum, floor). Debug builds take `THERA_DISK_START_MB` / `THERA_DISK_FLOOR_MB` (E2E).
fn disk_limits() -> (u64, u64) {
    #[cfg(debug_assertions)]
    {
        let mb = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<u64>().ok()).map(|v| v << 20);
        (mb("THERA_DISK_START_MB").unwrap_or(DISK_START_MIN), mb("THERA_DISK_FLOOR_MB").unwrap_or(DISK_FLOOR))
    }
    #[cfg(not(debug_assertions))]
    (DISK_START_MIN, DISK_FLOOR)
}

/// Runs for one recording: keeps the machine awake (the flag is per thread) and stops the
/// recording before the disk fills up.
fn spawn_recording_guard(app: AppHandle, source_id: String, free: Arc<AtomicU64>) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let _awake = KeepAwake::new();
        let (_, floor) = disk_limits();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let current = state.recording.lock().ok().and_then(|g| g.as_ref().map(|r| r.source_id.clone()));
            if current.as_deref() != Some(source_id.as_str()) {
                break;
            }
            if let Some(bytes) = power::free_space(&state.audio_root()) {
                free.store(bytes, Ordering::Relaxed);
                if bytes < floor {
                    let _ = stop_recording(&state, "disk_full");
                    break;
                }
            }
        }
    });
}

impl AppState {
    fn keys(&self) -> OsKeyStore<'_> {
        OsKeyStore { user: &self.keyring_user }
    }
    fn db_path(&self) -> PathBuf {
        self.data_dir.join("thera.db")
    }
    fn wrapped_path(&self) -> PathBuf {
        self.data_dir.join("master.wrapped")
    }
    fn audio_root(&self) -> PathBuf {
        self.data_dir.join("audio")
    }
    fn models_dir(&self) -> PathBuf {
        self.data_dir.join("models")
    }
    /// Plaintext WAV/JSON while the sidecar runs. Wiped on every unlock (crash leftovers).
    fn stt_tmp(&self) -> PathBuf {
        self.data_dir.join("stt-tmp")
    }
    fn whisper_dir(&self) -> PathBuf {
        self.resource_dir.join("whisper")
    }
    fn llama_dir(&self) -> PathBuf {
        self.resource_dir.join("llama")
    }
    /// sherpa-onnx and the speaker models (scripts/fetch-sherpa.ps1).
    fn diarize_dir(&self) -> PathBuf {
        self.resource_dir.join("diarize")
    }
    /// The engine string prints and labels are stored under; `None` when this build lacks it.
    fn diarize_engine(&self) -> Option<String> {
        let dir = self.diarize_dir();
        diarize::Sidecar::ready(&dir).then(|| diarize::Sidecar::version(&dir).unwrap_or_else(|| "sherpa-onnx".into()))
    }
    /// This app started again with `--diarize`, in the kill-on-close job.
    fn diarize_sidecar(&self) -> CmdResult<diarize::Sidecar> {
        let exe = std::env::current_exe().map_err(|e| CmdError::Other(e.to_string()))?;
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(1).clamp(1, 8)) as i32;
        let on_spawn = self.job.clone().map(|job| -> Arc<dyn Fn(&std::process::Child) + Send + Sync> {
            Arc::new(move |child| {
                job.assign(child);
            })
        });
        Ok(diarize::Sidecar { exe, args: vec!["--diarize".into()], dir: self.diarize_dir(), threads, on_spawn })
    }
    /// Debug builds take `THERA_LLAMA_SERVER` (E2E against the fake server).
    fn llama_exe(&self) -> PathBuf {
        #[cfg(debug_assertions)]
        if let Some(exe) = std::env::var_os("THERA_LLAMA_SERVER") {
            return exe.into();
        }
        self.llama_dir().join("llama-server.exe")
    }
    /// e.g. `llama.cpp-b11932`; `None` when llama-server is missing from this build.
    fn llama_engine(&self) -> Option<String> {
        if !self.llama_exe().exists() {
            return None;
        }
        let version = std::fs::read_to_string(self.llama_dir().join("VERSION")).unwrap_or_default();
        Some(format!("{}-{}", local::ENGINE, version.trim()))
    }
    /// e.g. `whisper.cpp-b5454`; `None` when the sidecar is missing from this build.
    fn engine(&self) -> Option<String> {
        if !self.whisper_dir().join("whisper-cli.exe").exists() {
            return None;
        }
        let version = std::fs::read_to_string(self.whisper_dir().join("VERSION")).unwrap_or_default();
        Some(format!("{}-{}", stt::ENGINE, version.trim()))
    }
    /// Opens the store, closes sources a previous crash left in `recording` and resumes jobs.
    fn open_store(&self, key: &MasterKey) -> CmdResult<()> {
        let store = Store::open(&self.db_path(), key)?;
        store.recover_interrupted_sources()?;
        store.requeue_running_jobs()?;
        let _ = std::fs::remove_dir_all(self.stt_tmp());
        *lock(&self.store)? = Some(store);
        self.wake_worker();
        Ok(())
    }
    fn wake_worker(&self) {
        let (flag, cv) = &self.work;
        if let Ok(mut f) = flag.lock() {
            *f = true;
            cv.notify_one();
        }
    }
    fn with<T>(&self, f: impl FnOnce(&Store) -> thera_core::Result<T>) -> CmdResult<T> {
        let guard = self.store.lock().map_err(|_| CmdError::Other("state poisoned".into()))?;
        let store = guard.as_ref().ok_or_else(|| CmdError::Other("locked".into()))?;
        Ok(f(store)?)
    }
}

fn lock<T>(m: &Mutex<T>) -> CmdResult<MutexGuard<'_, T>> {
    m.lock().map_err(|_| CmdError::Other("state poisoned".into()))
}

#[derive(Serialize)]
struct InitResult {
    /// "ready" | "recovery_required" | "new" (nothing set up yet: set up or restore a backup)
    status: &'static str,
    /// Shown once on first run; the user must write it down / print it.
    recovery_key: Option<String>,
    /// The current recovery key was confirmed as stored. False → the UI asks for a new kit.
    recovery_confirmed: bool,
    encrypted: bool,
}

#[tauri::command]
fn app_init(state: State<'_, AppState>) -> CmdResult<InitResult> {
    if state.store.lock().map_err(|_| CmdError::Other("state poisoned".into()))?.is_some() {
        let recovery_confirmed = state.with(|s| s.recovery_kit_confirmed())?;
        return Ok(InitResult { status: "ready", recovery_key: None, recovery_confirmed, encrypted: Store::is_encrypted() });
    }
    // First start on this machine: let the user choose between a new setup and a restore.
    if state.keys().load()?.is_none() && !state.wrapped_path().exists() {
        return Ok(InitResult { status: "new", recovery_key: None, recovery_confirmed: false, encrypted: Store::is_encrypted() });
    }
    app_open(&state)
}

/// "Neu einrichten": creates the master key, recovery key and an empty database.
#[tauri::command]
fn app_setup(state: State<'_, AppState>) -> CmdResult<InitResult> {
    if lock(&state.store)?.is_some() {
        return Err(CmdError::Other("Bereits eingerichtet".into()));
    }
    app_open(&state)
}

fn app_open(state: &AppState) -> CmdResult<InitResult> {
    let (key, recovery_key) = match crypto::unlock_or_init(&state.keys(), &state.wrapped_path()) {
        Ok(Unlock::Existing(k)) => (k, None),
        Ok(Unlock::Created { key, recovery }) => (key, Some(recovery.display())),
        Err(thera_core::Error::Crypto(m)) if m == "recovery_required" => {
            return Ok(InitResult {
                status: "recovery_required",
                recovery_key: None,
                recovery_confirmed: false,
                encrypted: Store::is_encrypted(),
            })
        }
        Err(e) => return Err(e.into()),
    };
    state.open_store(&key)?;
    let recovery_confirmed = state.with(|s| s.recovery_kit_confirmed())?;
    Ok(InitResult { status: "ready", recovery_key, recovery_confirmed, encrypted: Store::is_encrypted() })
}

#[tauri::command]
fn app_recover(state: State<'_, AppState>, recovery_key: String) -> CmdResult<()> {
    let key = crypto::recover(&state.keys(), &state.wrapped_path(), &recovery_key)?;
    state.open_store(&key)
}

/// Writes the printable Notfall-Kit for `recovery_key` (the one just shown on first run or
/// after rotation). Refuses a key that doesn't open `master.wrapped`.
#[tauri::command]
fn recovery_kit_export(state: State<'_, AppState>, recovery_key: String, rotated: bool, path: String) -> CmdResult<()> {
    let key = crypto::check_recovery(&state.wrapped_path(), &recovery_key)?;
    let kit = render::RecoveryKit::new(
        &key,
        &thera_core::domain::now(),
        rotated,
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "—".into()),
        env!("CARGO_PKG_VERSION").into(),
        state.data_dir.display().to_string(),
    );
    std::fs::write(&path, render::recovery_kit_pdf(&kit)?).map_err(thera_core::Error::from)?;
    state.with(|s| s.log_recovery_kit_export(!rotated))
}

#[tauri::command]
fn recovery_kit_confirm(state: State<'_, AppState>) -> CmdResult<()> {
    state.with(|s| s.confirm_recovery_kit())
}

#[derive(Serialize)]
struct Rotated {
    /// Shown once.
    recovery_key: String,
    /// Backups in the backup folder moved to the new key.
    rekeyed: u32,
    /// Backups there that could not be updated (they still need the old key).
    rekey_failed: u32,
}

/// "Neuen Notfall-Schlüssel erstellen". Returns the new key once; earlier kits stop working.
/// Backups in the backup folder are re-wrapped to the new key (header only).
#[tauri::command]
async fn recovery_key_rotate(app: AppHandle) -> CmdResult<Rotated> {
    blocking(app, |state| {
        let key = state.keys().load()?.ok_or_else(|| CmdError::Other("Schlüssel nicht im Schlüsselbund".into()))?;
        let recovery = state.with(|s| s.rotate_recovery_key(&key, &state.wrapped_path()))?;
        let (mut rekeyed, mut rekey_failed) = (0, 0);
        if let Some(dir) = backup_dir(state)? {
            for path in backup_files(&dir) {
                match backup::rewrap_file(&path, &key, &recovery) {
                    Ok(true) => rekeyed += 1,
                    Ok(false) => {}
                    Err(_) => rekey_failed += 1,
                }
            }
            if rekeyed > 0 {
                state.with(|s| s.log_backup("backup.rekey", rekeyed as u64, 0))?;
            }
        }
        Ok(Rotated { recovery_key: recovery.display(), rekeyed, rekey_failed })
    })
    .await
}

// ---------- backup ----------

/// Runs `f` off the main thread (backups and restores take seconds).
async fn blocking<T: Send + 'static>(app: AppHandle, f: impl FnOnce(&AppState) -> CmdResult<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>()))
        .await
        .map_err(|e| CmdError::Other(e.to_string()))?
}

fn backup_dir(state: &AppState) -> CmdResult<Option<PathBuf>> {
    Ok(state
        .with(|s| s.setting("backup_dir"))?
        .and_then(|v| v.as_str().map(PathBuf::from))
        .filter(|p| !p.as_os_str().is_empty()))
}

fn backup_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == backup::EXTENSION))
        .collect();
    files.sort();
    files
}

#[derive(Serialize)]
struct BackupDone {
    path: String,
    summary: BackupSummary,
}

/// Writes `thera-backup-<time>.therabak` to the backup folder and checks it can be restored
/// (decrypt everything, open the database, walk the audit chain).
#[tauri::command]
async fn backup_create(app: AppHandle) -> CmdResult<BackupDone> {
    blocking(app, |state| {
        let dir = backup_dir(state)?.ok_or_else(|| CmdError::Other("Bitte zuerst einen Sicherungsordner wählen".into()))?;
        std::fs::create_dir_all(&dir).map_err(thera_core::Error::from)?;
        let key = state.keys().load()?.ok_or_else(|| CmdError::Other("Schlüssel nicht im Schlüsselbund".into()))?;
        let wrapped = std::fs::read_to_string(state.wrapped_path()).map_err(thera_core::Error::from)?;

        let tmp = state.data_dir.join("backup-tmp");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir(&tmp).map_err(thera_core::Error::from)?;
        let snapshot = tmp.join("thera.db");
        let result = (|| {
            state.with(|s| s.snapshot_to(&snapshot))?;
            let stamp = render::file_stamp(&thera_core::domain::now());
            let out = dir.join(format!("thera-backup-{stamp}.{}", backup::EXTENSION));
            let summary = backup::create(&out, &snapshot, &state.audio_root(), &key, &wrapped, env!("CARGO_PKG_VERSION"))?;
            backup::verify(&out, &key, &state.data_dir.join("backup-verify"))?;
            Ok::<_, CmdError>((out, summary))
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        let (out, summary) = result?;
        let name = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        state.with(|s| {
            s.log_backup("backup.create", summary.files, summary.bytes)?;
            s.set_setting("last_backup", &serde_json::json!({ "at": summary.created_at, "file": name, "bytes": summary.bytes }))
        })?;
        Ok(BackupDone { path: out.display().to_string(), summary })
    })
    .await
}

#[derive(Serialize)]
struct BackupHeaderView {
    created_at: String,
    app_version: String,
    /// The keychain already holds this backup's key: no Notfall-Schlüssel needed.
    keychain_matches: bool,
}

#[tauri::command]
fn backup_inspect(state: State<'_, AppState>, path: String) -> CmdResult<BackupHeaderView> {
    let h = backup::inspect(std::path::Path::new(&path))?;
    let keychain_matches = state.keys().load()?.is_some_and(|k| backup::key_id(&k) == h.key_id);
    Ok(BackupHeaderView { created_at: h.created_at, app_version: h.app_version, keychain_matches })
}

#[derive(Serialize)]
struct RestoreDone {
    summary: BackupSummary,
    /// Folder holding the data that was replaced (moved, not deleted).
    previous: Option<String>,
}

/// Replaces the current data with a backup. Needs the Notfall-Schlüssel unless the keychain key
/// matches. Works from the setup/recovery screens (store closed) and from Settings (store open).
#[tauri::command]
async fn backup_restore(app: AppHandle, path: String, recovery_key: Option<String>) -> CmdResult<RestoreDone> {
    blocking(app, move |state| {
        if lock(&state.recording)?.is_some() {
            return Err(CmdError::Other("Bitte zuerst die laufende Aufnahme beenden".into()));
        }
        if lock(&state.transcribing)?.is_some() {
            return Err(CmdError::Other("Bitte warten, bis die Transkription fertig ist, oder sie abbrechen".into()));
        }
        let archive = PathBuf::from(&path);
        let header = backup::inspect(&archive)?;
        let keychain = state.keys().load()?;
        let (master, how) = backup::unlock(&header, keychain.as_ref(), recovery_key.as_deref())?;
        let via_recovery = matches!(how, Unlocked::Recovery);
        let wrapped = (via_recovery || !state.wrapped_path().exists()).then_some(header.wrapped.as_str());

        let was_open = lock(&state.store)?.take().is_some();
        let installed = match backup::install(&archive, &master, &state.data_dir, wrapped) {
            Ok(i) => i,
            Err(e) => {
                if let (true, Some(k)) = (was_open, keychain.as_ref()) {
                    let _ = state.open_store(k);
                }
                return Err(e.into());
            }
        };
        if via_recovery {
            state.keys().store(&master)?;
        }
        state.open_store(&master)?;
        // The restored database predates its own "last backup" entry; this backup is the latest known one.
        let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        state.with(|s| {
            s.log_backup("backup.restore", installed.summary.files, installed.summary.bytes)?;
            s.set_setting(
                "last_backup",
                &serde_json::json!({ "at": installed.summary.created_at, "file": name, "bytes": installed.summary.bytes }),
            )
        })?;
        Ok(RestoreDone { summary: installed.summary, previous: installed.previous.map(|p| p.display().to_string()) })
    })
    .await
}

// ---------- patients ----------

#[tauri::command]
fn patients_list(state: State<'_, AppState>, query: Option<String>) -> CmdResult<Vec<Patient>> {
    state.with(|s| s.list_patients(query.as_deref()))
}

#[tauri::command]
fn patient_get(state: State<'_, AppState>, id: String) -> CmdResult<Patient> {
    state.with(|s| s.patient(&id))
}

#[tauri::command]
fn patient_create(state: State<'_, AppState>, input: PatientInput) -> CmdResult<Patient> {
    state.with(|s| s.create_patient(input))
}

#[tauri::command]
fn patient_update(state: State<'_, AppState>, id: String, input: PatientInput) -> CmdResult<Patient> {
    state.with(|s| s.update_patient(&id, input))
}

#[tauri::command]
fn patient_cards(state: State<'_, AppState>, query: Option<String>, now: String) -> CmdResult<Vec<PatientCard>> {
    state.with(|s| s.patient_cards(query.as_deref(), &now))
}

/// The portrait as a `data:` URL, or `None`. It's decoded only in the webview, never written out.
#[tauri::command]
fn patient_photo(state: State<'_, AppState>, id: String) -> CmdResult<Option<String>> {
    use base64::Engine as _;
    let jpeg = state.with(|s| s.patient_photo(&id))?;
    Ok(jpeg.map(|j| format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(j))))
}

/// Reads the picked file and stores a normalized copy; the original file stays where it was.
#[tauri::command]
fn patient_photo_set(state: State<'_, AppState>, id: String, path: String) -> CmdResult<()> {
    let len = std::fs::metadata(&path).map_err(|e| CmdError::Other(format!("Bild nicht lesbar: {e}")))?.len();
    if len as usize > thera_core::photo::MAX_UPLOAD {
        return Err(CmdError::Other("Das Bild ist zu groß (höchstens 25 MB).".into()));
    }
    let bytes = std::fs::read(&path).map_err(|e| CmdError::Other(format!("Bild nicht lesbar: {e}")))?;
    state.with(|s| s.set_patient_photo(&id, &bytes))
}

#[tauri::command]
fn patient_photo_remove(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.with(|s| s.remove_patient_photo(&id))
}

#[tauri::command]
fn patient_erase(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    // A running transcription of this patient is stopped first, which removes its plaintext WAV.
    if let Some(r) = lock(&state.transcribing)?.as_ref().filter(|r| r.patient_id == id) {
        r.cancel.store(true, Ordering::Relaxed);
    }
    state.with(|s| s.erase_patient(&id))
}

// ---------- consent ----------

#[tauri::command]
fn consent_text(kind: ConsentKind) -> ConsentText {
    consent::current(kind)
}

/// Printable consent form (PDF) with the current text, for signing on paper.
#[tauri::command]
fn consent_form_export(state: State<'_, AppState>, patient_id: String, kind: ConsentKind, path: String) -> CmdResult<()> {
    state.with(|s| {
        let patient = s.patient(&patient_id)?;
        let guardians = s.list_episodes(&patient_id)?.iter().any(|e| e.age_group == AgeGroup::Kjp);
        let text_of = |k: &str| s.setting(k).map(|v| v.and_then(|v| v.as_str().map(str::to_string)));
        let text = consent::current(kind);
        let form = render::ConsentForm::new(
            &text,
            &patient,
            text_of("therapist_name")?,
            text_of("practice_header")?,
            guardians,
            &thera_core::domain::now(),
        );
        std::fs::write(&path, render::consent_form_pdf(&form)?)?;
        s.log_consent_form(&patient_id, kind, &text.version)
    })
}

#[tauri::command]
fn consents_list(state: State<'_, AppState>, patient_id: String) -> CmdResult<Vec<Consent>> {
    state.with(|s| s.list_consents(&patient_id))
}

#[tauri::command]
fn consent_grant(state: State<'_, AppState>, patient_id: String, kind: ConsentKind, note: Option<String>) -> CmdResult<Consent> {
    state.with(|s| s.grant_consent(&patient_id, kind, note.as_deref()))
}

#[tauri::command]
fn consent_revoke(state: State<'_, AppState>, id: String) -> CmdResult<Consent> {
    // Revocation ends a running session recording of this patient at once (the cut point).
    let patient_id = state.with(|s| s.consent(&id))?.patient_id;
    let affected = state.recording.lock().map_err(|_| CmdError::Other("state poisoned".into()))?.as_ref().is_some_and(|r| {
        r.kind == SourceKind::SessionAudio && r.patient_id == patient_id
    });
    if affected {
        stop_recording(&state, "consent_revoked")?;
    }
    let revoked = state.with(|s| s.revoke_consent(&id))?;
    // Revoking cloud transcription stops a cloud run of this patient's recording mid-way.
    if revoked.kind == ConsentKind::CloudTranscription {
        if let Some(r) = lock(&state.transcribing)?.as_ref().filter(|r| r.cloud_patient.as_deref() == Some(patient_id.as_str())) {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }
    Ok(revoked)
}

// ---------- capture ----------

#[tauri::command]
fn audio_devices() -> CmdResult<Vec<InputDevice>> {
    thera_capture::input_devices().map_err(|e| CmdError::Other(e.to_string()))
}

#[tauri::command]
fn recording_start(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    kind: SourceKind,
    device_id: Option<String>,
) -> CmdResult<SessionSource> {
    let mut slot = state.recording.lock().map_err(|_| CmdError::Other("state poisoned".into()))?;
    if slot.is_some() {
        return Err(CmdError::Other("Es läuft bereits eine Aufnahme".into()));
    }
    if lock(&state.dictation)?.is_some() {
        return Err(CmdError::Other("Bitte zuerst das Diktat beenden".into()));
    }
    let patient_id = state.with(|s| s.session(&session_id))?.patient_id;
    let free = power::free_space(&state.audio_root());
    if let Some(bytes) = free.filter(|b| *b < disk_limits().0) {
        return Err(CmdError::Other(format!(
            "Zu wenig Speicherplatz: {} MB frei, für eine Aufnahme werden mindestens {} MB benötigt",
            bytes >> 20,
            disk_limits().0 >> 20
        )));
    }
    let (source, writer) = state.with(|s| s.start_source(&session_id, kind, &state.audio_root()))?;
    let writer = Arc::new(Mutex::new(writer));
    let w = writer.clone();
    let sink = Box::new(move |pcm: &[i16]| w.lock().map_err(|_| "poisoned".to_string())?.write(pcm).map_err(|e| e.to_string()));
    let recorder = match Recorder::start(device_id.as_deref(), audio::CHUNK_SAMPLES, sink) {
        Ok(r) => r,
        Err(e) => {
            let w = writer.lock().map_err(|_| CmdError::Other("state poisoned".into()))?;
            state.with(|s| s.finish_source(&source.id, "device_error", &w))?;
            return Err(CmdError::Other(e.to_string()));
        }
    };
    let free_bytes = Arc::new(AtomicU64::new(free.unwrap_or(u64::MAX)));
    *slot = Some(ActiveRecording { source_id: source.id.clone(), session_id, patient_id, kind, recorder, writer, free_bytes: free_bytes.clone() });
    spawn_recording_guard(app, source.id.clone(), free_bytes);
    Ok(source)
}

#[derive(Serialize)]
struct RecordingStatus {
    source_id: String,
    session_id: String,
    kind: SourceKind,
    duration_ms: u64,
    level: f32,
    error: Option<String>,
    /// Minutes of recording left before the automatic stop; only when space is getting low.
    minutes_left: Option<u64>,
}

/// Polled by the UI (~10 Hz) for the timer and level meter.
#[tauri::command]
fn recording_status(state: State<'_, AppState>) -> CmdResult<Option<RecordingStatus>> {
    let slot = state.recording.lock().map_err(|_| CmdError::Other("state poisoned".into()))?;
    Ok(slot.as_ref().map(|r| RecordingStatus {
        source_id: r.source_id.clone(),
        session_id: r.session_id.clone(),
        kind: r.kind,
        duration_ms: r.recorder.duration_ms(),
        level: r.recorder.level(),
        error: r.recorder.error(),
        minutes_left: Some(r.free_bytes.load(Ordering::Relaxed))
            .filter(|b| *b != u64::MAX)
            .map(|b| b.saturating_sub(disk_limits().1) / AUDIO_BYTES_PER_SEC / 60)
            .filter(|m| *m < DISK_WARN_MINUTES),
    }))
}

#[tauri::command]
fn recording_stop(state: State<'_, AppState>) -> CmdResult<SessionSource> {
    stop_recording(&state, "stopped")
}

/// Stops the device, flushes the last chunk and closes the source in the database.
fn stop_recording(state: &AppState, reason: &str) -> CmdResult<SessionSource> {
    let active = state
        .recording
        .lock()
        .map_err(|_| CmdError::Other("state poisoned".into()))?
        .take()
        .ok_or_else(|| CmdError::Other("Es läuft keine Aufnahme".into()))?;
    let failed = active.recorder.error().is_some();
    let flushed = active.recorder.stop();
    let reason = if failed || flushed.is_err() { "device_error" } else { reason };
    let w = active.writer.lock().map_err(|_| CmdError::Other("state poisoned".into()))?;
    let source = state.with(|s| s.finish_source(&active.source_id, reason, &w))?;
    drop(w);
    // Autopilot: the transcription starts by itself with the engine Settings choose. A failure
    // to queue (nothing recorded) leaves the manual button.
    if state.with(|s| s.autopilot_enabled())? && source.duration_ms.unwrap_or(0) > 0 && state.with(|s| s.enqueue_transcription(&source.id, None)).is_ok() {
        state.wake_worker();
    }
    Ok(source)
}

/// Everything the session page needs in one round-trip.
#[derive(Serialize)]
struct SessionView {
    session: Session,
    patient: Patient,
    sources: Vec<SourceView>,
}

#[derive(Serialize)]
struct SourceView {
    #[serde(flatten)]
    source: SessionSource,
    /// Latest transcription job, if any.
    job: Option<Job>,
    transcript: Option<Transcript>,
    stt: SourceStt,
}

/// The engines a recording can be transcribed with right now, for the choice in the session.
#[derive(Serialize)]
struct SourceStt {
    /// What Settings pick (preselected).
    default: SttTarget,
    /// Why local can't run (no sidecar, no speech model), or `None`.
    local_problem: Option<String>,
    /// Why cloud can't run (no consent for a session recording, no key, no model), or `None`.
    cloud_problem: Option<String>,
    /// The cloud problem is the patient's missing consent (fixed on the patient page).
    cloud_needs_consent: bool,
    /// e.g. "OpenRouter · openai/whisper-large-v3-turbo".
    cloud_label: String,
}

fn source_stt(state: &AppState, src: &SessionSource, patient_id: &str) -> CmdResult<SourceStt> {
    let config = state.with(|s| s.stt_config())?;
    let consent = state.with(|s| s.active_consent(patient_id, ConsentKind::CloudTranscription))?.is_some();
    let local_problem = local_stt(state, &src.language).err().map(|e| e.to_string());
    let model = config.model();
    let cloud_problem = if !stt::cloud_allowed(src.kind, consent) {
        Some(stt::NO_CLOUD_CONSENT.to_string())
    } else if model.is_none() {
        Some("Kein Cloud-Sprachmodell eingetragen (Einstellungen → Transkription)".to_string())
    } else {
        llm_endpoint(state, config.cloud_provider, &AtomicBool::new(false)).err().map(|e| e.to_string())
    };
    Ok(SourceStt {
        default: stt::default_target(&config, src.kind, local_problem.is_none(), consent),
        local_problem,
        cloud_problem,
        cloud_needs_consent: !stt::cloud_allowed(src.kind, consent),
        cloud_label: format!("{} · {}", llm::provider(config.cloud_provider).label, model.unwrap_or_default()),
    })
}

/// A stretch of a recording as WAV bytes, decrypted in memory for playback in the transcript
/// viewer. Nothing is written to disk; the UI plays it from a blob URL.
#[tauri::command]
async fn source_audio_wav(app: AppHandle, source_id: String, from_ms: u64, to_ms: u64) -> CmdResult<tauri::ipc::Response> {
    blocking(app, move |state| Ok(tauri::ipc::Response::new(state.with(|s| s.audio_clip(&source_id, from_ms, to_ms))?))).await
}

#[tauri::command]
fn session_get(state: State<'_, AppState>, id: String) -> CmdResult<SessionView> {
    let (session, patient, rows) = state.with(|s| {
        let session = s.session(&id)?;
        let rows = s
            .list_sources(&id)?
            .into_iter()
            .map(|src| Ok((s.latest_job(&src.id)?, s.transcript_for_source(&src.id)?, src)))
            .collect::<thera_core::Result<Vec<_>>>()?;
        let patient = s.patient(&session.patient_id)?;
        Ok((session, patient, rows))
    })?;
    let sources = rows
        .into_iter()
        .map(|(job, transcript, source)| Ok(SourceView { stt: source_stt(&state, &source, &patient.id)?, job, transcript, source }))
        .collect::<CmdResult<_>>()?;
    Ok(SessionView { patient, sources, session })
}

// ---------- transcription ----------

/// Queues a transcription. `engine` is the choice made in the session (`None` = Settings); a
/// cloud choice the rules forbid is refused here already.
#[tauri::command]
fn transcription_start(state: State<'_, AppState>, source_id: String, engine: Option<SttTarget>) -> CmdResult<Job> {
    if engine == Some(SttTarget::Cloud) {
        let src = state.with(|s| s.source(&source_id))?;
        let patient_id = state.with(|s| s.session(&src.session_id))?.patient_id;
        if let Some(problem) = source_stt(&state, &src, &patient_id)?.cloud_problem {
            return Err(CmdError::Other(problem));
        }
    }
    let job = state.with(|s| s.enqueue_transcription(&source_id, engine))?;
    state.wake_worker();
    Ok(job)
}

#[tauri::command]
fn transcription_cancel(state: State<'_, AppState>, job_id: String) -> CmdResult<()> {
    if let Some(r) = lock(&state.transcribing)?.as_ref().filter(|r| r.job_id == job_id) {
        r.cancel.store(true, Ordering::Relaxed);
        return Ok(());
    }
    state.with(|s| s.cancel_queued_job(&job_id)).map(|_| ())
}

#[tauri::command]
fn transcript_edit_segment(state: State<'_, AppState>, transcript_id: String, idx: i64, text: String) -> CmdResult<()> {
    state.with(|s| s.edit_segment(&transcript_id, idx, &text))
}

#[tauri::command]
fn transcript_confirm(state: State<'_, AppState>, id: String) -> CmdResult<Transcript> {
    let t = state.with(|s| s.confirm_transcript(&id))?;
    // Autopilot: note, facts and summary follow without clicks (nothing is applied or signed).
    let session_id = state.with(|s| Ok(s.source(&t.source_id)?.session_id))?;
    if state.with(|s| s.enqueue_autopilot(&session_id, AutopilotTrigger::Confirm))?.is_some() {
        state.wake_worker();
    }
    Ok(t)
}

#[tauri::command]
fn transcript_set_speaker(state: State<'_, AppState>, transcript_id: String, idx: i64, label: String) -> CmdResult<()> {
    state.with(|s| s.set_segment_speaker(&transcript_id, idx, &label))
}

#[tauri::command]
fn transcript_swap_speakers(state: State<'_, AppState>, transcript_id: String) -> CmdResult<Transcript> {
    state.with(|s| s.swap_speakers(&transcript_id))
}

#[tauri::command]
fn transcript_assign_therapist(state: State<'_, AppState>, transcript_id: String, label: String) -> CmdResult<Transcript> {
    state.with(|s| s.assign_therapist(&transcript_id, &label))
}

#[derive(Serialize)]
struct DiarizeOverview {
    /// `None`: sherpa-onnx or the speaker models are missing from this build.
    engine: Option<String>,
    config: diarize::DiarizeConfig,
    /// Her prints for the current engine.
    profile: VoiceProfile,
}

#[tauri::command]
fn diarize_overview(state: State<'_, AppState>) -> CmdResult<DiarizeOverview> {
    let engine = state.diarize_engine();
    let config = state.with(|s| s.diarize_config())?;
    let profile = match &engine {
        Some(e) => state.with(|s| s.voice_profile(e))?,
        None => VoiceProfile::default(),
    };
    Ok(DiarizeOverview { engine, config, profile })
}

/// "Neu trennen" with `speakers` people; the worker picks the job up like a transcription.
#[tauri::command]
fn transcript_rediarize(state: State<'_, AppState>, source_id: String, speakers: u8) -> CmdResult<Job> {
    let job = state.with(|s| s.enqueue_diarization(&source_id, speakers))?;
    state.wake_worker();
    Ok(job)
}

#[tauri::command]
fn diarize_config_set(state: State<'_, AppState>, config: diarize::DiarizeConfig) -> CmdResult<()> {
    state.with(|s| s.set_diarize_config(&config))
}

#[tauri::command]
fn voice_profile_delete(state: State<'_, AppState>) -> CmdResult<usize> {
    state.with(|s| s.delete_voice_profile())
}

/// Runs queued jobs one at a time, for the life of the app.
fn stt_worker(app: AppHandle) {
    let state = app.state::<AppState>();
    loop {
        {
            let (flag, cv) = &state.work;
            let mut pending = flag.lock().expect("work flag");
            while !*pending {
                pending = cv.wait(pending).expect("work flag");
            }
            *pending = false;
        }
        // `with` fails while the store is locked; the next unlock wakes us again.
        while let Ok(Some(job)) = state.with(|s| s.claim_next_job()) {
            run_job(&app, &state, job);
        }
    }
}

/// `transcript-ready`: a transcription finished and waits for her review.
#[derive(Clone, Serialize)]
struct TranscriptReady {
    session_id: String,
    patient_name: String,
}

/// `autopilot-changed`: a step of the session's autopilot run moved on.
#[derive(Clone, Serialize)]
struct AutopilotChanged<'a> {
    session_id: &'a str,
}

fn run_job(app: &AppHandle, state: &AppState, job: Job) {
    let session_id = match (&job.session_id, &job.source_id) {
        (Some(id), _) => state.with(|s| Ok(s.session(id)?.id)),
        (None, Some(src)) => state.with(|s| Ok(s.source(src)?.session_id)),
        (None, None) => return,
    };
    let Ok(session_id) = session_id else { return };
    let Ok(patient_id) = state.with(|s| Ok(s.session(&session_id)?.patient_id)) else { return };
    let cancel = Arc::new(AtomicBool::new(false));
    if let Ok(mut slot) = state.transcribing.lock() {
        *slot = Some(RunningJob { job_id: job.id.clone(), patient_id: patient_id.clone(), cancel: cancel.clone(), cloud_patient: None });
    }
    let result = match (job.kind.as_str(), job.source_id.clone()) {
        ("autopilot", _) => autopilot_job(app, state, &job, &session_id, cancel.clone()),
        ("diarize", Some(source_id)) => diarize_job(state, &job, &source_id, &cancel).map(|_| ()),
        (_, Some(source_id)) => transcribe_job(state, &job, &source_id, &cancel).and_then(|(engine, model, segments, speakers)| {
            state.with(|s| s.finish_transcription_with(&job.id, &engine, &model, &segments, speakers.as_ref()))?;
            if let Ok(p) = state.with(|s| s.patient(&patient_id)) {
                let _ = app.emit("transcript-ready", TranscriptReady { session_id: session_id.clone(), patient_name: format!("{} {}", p.first_name, p.last_name) });
            }
            Ok(())
        }),
        _ => Ok(()),
    };
    if let Err(e) = result {
        let (status, msg) = match e {
            CmdError::Core(thera_core::Error::Cancelled) => (JobStatus::Cancelled, None),
            e => (JobStatus::Failed, Some(e.to_string())),
        };
        if !state.exiting.load(Ordering::Relaxed) {
            // Fails harmlessly when the patient was erased: the job went with the source.
            let _ = state.with(|s| s.fail_job(&job.id, status, msg.as_deref()));
        }
    }
    if let Ok(mut slot) = state.transcribing.lock() {
        *slot = None;
    }
    if job.kind == "autopilot" {
        let _ = app.emit("autopilot-changed", AutopilotChanged { session_id: &session_id });
    }
}

/// Note, facts and summary after a confirmed transcript, without a preview each time (the
/// user's decision); what was sent is stored per step.
fn autopilot_job(app: &AppHandle, state: &AppState, job: &Job, session_id: &str, cancel: Arc<AtomicBool>) -> CmdResult<()> {
    let mut host = LlmHost::new(state, app, cancel);
    host.changed_session = Some(session_id.to_string());
    autopilot::run(&mut host, job)?;
    Ok(())
}

type Transcription = (String, String, Vec<stt::Segment>, Option<diarize::Diarization>);

/// Transcribes, then separates the speakers of a session recording (locally, whatever engine
/// transcribed) or learns her voice from a dictation.
fn transcribe_job(state: &AppState, job: &Job, source_id: &str, cancel: &AtomicBool) -> CmdResult<Transcription> {
    let source = state.with(|s| s.source(source_id))?;
    let engine = state.diarize_engine();
    let config = state.with(|s| s.diarize_config())?;
    let diarizing = engine.is_some() && config.enabled && source.kind == SourceKind::SessionAudio;
    // With speakers to separate, the transcription is the first 80 % of the progress bar.
    let stt_share = if diarizing { 80 } else { 100 };
    let (stt_engine, model, segments) = transcribe_speech(state, job, source_id, cancel, stt_share)?;
    let Some(engine) = engine else { return Ok((stt_engine, model, segments, None)) };
    let (dir, key) = state.with(|s| s.audio_location(source_id))?;
    let work = state.stt_tmp().join(format!("{}-speakers", job.id));
    let sidecar = state.diarize_sidecar()?;
    if diarizing && !segments.is_empty() {
        let progress = |p: u8| {
            let _ = state.with(|s| s.set_job_progress(&job.id, 80 + p / 5));
        };
        match diarize::diarize_source(&dir, &key, source.speakers, &sidecar, &work, progress, cancel) {
            Ok(out) => {
                let profile = state.with(|s| s.voice_profile_embedding(&engine))?;
                let d = diarize::label(&segments, &out, profile.as_deref(), &engine);
                return Ok((stt_engine, model, segments, Some(d)));
            }
            Err(thera_core::Error::Cancelled) => return Err(thera_core::Error::Cancelled.into()),
            // The transcript is still worth having; it just has no speakers.
            Err(e) => eprintln!("diarization failed: {e}"),
        }
    } else if source.kind == SourceKind::Dictation && config.learn {
        match diarize::embed_source(&dir, &key, &sidecar, &work, cancel) {
            Ok(Some(print)) if print.seconds >= MIN_PRINT_SECONDS => {
                state.with(|s| s.add_voice_print("dictation", &engine, &print.embedding, print.seconds as f64))?;
            }
            Ok(_) => {}
            Err(thera_core::Error::Cancelled) => return Err(thera_core::Error::Cancelled.into()),
            Err(e) => eprintln!("voice print failed: {e}"),
        }
    }
    Ok((stt_engine, model, segments, None))
}

/// "Neu trennen": separates the speakers of a draft transcript again with the recording's
/// speaker count. Unlike after a transcription, a failure here fails the job (she asked for it).
fn diarize_job(state: &AppState, job: &Job, source_id: &str, cancel: &AtomicBool) -> CmdResult<Transcript> {
    let engine = state.diarize_engine().ok_or_else(|| CmdError::Other("Die Sprechertrennung ist nicht installiert".into()))?;
    let source = state.with(|s| s.source(source_id))?;
    let transcript = state.with(|s| s.transcript_for_source(source_id))?.ok_or_else(|| CmdError::Other("Noch keine Abschrift".into()))?;
    let segments: Vec<stt::Segment> = transcript
        .segments
        .iter()
        .map(|s| stt::Segment { start_ms: s.start_ms, end_ms: s.end_ms, text: String::new() })
        .collect();
    let (dir, key) = state.with(|s| s.audio_location(source_id))?;
    let work = state.stt_tmp().join(format!("{}-speakers", job.id));
    let sidecar = state.diarize_sidecar()?;
    let progress = |p: u8| {
        let _ = state.with(|s| s.set_job_progress(&job.id, p));
    };
    let out = diarize::diarize_source(&dir, &key, source.speakers, &sidecar, &work, progress, cancel)?;
    let profile = state.with(|s| s.voice_profile_embedding(&engine))?;
    let d = diarize::label(&segments, &out, profile.as_deref(), &engine);
    state.with(|s| s.finish_diarization(&job.id, &d))
}

/// Speech under this length says too little about a voice to join her profile.
const MIN_PRINT_SECONDS: f32 = 8.0;

/// Transcribes with the engine Settings choose for this kind of recording. Only dictations can
/// reach the cloud (`stt::choose`, enforced again in `stt::transcribe`). Progress goes up to
/// `share` percent.
fn transcribe_speech(state: &AppState, job: &Job, source_id: &str, cancel: &AtomicBool, share: u8) -> CmdResult<(String, String, Vec<stt::Segment>)> {
    let (source, (dir, key)) = state.with(|s| Ok((s.source(source_id)?, s.audio_location(source_id)?)))?;
    let patient_id = state.with(|s| s.session(&source.session_id))?.patient_id;
    // Looked up now, not when queued: a consent revoked meanwhile counts.
    let consent = state.with(|s| s.active_consent(&patient_id, ConsentKind::CloudTranscription))?.is_some();
    let config = state.with(|s| s.stt_config())?;
    let local = local_stt(state, &source.language);
    let mut last = None;
    let on_progress = |p: u8| {
        let p = (p as u32 * share as u32 / 100) as u8;
        if last != Some(p) {
            last = Some(p);
            let _ = state.with(|s| s.set_job_progress(&job.id, p));
        }
    };
    let work = state.stt_tmp().join(&job.id);
    match stt::choose(&config, source.kind, local.is_ok(), consent, job.engine)? {
        SttTarget::Local => {
            let (engine, model_id, cli) = local.map_err(|e| match (source.kind, config.mode, consent) {
                (SourceKind::SessionAudio, stt::SttMode::Cloud | stt::SttMode::Fallback, false) => CmdError::Other(format!(
                    "{e}. In die Cloud darf diese Sitzungsaufnahme nicht: es fehlt die Einwilligung zur Cloud-Transkription."
                )),
                _ => e,
            })?;
            let t = stt::transcribe(source.kind, consent, &dir, &key, &SttEngine::Local(cli), &work, on_progress, cancel)?;
            Ok((engine, model_id, t.segments))
        }
        SttTarget::Cloud => {
            if let Ok(mut slot) = state.transcribing.lock() {
                if let Some(r) = slot.as_mut() {
                    r.cloud_patient = Some(patient_id.clone());
                }
            }
            let provider = config.cloud_provider;
            let model = config
                .model()
                .ok_or_else(|| CmdError::Other("Kein Cloud-Sprachmodell gewählt (Einstellungen → Transkription)".into()))?;
            let (endpoint, _) = llm_endpoint(state, provider, &AtomicBool::new(false))?;
            let engine = SttEngine::Cloud(CloudStt { endpoint, model: model.clone(), language: source.language.clone() });
            let result = stt::transcribe(source.kind, consent, &dir, &key, &engine, &work, on_progress, cancel);
            let reason = if matches!(result, Err(thera_core::Error::Cancelled)) { "cancelled" } else { "error" };
            let usage = result.as_ref().map(|t| t.cloud.clone().unwrap_or_default()).map_err(|_| reason);
            state.with(|s| s.log_stt_cloud(source_id, provider, &model, usage.as_ref().map_err(|r| *r)))?;
            Ok((format!("cloud:{}", provider_str(provider)), model, result?.segments))
        }
    }
}

/// The local sidecar with the chosen speech model: (engine, model id, runner), or why not.
fn local_stt(state: &AppState, language: &str) -> CmdResult<(String, String, WhisperCli)> {
    let engine = state
        .engine()
        .ok_or_else(|| CmdError::Other("Das Transkriptionsprogramm fehlt in dieser Installation".into()))?;
    let model_id = active_model(state)?;
    let model = stt::model(&model_id)?;
    let model_path = state.models_dir().join(model.file);
    if !model_path.exists() {
        return Err(CmdError::Other("Kein Sprachmodell installiert (Einstellungen → Transkription)".into()));
    }
    // Leave one core for the UI; beyond 8 threads whisper.cpp stops getting faster.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(1).clamp(1, 8));
    // In the kill-on-close job, so a crashed app can't leave whisper-cli running.
    let on_spawn = state.job.clone().map(|job| -> Arc<dyn Fn(&std::process::Child) + Send + Sync> {
        Arc::new(move |child| {
            job.assign(child);
        })
    });
    let cli = WhisperCli { exe: state.whisper_dir().join("whisper-cli.exe"), model: model_path, language: language.to_string(), threads, on_spawn };
    Ok((engine, model_id, cli))
}

fn active_model(state: &AppState) -> CmdResult<String> {
    let chosen = state.with(|s| s.setting("stt_model"))?.and_then(|v| v.as_str().map(str::to_string));
    Ok(chosen.unwrap_or_else(|| stt::DEFAULT_MODEL.to_string()))
}

// ---------- speech models ----------

#[derive(Serialize)]
struct SttOverview {
    /// `None`: the sidecar is missing from this build.
    engine: Option<String>,
    active: String,
    models: Vec<ModelView>,
    config: SttConfig,
    cloud_models: &'static [CloudSttModel],
    /// Why the cloud can't be used right now (no key, no endpoint, no model), or `None`.
    cloud_problem: Option<String>,
    /// Whether the local engine and model are ready (decides "Ausweichlösung").
    local_ready: bool,
}

#[derive(Serialize)]
struct ModelView {
    id: &'static str,
    file: &'static str,
    bytes: u64,
    installed: bool,
    transfer: Option<Transfer>,
}

#[tauri::command]
fn stt_overview(state: State<'_, AppState>) -> CmdResult<SttOverview> {
    let transfers = lock(&state.model_transfers)?.clone();
    let models = stt::MODELS
        .iter()
        .map(|m| ModelView {
            id: m.id,
            file: m.file,
            bytes: m.bytes,
            installed: model_installed(&state, m),
            transfer: transfers.get(m.id).cloned(),
        })
        .collect();
    let config = state.with(|s| s.stt_config())?;
    let cloud_problem = match config.model() {
        None => Some("Kein Cloud-Sprachmodell eingetragen".to_string()),
        Some(_) => llm_endpoint(&state, config.cloud_provider, &AtomicBool::new(false)).err().map(|e| e.to_string()),
    };
    Ok(SttOverview {
        engine: state.engine(),
        active: active_model(&state)?,
        models,
        local_ready: local_stt(&state, "de").is_ok(),
        config,
        cloud_models: stt::CLOUD_MODELS,
        cloud_problem,
    })
}

#[tauri::command]
fn stt_config_set(state: State<'_, AppState>, config: SttConfig) -> CmdResult<()> {
    state.with(|s| s.set_stt_config(&config))
}

#[tauri::command]
fn stt_model_select(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    stt::model(&id)?;
    state.with(|s| s.set_setting("stt_model", &serde_json::json!(id)))
}

#[tauri::command]
fn stt_model_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    delete_model(&state, stt::model(&id)?)
}

#[tauri::command]
fn stt_model_download(app: AppHandle, id: String) -> CmdResult<()> {
    download_model(&app, *stt::model(&id)?)
}

#[tauri::command]
fn stt_model_import(app: AppHandle, id: String, path: String) -> CmdResult<()> {
    import_model(&app, *stt::model(&id)?, path)
}

/// Fetches a model file from Hugging Face. Only the model travels; no patient data is sent.
fn download_model(app: &AppHandle, m: ModelInfo) -> CmdResult<()> {
    start_transfer(app, m, Box::new(move || {
        let resp = ureq::get(m.url).call().map_err(|e| format!("Download fehlgeschlagen: {e}"))?;
        Ok(Box::new(resp.into_body().into_with_config().limit(m.bytes + 1).reader()) as Box<dyn std::io::Read>)
    }))
}

/// Copies a model file the user already has (offline installs). It must match the catalog hash.
fn import_model(app: &AppHandle, m: ModelInfo, path: String) -> CmdResult<()> {
    start_transfer(app, m, Box::new(move || {
        Ok(Box::new(std::fs::File::open(&path).map_err(|e| e.to_string())?) as Box<dyn std::io::Read>)
    }))
}

/// Removes a model file unless it is being transferred.
fn delete_model(state: &AppState, m: &ModelInfo) -> CmdResult<()> {
    if lock(&state.model_transfers)?.get(m.id).is_some_and(|t| t.error.is_none()) {
        return Err(CmdError::Other("Das Modell wird gerade übertragen".into()));
    }
    match std::fs::remove_file(state.models_dir().join(m.file)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(thera_core::Error::from(e).into()),
        _ => Ok(()),
    }
}

/// Size only; the full hash is checked once, when the file arrives.
fn model_installed(state: &AppState, m: &ModelInfo) -> bool {
    std::fs::metadata(state.models_dir().join(m.file)).is_ok_and(|md| md.len() == m.bytes)
}

type OpenSource = Box<dyn FnOnce() -> Result<Box<dyn std::io::Read>, String> + Send>;

/// Runs a download or import on its own thread; progress is read through `stt_overview` /
/// `local_overview` (speech and language model ids don't overlap).
fn start_transfer(app: &AppHandle, m: ModelInfo, open: OpenSource) -> CmdResult<()> {
    let state = app.state::<AppState>();
    if let Some(free) = power::free_space(&state.models_dir()) {
        let need = m.bytes + (1 << 30);
        if free < need {
            let gb = |b: u64| format!("{:.1}", b as f64 / 1e9).replace('.', ",");
            return Err(CmdError::Other(format!("Nicht genug Speicherplatz: {} GB frei, {} GB nötig", gb(free), gb(need))));
        }
    }
    {
        let mut transfers = lock(&state.model_transfers)?;
        if transfers.get(m.id).is_some_and(|t| t.error.is_none()) {
            return Err(CmdError::Other("Das Modell wird bereits übertragen".into()));
        }
        transfers.insert(m.id.to_string(), Transfer { received: 0, total: m.bytes, error: None });
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let result = transfer_model(&state, &m, open);
        let Ok(mut transfers) = state.model_transfers.lock() else { return };
        match result {
            Ok(()) => {
                transfers.remove(m.id);
            }
            Err(e) => {
                if let Some(t) = transfers.get_mut(m.id) {
                    t.error = Some(e);
                }
            }
        }
    });
    Ok(())
}

/// Streams into `<file>.part` while hashing, and renames only if the SHA-256 matches.
fn transfer_model(state: &AppState, m: &ModelInfo, open: OpenSource) -> Result<(), String> {
    use std::io::{Read, Write};
    std::fs::create_dir_all(state.models_dir()).map_err(|e| e.to_string())?;
    let dest = state.models_dir().join(m.file);
    let part = dest.with_extension("part");
    let result = (|| {
        let mut src = open()?;
        let mut out = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut received = 0u64;
        loop {
            let n = src.read(&mut buf).map_err(|e| format!("Übertragung abgebrochen: {e}"))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            received += n as u64;
            if let Some(t) = state.model_transfers.lock().ok().as_mut().and_then(|t| t.get_mut(m.id)) {
                t.received = received;
            }
        }
        out.sync_all().map_err(|e| e.to_string())?;
        drop(out);
        if hex::encode(hasher.finalize()) != m.sha256 {
            return Err("Prüfsumme stimmt nicht: Datei ist beschädigt oder ein anderes Modell".into());
        }
        std::fs::rename(&part, &dest).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

// ---------- episodes / applications / sessions ----------

#[tauri::command]
fn episodes_list(state: State<'_, AppState>, patient_id: String) -> CmdResult<Vec<Episode>> {
    state.with(|s| s.list_episodes(&patient_id))
}

#[tauri::command]
fn episode_create(state: State<'_, AppState>, input: EpisodeInput) -> CmdResult<Episode> {
    state.with(|s| s.create_episode(input))
}

#[tauri::command]
fn episode_diagnoses_set(state: State<'_, AppState>, episode_id: String, diagnoses: Vec<Diagnosis>) -> CmdResult<Episode> {
    state.with(|s| s.set_episode_diagnoses(&episode_id, diagnoses))
}

#[tauri::command]
fn applications_list(state: State<'_, AppState>, episode_id: String) -> CmdResult<Vec<Application>> {
    state.with(|s| s.list_applications(&episode_id))
}

#[tauri::command]
fn application_create(state: State<'_, AppState>, episode_id: String, kind: ApplicationKind) -> CmdResult<Application> {
    state.with(|s| s.create_application(&episode_id, kind))
}

#[tauri::command]
fn sessions_list(state: State<'_, AppState>, patient_id: Option<String>) -> CmdResult<Vec<Session>> {
    state.with(|s| s.list_sessions(patient_id.as_deref()))
}

#[tauri::command]
fn session_create(state: State<'_, AppState>, input: SessionInput) -> CmdResult<Session> {
    state.with(|s| s.create_session(input))
}

// ---------- calendar (slice 22) ----------

#[tauri::command]
fn calendar_range(state: State<'_, AppState>, from: String, to: String) -> CmdResult<Vec<CalendarSession>> {
    state.with(|s| s.sessions_between(&from, &to))
}

/// The Sitzungen work queue: every session with its flow state, avatar and summary head.
#[tauri::command]
fn session_queue(state: State<'_, AppState>) -> CmdResult<Vec<QueueSession>> {
    state.with(|s| s.session_queue())
}

/// Writes the sessions in `[from, to)` as .ics (Chiffre only) to `path`; returns how many.
#[tauri::command]
fn calendar_export_ics(state: State<'_, AppState>, from: String, to: String, path: String) -> CmdResult<usize> {
    state.with(|s| {
        let sessions = s.sessions_between(&from, &to)?;
        std::fs::write(&path, thera_core::calendar::ics(&sessions, &thera_core::domain::now())?)?;
        s.log_calendar_export(sessions.len())?;
        Ok(sessions.len())
    })
}

#[tauri::command]
fn sessions_create_series(state: State<'_, AppState>, inputs: Vec<SessionInput>) -> CmdResult<Vec<Session>> {
    state.with(|s| s.create_sessions(&inputs))
}

#[tauri::command]
fn session_update(state: State<'_, AppState>, id: String, update: SessionUpdate) -> CmdResult<Session> {
    state.with(|s| s.update_session(&id, update))
}

#[tauri::command]
fn session_cancel(state: State<'_, AppState>, id: String, reason: Option<String>) -> CmdResult<Session> {
    state.with(|s| s.cancel_session(&id, reason.as_deref()))
}

#[tauri::command]
fn session_reopen(state: State<'_, AppState>, id: String) -> CmdResult<Session> {
    state.with(|s| s.reopen_session(&id))
}

#[tauri::command]
fn session_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.with(|s| s.delete_session(&id))
}

/// "Fällig" on the Heute page; see `Store::due_items`.
#[tauri::command]
fn due_items(state: State<'_, AppState>, from: String, day_start: String, until: String) -> CmdResult<Vec<DueItem>> {
    state.with(|s| s.due_items(&from, &day_start, &until))
}

#[tauri::command]
fn tasks_list(state: State<'_, AppState>, from: String, to: String, open_before: bool) -> CmdResult<Vec<Task>> {
    state.with(|s| s.list_tasks(&from, &to, open_before))
}

#[tauri::command]
fn task_create(state: State<'_, AppState>, input: TaskInput) -> CmdResult<Task> {
    state.with(|s| s.create_task(input))
}

#[tauri::command]
fn task_update(state: State<'_, AppState>, id: String, input: TaskInput) -> CmdResult<Task> {
    state.with(|s| s.update_task(&id, input))
}

#[tauri::command]
fn task_set_done(state: State<'_, AppState>, id: String, done: bool) -> CmdResult<Task> {
    state.with(|s| s.set_task_done(&id, done))
}

#[tauri::command]
fn task_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.with(|s| s.delete_task(&id))
}

#[tauri::command]
fn day_note_get(state: State<'_, AppState>, date: String) -> CmdResult<Option<String>> {
    state.with(|s| s.day_note(&date))
}

#[tauri::command]
fn day_note_set(state: State<'_, AppState>, date: String, body: String) -> CmdResult<()> {
    state.with(|s| s.set_day_note(&date, &body))
}

/// Creates the session's Sitzungsnotiz (or returns the existing one).
#[tauri::command]
fn session_note_create(state: State<'_, AppState>, session_id: String) -> CmdResult<Report> {
    state.with(|s| s.create_session_note(&session_id))
}

#[tauri::command]
fn templates_list(state: State<'_, AppState>) -> CmdResult<Vec<Template>> {
    state.with(|s| s.list_templates())
}

// ---------- reports ----------

/// Everything the Bericht editor needs in one round-trip.
#[derive(Serialize)]
struct ReportView {
    report: Report,
    template: Template,
    patient: Patient,
    episode: Option<Episode>,
    application: Option<Application>,
    /// Sitzungsnotiz only: its session and the confirmed transcript segments to pick from.
    session: Option<Session>,
    segments: Vec<SourceSegment>,
}

fn report_view(s: &Store, id: &str) -> thera_core::Result<ReportView> {
    let report = s.report(id)?;
    let patient = s.patient(&report.patient_id)?;
    let episode = report.episode_id.as_deref().map(|e| s.episode(e)).transpose()?;
    let application = match (&episode, &report.application_id) {
        (Some(ep), Some(app_id)) => s.list_applications(&ep.id)?.into_iter().find(|a| &a.id == app_id),
        _ => None,
    };
    let session = report.session_id.as_deref().map(|id| s.session(id)).transpose()?;
    let segments = match &session {
        Some(sess) => s.source_segments(&sess.id)?,
        None => vec![],
    };
    let mut template = s.template(&report.template_key)?;
    if let Some(ep) = &episode {
        // A Bericht resolves for its application kind; a Sitzungsnotiz has none.
        let kind = application.as_ref().map(|a| a.kind);
        if kind.is_some() || session.is_some() {
            template = template.resolve(&Context { method: ep.method, age_group: ep.age_group, kind });
        }
    }
    Ok(ReportView { report, template, patient, episode, application, session, segments })
}

#[tauri::command]
fn reports_list(state: State<'_, AppState>, status: Option<ReportStatus>) -> CmdResult<Vec<ReportSummary>> {
    state.with(|s| s.list_reports(status, &thera_core::domain::now()))
}

#[tauri::command]
fn report_get(state: State<'_, AppState>, id: String) -> CmdResult<ReportView> {
    state.with(|s| report_view(s, &id))
}

#[tauri::command]
fn report_save_section(state: State<'_, AppState>, id: String, key: String, body: String) -> CmdResult<()> {
    state.with(|s| s.save_section(&id, &key, &body))
}

#[tauri::command]
fn report_checklist(state: State<'_, AppState>, id: String) -> CmdResult<Vec<CheckItem>> {
    state.with(|s| {
        let v = report_view(s, &id)?;
        Ok(pii::checklist(&v.report, &v.template, &v.patient))
    })
}

#[tauri::command]
fn report_sign(state: State<'_, AppState>, id: String, signed_by: String) -> CmdResult<Report> {
    if signed_by.trim().is_empty() {
        return Err(CmdError::Other("Name für die Unterschrift fehlt".into()));
    }
    state.with(|s| {
        let v = report_view(s, &id)?;
        let blocking: Vec<_> = pii::checklist(&v.report, &v.template, &v.patient)
            .into_iter()
            .filter(|c| !c.ok && c.severity == "block")
            .map(|c| c.message)
            .collect();
        if !blocking.is_empty() {
            return Err(thera_core::Error::Invalid(format!("Checkliste nicht erfüllt: {}", blocking.join("; "))));
        }
        let signed = s.sign_report(&id, signed_by.trim())?;
        // Autopilot: a note that changed a lot since its summary gets a fresh one.
        if let Some(session_id) = &signed.session_id {
            if s.autopilot_enabled()? && s.summary_refresh_due(session_id)? && s.enqueue_autopilot(session_id, AutopilotTrigger::Signed)?.is_some() {
                state.wake_worker();
            }
        }
        Ok(signed)
    })
}

/// Writes PDF or DOCX to `path` (chosen by the user in a save dialog).
/// Nachtrag to a signed Sitzungsnotiz, signed on submit. Returns the updated note.
#[tauri::command]
fn report_add_addendum(state: State<'_, AppState>, id: String, body: String, signed_by: String) -> CmdResult<Report> {
    state.with(|s| s.add_addendum(&id, &body, &signed_by))
}

fn render_doc(s: &Store, id: &str) -> thera_core::Result<(ReportView, render::RenderDoc)> {
    let v = report_view(s, id)?;
    let practice = s.setting("practice_header")?.and_then(|v| v.as_str().map(str::to_string));
    let doc = render::build_doc(&v.report, &v.template, &v.patient, v.episode.as_ref(), v.application.as_ref(), v.session.as_ref(), practice);
    Ok((v, doc))
}

/// The pages an export would write right now, as PNG data URLs for the editor's preview.
/// Nothing is written to disk or marked as exported.
#[tauri::command]
async fn report_preview(app: AppHandle, id: String, scale: f32) -> CmdResult<Vec<String>> {
    use base64::Engine as _;
    blocking(app, move |state| {
        let (_, doc) = state.with(|s| render_doc(s, &id))?;
        let pages = render::preview_pngs(&doc, scale.clamp(0.5, 4.0))?;
        Ok(pages.iter().map(|png| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))).collect())
    })
    .await
}

#[tauri::command]
fn report_export(state: State<'_, AppState>, id: String, format: String, path: String) -> CmdResult<()> {
    state.with(|s| {
        let (v, doc) = render_doc(s, &id)?;
        let bytes = match format.as_str() {
            "pdf" => render::pdf(&doc)?,
            "docx" => render::docx(&doc)?,
            other => return Err(thera_core::Error::Invalid(format!("unknown format {other}"))),
        };
        std::fs::write(&path, &bytes)?;
        if v.report.status == ReportStatus::Signed || v.report.status == ReportStatus::Exported {
            s.mark_exported(&id, &format, &hex::encode(Sha256::digest(&bytes)))?;
        }
        Ok(())
    })
}

// ---------- audit & settings ----------

#[tauri::command]
fn audit_list(state: State<'_, AppState>, limit: Option<i64>) -> CmdResult<Vec<AuditEvent>> {
    state.with(|s| thera_core::audit::list(s.conn(), limit.unwrap_or(200)))
}

#[tauri::command]
fn audit_verify(state: State<'_, AppState>) -> CmdResult<Option<i64>> {
    state.with(|s| thera_core::audit::verify(s.conn()))
}

#[tauri::command]
fn setting_get(state: State<'_, AppState>, key: String) -> CmdResult<Option<serde_json::Value>> {
    state.with(|s| s.setting(&key))
}

#[tauri::command]
fn setting_set(state: State<'_, AppState>, key: String, value: serde_json::Value) -> CmdResult<()> {
    state.with(|s| s.set_setting(&key, &value))
}

// ---------- llm (Phase 2) ----------

/// Keychain entry for a provider's API key. Debug runs with `THERA_KEYRING_USER` get their own.
fn llm_key_entry(state: &AppState, id: ProviderId) -> CmdResult<keyring::Entry> {
    let id = serde_json::to_value(id).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let user = if state.keyring_user == KEYRING_USER { format!("llm-{id}") } else { format!("{}-llm-{id}", state.keyring_user) };
    keyring::Entry::new(KEYRING_SERVICE, &user).map_err(|e| kr(e).into())
}

fn llm_key(state: &AppState, id: ProviderId) -> CmdResult<Option<String>> {
    match llm_key_entry(state, id)?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(kr(e).into()),
    }
}

/// `cancel` stops a local model that is still loading.
fn llm_endpoint(state: &AppState, id: ProviderId, cancel: &AtomicBool) -> CmdResult<(Endpoint, LlmConfig)> {
    let config = state.with(|s| s.llm_config())?;
    if id == ProviderId::Local {
        let model = config
            .settings(id)
            .model
            .filter(|m| !m.is_empty())
            .ok_or_else(|| CmdError::Other("Kein lokales Modell gewählt".into()))?;
        return Ok((local_endpoint(state, &model, cancel)?, config));
    }
    let api_key = llm_key(state, id)?;
    if llm::provider(id).needs_key && api_key.is_none() {
        return Err(CmdError::Other("Kein API-Schlüssel hinterlegt".into()));
    }
    let base_url = config.base_url(id);
    if base_url.is_empty() {
        return Err(CmdError::Other("Kein Endpunkt eingetragen".into()));
    }
    Ok((Endpoint { provider: id, base_url, api_key }, config))
}

#[derive(Serialize)]
struct LlmProviderView {
    #[serde(flatten)]
    info: ProviderInfo,
    has_key: bool,
}

#[derive(Serialize)]
struct LlmOverview {
    config: LlmConfig,
    providers: Vec<LlmProviderView>,
}

#[tauri::command]
fn llm_overview(state: State<'_, AppState>) -> CmdResult<LlmOverview> {
    let config = state.with(|s| s.llm_config())?;
    let providers = llm::providers()
        .into_iter()
        .map(|info| Ok(LlmProviderView { has_key: llm_key(&state, info.id)?.is_some(), info }))
        .collect::<CmdResult<_>>()?;
    Ok(LlmOverview { config, providers })
}

#[tauri::command]
fn llm_config_set(state: State<'_, AppState>, config: LlmConfig) -> CmdResult<()> {
    state.with(|s| s.set_llm_config(&config))?;
    // Switching away from a loaded local model frees its memory right away.
    let wanted = (config.active == Some(ProviderId::Local)).then(|| config.settings(ProviderId::Local).model).flatten();
    let mut slot = lock(&state.llama)?;
    if slot.as_ref().is_some_and(|s| Some(s.model_id) != wanted.as_deref()) {
        *slot = None;
    }
    Ok(())
}

/// Stores (or with `None`/empty removes) a provider's API key in the OS keychain.
#[tauri::command]
fn llm_key_set(state: State<'_, AppState>, provider: ProviderId, key: Option<String>) -> CmdResult<()> {
    let entry = llm_key_entry(&state, provider)?;
    match key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
        Some(k) => entry.set_password(&k).map_err(kr)?,
        None => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(kr(e).into()),
        },
    }
    Ok(())
}

#[tauri::command]
async fn llm_models(app: AppHandle, provider: ProviderId) -> CmdResult<Vec<RemoteModel>> {
    blocking(app, move |state| {
        if provider == ProviderId::Local {
            // The installed catalog models; listing them must not load one.
            return Ok(local::MODELS
                .iter()
                .filter(|m| model_installed(state, &m.file))
                .map(|m| RemoteModel { id: m.file.id.into(), name: Some(m.label.into()), context_length: Some(m.ctx), tools: Some(true) })
                .collect());
        }
        let (ep, _) = llm_endpoint(state, provider, &AtomicBool::new(false))?;
        Ok(llm::list_models(&ep)?)
    })
    .await
}

#[derive(Serialize)]
struct LlmTestResult {
    model: String,
    latency_ms: u64,
    answer: String,
}

/// Sends a fixed prompt (no patient data) to the provider's chosen model.
#[tauri::command]
async fn llm_test(app: AppHandle, provider: ProviderId) -> CmdResult<LlmTestResult> {
    blocking(app, move |state| {
        let (ep, config) = llm_endpoint(state, provider, &AtomicBool::new(false))?;
        let model = config.settings(provider).model.filter(|m| !m.is_empty()).ok_or_else(|| CmdError::Other("Kein Modell gewählt".into()))?;
        let result = llm::ping(&ep, &model);
        state.with(|s| s.log_llm_call("llm.test", provider, &model, result.as_ref().map_err(|_| "error")))?;
        let c = result?;
        Ok(LlmTestResult { model: c.model, latency_ms: c.latency_ms, answer: c.text.chars().take(200).collect() })
    })
    .await
}

#[derive(Serialize)]
struct LlmCatalog {
    models: Vec<CloudModel>,
    recommended: Tier,
}

/// The curated OpenRouter models with tiers and cost estimates.
#[tauri::command]
fn llm_catalog() -> LlmCatalog {
    LlmCatalog { models: catalog::models(), recommended: catalog::RECOMMENDED }
}

#[derive(Serialize)]
struct LlmSpendView {
    month: thera_core::store::LlmSpend,
    total: thera_core::store::LlmSpend,
}

/// Billed cost of AI calls this month (UTC) and overall, from the audit log.
#[tauri::command]
fn llm_spend(state: State<'_, AppState>) -> CmdResult<LlmSpendView> {
    let month: String = now().chars().take(7).collect();
    Ok(state.with(|s| Ok(LlmSpendView { month: s.llm_spend(&month)?, total: s.llm_spend("")? }))?)
}

// ---------- read-aloud (slice 18) ----------

#[derive(Serialize)]
struct TtsOverview {
    config: TtsConfig,
    voices: &'static [tts::TtsVoice],
    default_models: std::collections::BTreeMap<ProviderId, &'static str>,
    /// Why read-aloud can't run with this config (no key, no endpoint, no model), or `None`.
    problem: Option<String>,
}

fn tts_problem(state: &AppState, config: &TtsConfig) -> Option<String> {
    match config.model() {
        None => Some("Kein Sprachmodell eingetragen".to_string()),
        Some(_) => llm_endpoint(state, config.provider, &AtomicBool::new(false)).err().map(|e| e.to_string()),
    }
}

#[tauri::command]
fn tts_overview(state: State<'_, AppState>) -> CmdResult<TtsOverview> {
    let config = state.with(|s| s.tts_config())?;
    let default_models = [ProviderId::Openrouter, ProviderId::Openai]
        .into_iter()
        .filter_map(|p| Some((p, tts::default_model(p)?)))
        .collect();
    Ok(TtsOverview { problem: tts_problem(&state, &config), config, voices: tts::VOICES, default_models })
}

#[tauri::command]
fn tts_config_set(state: State<'_, AppState>, config: TtsConfig) -> CmdResult<()> {
    state.with(|s| s.set_tts_config(&config))
}

/// Speaks a fixed sentence (no patient data) with the saved config, even while read-aloud is
/// off, so a voice can be tried first. Returns MP3 bytes.
#[tauri::command]
async fn tts_sample(app: AppHandle) -> CmdResult<tauri::ipc::Response> {
    blocking(app, move |state| {
        let config = state.with(|s| s.tts_config())?;
        let model = config.model().ok_or_else(|| CmdError::Other("Kein Sprachmodell eingetragen".into()))?;
        let (ep, _) = llm_endpoint(state, config.provider, &AtomicBool::new(false))?;
        let result = tts::sample(&ep, &model, &config.voice());
        state.with(|s| s.log_tts(config.provider, &model, result.as_ref().map_err(|_| "error")))?;
        Ok(tauri::ipc::Response::new(result?.audio))
    })
    .await
}

// ---------- fact extraction (Phase 2) ----------

fn provider_str(id: ProviderId) -> String {
    serde_json::to_value(id).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// The provider chosen in Settings and its model.
fn active_llm(state: &AppState) -> CmdResult<(ProviderId, String)> {
    let config = state.with(|s| s.llm_config())?;
    let id = config.active.ok_or_else(|| CmdError::Other("Kein KI-Anbieter gewählt (Einstellungen → KI-Entwürfe)".into()))?;
    let model = config.settings(id).model.filter(|m| !m.is_empty()).ok_or_else(|| CmdError::Other("Kein Modell gewählt".into()))?;
    Ok((id, model))
}

#[derive(Serialize)]
struct ExtractPreview {
    #[serde(flatten)]
    preview: extract::Preview,
    terms: Vec<PseudoTerm>,
    provider: Option<ProviderInfo>,
    model: Option<String>,
}

/// The redaction preview: the pseudonymized text per request and the token table (local only).
#[tauri::command]
fn extract_preview(state: State<'_, AppState>, report_id: String) -> CmdResult<ExtractPreview> {
    let (prepared, _) = state.with(|s| autopilot::prepare_extraction(s, &report_id))?;
    let report = state.with(|s| s.report(&report_id))?;
    let terms = state.with(|s| s.pseudo_terms(&report.patient_id))?;
    let config = state.with(|s| s.llm_config())?;
    let provider = config.active.map(llm::provider);
    let model = config.active.and_then(|id| config.settings(id).model).filter(|m| !m.is_empty());
    Ok(ExtractPreview { preview: prepared.preview(), terms, provider, model })
}

#[tauri::command]
fn pseudo_term_add(state: State<'_, AppState>, patient_id: String, kind: EntityKind, text: String) -> CmdResult<PseudoTerm> {
    state.with(|s| s.add_pseudo_term(&patient_id, kind, &text))
}

#[tauri::command]
fn pseudo_term_remove(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.with(|s| s.remove_pseudo_term(&id))
}

#[tauri::command]
fn note_facts(state: State<'_, AppState>, report_id: String) -> CmdResult<Vec<NoteFact>> {
    state.with(|s| s.note_facts(&report_id))
}

/// The Bericht sections this note's facts can be tagged with (empty without an episode).
#[tauri::command]
fn fact_tag_sections(state: State<'_, AppState>, report_id: String) -> CmdResult<Vec<thera_core::templates::SectionDef>> {
    state.with(|s| {
        let report = s.report(&report_id)?;
        let episode = report.episode_id.as_deref().map(|e| s.episode(e)).transpose()?;
        Ok(autopilot::tag_template(s, episode.as_ref())?.map(|t| extract::tag_sections(&t).into_iter().cloned().collect()).unwrap_or_default())
    })
}

/// A running extraction or draft that `llm_cancel` can stop. Unregisters itself when dropped.
struct LlmRun<'a> {
    state: &'a AppState,
    id: String,
    cancel: Arc<AtomicBool>,
}

impl<'a> LlmRun<'a> {
    fn start(state: &'a AppState, id: &str) -> CmdResult<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        lock(&state.llm_runs)?.insert(id.to_string(), cancel.clone());
        Ok(Self { state, id: id.to_string(), cancel })
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

impl Drop for LlmRun<'_> {
    fn drop(&mut self) {
        if let Ok(mut runs) = self.state.llm_runs.lock() {
            runs.remove(&self.id);
        }
    }
}

/// Stops a running extraction or draft. What finished before is kept.
#[tauri::command]
fn llm_cancel(state: State<'_, AppState>, request_id: String) -> CmdResult<()> {
    if let Some(cancel) = lock(&state.llm_runs)?.get(&request_id) {
        cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// One piece of streamed answer text (`llm:delta`). Still tokenized: it is shown as progress only,
/// never stored.
#[derive(Clone, Serialize)]
struct LlmDelta<'a> {
    request_id: &'a str,
    text: &'a str,
}

/// Streams one request, forwarding deltas to the UI, and logs it. `Ok(None)` = cancelled.
#[allow(clippy::too_many_arguments)]
fn stream_call(
    state: &AppState,
    events: &AppHandle,
    run: &LlmRun,
    action: &str,
    (provider, model): (ProviderId, &str),
    ep: &Endpoint,
    messages: &[(llm::Role, &thera_core::pseudo::Pseudonymized<String>)],
    opts: &llm::ChatOptions,
) -> CmdResult<Option<llm::Completion>> {
    let result = llm::chat_stream(ep, model, messages, opts, &run.cancel, |text| {
        let _ = events.emit("llm:delta", LlmDelta { request_id: &run.id, text });
    });
    let reason = if matches!(result, Err(thera_core::Error::Cancelled)) { "cancelled" } else { "error" };
    state.with(|s| s.log_llm_call(action, provider, model, result.as_ref().map_err(|_| reason)))?;
    match result {
        Err(thera_core::Error::Cancelled) => Ok(None),
        r => Ok(Some(r?)),
    }
}

/// The app side of `core::autopilot`: store access, the model (resolved on first use) and the
/// streamed requests with their audit entry. The manual buttons use it with a progress event.
struct LlmHost<'a> {
    state: &'a AppState,
    events: &'a AppHandle,
    cancel: Arc<AtomicBool>,
    /// Manual runs show the streamed text (`llm:delta`) under this id.
    request_id: Option<String>,
    llm: Option<(ProviderId, String, Endpoint)>,
    progress: Option<Box<dyn Fn(usize, usize) + 'a>>,
    /// Autopilot: announce each step (`autopilot-changed`).
    changed_session: Option<String>,
}

impl<'a> LlmHost<'a> {
    fn new(state: &'a AppState, events: &'a AppHandle, cancel: Arc<AtomicBool>) -> Self {
        Self { state, events, cancel, request_id: None, llm: None, progress: None, changed_session: None }
    }
}

fn core_err(e: CmdError) -> thera_core::Error {
    match e {
        CmdError::Core(c) => c,
        CmdError::Other(m) => thera_core::Error::Invalid(m),
    }
}

impl autopilot::Host for LlmHost<'_> {
    fn store<T>(&mut self, f: impl FnOnce(&Store) -> thera_core::Result<T>) -> thera_core::Result<T> {
        let r = self.state.with(f).map_err(core_err);
        if let Some(id) = &self.changed_session {
            let _ = self.events.emit("autopilot-changed", AutopilotChanged { session_id: id });
        }
        r
    }

    fn ready(&mut self) -> thera_core::Result<Option<(ProviderId, String)>> {
        if let Some((p, m, _)) = &self.llm {
            return Ok(Some((*p, m.clone())));
        }
        // No provider or model chosen: the AI steps are skipped, not failed.
        let Ok((provider, model)) = active_llm(self.state) else { return Ok(None) };
        let (ep, _) = llm_endpoint(self.state, provider, &self.cancel).map_err(core_err)?;
        self.llm = Some((provider, model.clone(), ep));
        Ok(Some((provider, model)))
    }

    fn complete(&mut self, action: &str, messages: &[(llm::Role, &thera_core::pseudo::Pseudonymized<String>)], opts: &llm::ChatOptions) -> thera_core::Result<Option<llm::Completion>> {
        if self.cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let (provider, model, ep) = self.llm.as_ref().ok_or_else(|| thera_core::Error::Llm("Kein Modell bereit".into()))?;
        let result = llm::chat_stream(ep, model, messages, opts, &self.cancel, |text| {
            if let Some(id) = &self.request_id {
                let _ = self.events.emit("llm:delta", LlmDelta { request_id: id, text });
            }
        });
        let reason = if matches!(result, Err(thera_core::Error::Cancelled)) { "cancelled" } else { "error" };
        self.state.with(|s| s.log_llm_call(action, *provider, model, result.as_ref().map_err(|_| reason))).map_err(core_err)?;
        match result {
            Err(thera_core::Error::Cancelled) => Ok(None),
            r => r.map(Some),
        }
    }

    fn progress(&mut self, done: usize, total: usize) {
        if let Some(p) = &self.progress {
            p(done, total);
        }
    }
}

#[derive(Clone, Serialize)]
struct ExtractProgress {
    report_id: String,
    /// `loading` (local model starting) or `running`.
    stage: &'static str,
    done: usize,
    total: usize,
}

#[derive(Serialize)]
struct ExtractResult {
    facts: Vec<NoteFact>,
    dropped: usize,
    /// Chunks answered; fewer than `total` when cancelled.
    chunks: usize,
    total: usize,
    cancelled: bool,
}

/// Sends the previewed text to the active provider, one streamed request per chunk, and stores
/// the checked facts. `digest` must match the preview the therapist saw; nothing is stored if any
/// request fails. `llm_cancel(request_id)` stops it: facts from finished chunks are kept as
/// proposals.
#[tauri::command]
async fn extract_run(app: AppHandle, report_id: String, digest: String, request_id: String) -> CmdResult<ExtractResult> {
    let events = app.clone();
    blocking(app, move |state| {
        let run = LlmRun::start(state, &request_id)?;
        let (provider, model) = active_llm(state)?;
        let (prepared, scope) = state.with(|s| autopilot::prepare_extraction(s, &report_id))?;
        if prepared.digest != digest {
            return Err(CmdError::Other("Der Text hat sich seit der Vorschau geändert. Bitte die Vorschau erneut prüfen.".into()));
        }
        let total = prepared.chunks.len();
        let progress = |stage, done| {
            let _ = events.emit("extract-progress", ExtractProgress { report_id: report_id.clone(), stage, done, total });
        };
        if provider == ProviderId::Local {
            progress("loading", 0);
        }
        let nothing = || Ok(ExtractResult { facts: state.with(|s| s.note_facts(&report_id))?, dropped: 0, chunks: 0, total, cancelled: true });
        let ep = match llm_endpoint(state, provider, &run.cancel) {
            Err(_) if run.cancelled() => return nothing(),
            r => r?.0,
        };
        let mut host = LlmHost::new(state, &events, run.cancel.clone());
        host.request_id = Some(request_id.clone());
        host.llm = Some((provider, model.clone(), ep));
        host.progress = Some(Box::new(|done, _| progress("running", done)));
        match autopilot::extract(&mut host, (provider, &model), &report_id, &prepared, &scope)? {
            None => nothing(),
            Some(o) => Ok(ExtractResult { facts: o.facts, dropped: o.dropped, chunks: o.chunks, total, cancelled: o.chunks < o.total }),
        }
    })
    .await
}

#[tauri::command]
fn note_facts_reject(state: State<'_, AppState>, report_id: String, ids: Vec<String>) -> CmdResult<Vec<NoteFact>> {
    state.with(|s| s.reject_facts(&report_id, &ids))
}

#[tauri::command]
fn note_facts_apply(state: State<'_, AppState>, report_id: String, facts: Vec<FactEdit>) -> CmdResult<Report> {
    state.with(|s| s.apply_facts(&report_id, &facts))
}

// ---------- session summary (Phase 4, slice 30) ----------

#[tauri::command]
fn session_summary(state: State<'_, AppState>, session_id: String) -> CmdResult<Option<SessionSummary>> {
    state.with(|s| s.session_summary(&session_id))
}

/// The redaction preview for a summary, like `extract_preview`.
#[tauri::command]
fn summary_preview(state: State<'_, AppState>, session_id: String) -> CmdResult<ExtractPreview> {
    let (prepared, input) = state.with(|s| autopilot::prepare_summary(s, &session_id))?;
    let terms = state.with(|s| s.pseudo_terms(&input.patient.id))?;
    let config = state.with(|s| s.llm_config())?;
    let provider = config.active.map(llm::provider);
    let model = config.active.and_then(|id| config.settings(id).model).filter(|m| !m.is_empty());
    Ok(ExtractPreview { preview: prepared.preview(), terms, provider, model })
}

#[derive(Serialize)]
struct SummaryResult {
    summary: Option<SessionSummary>,
    dropped: usize,
    cancelled: bool,
}

/// Sends the previewed text, one request per chunk, and replaces the session's summary with the
/// checked points. Progress goes out as `summary-progress` with the session id in `report_id`.
/// Cancelled or failed: the earlier summary stays.
#[tauri::command]
async fn summary_run(app: AppHandle, session_id: String, digest: String, request_id: String) -> CmdResult<SummaryResult> {
    let events = app.clone();
    blocking(app, move |state| {
        let run = LlmRun::start(state, &request_id)?;
        let (provider, model) = active_llm(state)?;
        let (prepared, input) = state.with(|s| autopilot::prepare_summary(s, &session_id))?;
        if prepared.digest != digest {
            return Err(CmdError::Other("Der Text hat sich seit der Vorschau geändert. Bitte die Vorschau erneut prüfen.".into()));
        }
        let total = prepared.chunks.len();
        let progress = |stage, done| {
            let _ = events.emit("summary-progress", ExtractProgress { report_id: session_id.clone(), stage, done, total });
        };
        if provider == ProviderId::Local {
            progress("loading", 0);
        }
        let unchanged = |state: &AppState| -> CmdResult<SummaryResult> {
            Ok(SummaryResult { summary: state.with(|s| s.session_summary(&session_id))?, dropped: 0, cancelled: true })
        };
        let ep = match llm_endpoint(state, provider, &run.cancel) {
            Err(_) if run.cancelled() => return unchanged(state),
            r => r?.0,
        };
        let mut host = LlmHost::new(state, &events, run.cancel.clone());
        host.request_id = Some(request_id.clone());
        host.llm = Some((provider, model.clone(), ep));
        host.progress = Some(Box::new(|done, _| progress("running", done)));
        match autopilot::summarize(&mut host, (provider, &model), &session_id, &prepared, &input)? {
            None => unchanged(state),
            Some(o) => Ok(SummaryResult { summary: Some(o.summary), dropped: o.dropped, cancelled: false }),
        }
    })
    .await
}

#[tauri::command]
fn summary_edit(state: State<'_, AppState>, session_id: String, points: Vec<summary::SummaryPoint>) -> CmdResult<SessionSummary> {
    state.with(|s| s.edit_summary(&session_id, points))
}

#[tauri::command]
fn patient_timeline(state: State<'_, AppState>, patient_id: String) -> CmdResult<Vec<TimelineGroup>> {
    state.with(|s| s.patient_timeline(&patient_id, &thera_core::domain::now()))
}

// ---------- autopilot + contingent (Phase 4, slice 31) ----------

#[tauri::command]
fn autopilot_get(state: State<'_, AppState>) -> CmdResult<bool> {
    state.with(|s| s.autopilot_enabled())
}

#[tauri::command]
fn autopilot_set(state: State<'_, AppState>, on: bool) -> CmdResult<()> {
    state.with(|s| s.set_autopilot(on))
}

#[tauri::command]
fn autopilot_run(state: State<'_, AppState>, session_id: String) -> CmdResult<Option<AutopilotRun>> {
    state.with(|s| s.autopilot_run(&session_id))
}

/// "Was wurde gesendet?" for one step of a run.
#[tauri::command]
fn autopilot_sent(state: State<'_, AppState>, job_id: String, step: AutopilotStepKind) -> CmdResult<Option<extract::Preview>> {
    state.with(|s| s.autopilot_sent(&job_id, step))
}

#[tauri::command]
fn autopilot_retry(state: State<'_, AppState>, job_id: String) -> CmdResult<Job> {
    let job = state.with(|s| s.retry_autopilot(&job_id))?;
    state.wake_worker();
    Ok(job)
}

#[tauri::command]
fn episode_stage_set(state: State<'_, AppState>, episode_id: String, stage: Stage, therapy_since: Option<String>) -> CmdResult<Episode> {
    state.with(|s| s.set_episode_stage(&episode_id, stage, therapy_since.as_deref()))
}

#[tauri::command]
fn episode_contingent(state: State<'_, AppState>, episode_id: String) -> CmdResult<Contingent> {
    state.with(|s| s.episode_contingent(&episode_id, &thera_core::domain::now()))
}

// ---------- Bericht drafting (Phase 2) ----------

/// What the therapist chose in the draft dialog.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DraftChoice {
    sections: Vec<String>,
    instruction: Option<String>,
    /// Sessions whose facts to use; `None` = all of the episode.
    session_ids: Option<Vec<String>>,
    /// Send each section's current text along ("ergänzen").
    #[serde(default)]
    with_existing: bool,
    /// Offer every fact to every section, ignoring the Bericht tags.
    #[serde(default)]
    all_facts: bool,
}

/// Exactly what drafting the chosen sections of this Bericht would send, from the episode's
/// applied facts of the chosen sessions.
fn prepare_draft(s: &Store, report_id: &str, choice: &DraftChoice) -> thera_core::Result<(draft::Prepared, Vec<EpisodeFact>, Vec<draft::InputFact>)> {
    let view = report_view(s, report_id)?;
    let (Some(episode), Some(application)) = (&view.episode, &view.application) else {
        return Err(thera_core::Error::Invalid("Entwürfe gibt es nur für Berichte zu einem Antrag".into()));
    };
    let mut facts = s.episode_facts(&episode.id)?;
    if let Some(ids) = &choice.session_ids {
        if ids.is_empty() {
            return Err(thera_core::Error::Invalid("Keine Sitzung gewählt".into()));
        }
        facts.retain(|f| ids.contains(&f.session_id));
    }
    let note = s.template(thera_core::templates::SESSION_NOTE_KEY)?;
    let input: Vec<draft::InputFact> = facts
        .iter()
        .map(|f| draft::InputFact {
            session_no: f.session_no,
            label: note.sections.iter().find(|d| d.key == f.section_key).map_or(f.section_key.clone(), |d| d.heading.clone()),
            text: f.text.clone(),
            bericht: f.bericht.clone(),
        })
        .collect();
    // Tags name the sections of the Erst-/Umwandlungsbericht; other templates take all facts.
    let tag_keys: Vec<String> = match (view.template.key == extract::TAG_TEMPLATE_KEY && !choice.all_facts, autopilot::tag_template(s, Some(episode))?) {
        (true, Some(t)) => extract::tag_sections(&t).iter().map(|d| d.key.clone()).collect(),
        _ => vec![],
    };
    let existing: Vec<(String, String)> = if choice.with_existing {
        view.report.sections.iter().map(|sec| (sec.key.clone(), draft::plain_text(&sec.body))).collect()
    } else {
        vec![]
    };
    let frame = draft::Frame { method: episode.method, age_group: episode.age_group, kind: application.kind };
    let dict = s.pseudo_dictionary(&view.patient.id)?;
    let req = draft::Request {
        keys: &choice.sections,
        facts: &input,
        tag_keys: &tag_keys,
        existing: &existing,
        instruction: choice.instruction.as_deref(),
    };
    let prepared = draft::prepare(&view.template, &frame, &req, &view.patient, dict)?;
    Ok((prepared, facts, input))
}

#[derive(Serialize)]
struct DraftPreview {
    #[serde(flatten)]
    preview: ExtractPreview,
    /// Sections no fact is tagged for: nothing is sent, the draft comes back empty.
    skipped: Vec<String>,
    /// Facts from before tagging, offered to every section.
    untagged: usize,
}

/// The redaction preview for a draft: one outgoing message per section.
#[tauri::command]
fn draft_preview(state: State<'_, AppState>, report_id: String, choice: DraftChoice) -> CmdResult<DraftPreview> {
    let (prepared, _, _) = state.with(|s| prepare_draft(s, &report_id, &choice))?;
    let report = state.with(|s| s.report(&report_id))?;
    let terms = state.with(|s| s.pseudo_terms(&report.patient_id))?;
    let config = state.with(|s| s.llm_config())?;
    let provider = config.active.map(llm::provider);
    let model = config.active.and_then(|id| config.settings(id).model).filter(|m| !m.is_empty());
    let preview = ExtractPreview { preview: prepared.preview(), terms, provider, model };
    Ok(DraftPreview { preview, skipped: prepared.skipped, untagged: prepared.untagged })
}

#[derive(Serialize)]
struct DraftSources {
    facts: Vec<EpisodeFact>,
    /// When the episode's previous application was created: sessions after it are the default
    /// choice for this Bericht (`None`: the first application, all sessions).
    since: Option<String>,
}

/// The applied Sitzungsnotiz facts a draft of this Bericht could draw on.
#[tauri::command]
fn draft_sources(state: State<'_, AppState>, report_id: String) -> CmdResult<DraftSources> {
    state.with(|s| {
        let report = s.report(&report_id)?;
        let Some(ep) = report.episode_id else { return Ok(DraftSources { facts: vec![], since: None }) };
        let apps = s.list_applications(&ep)?;
        let this = apps.iter().find(|a| Some(&a.id) == report.application_id.as_ref());
        // `seq` counts per kind (Fortführung Nr. n), so order by creation.
        let since = this
            .and_then(|t| apps.iter().filter(|a| a.created_at < t.created_at).max_by_key(|a| a.created_at.clone()))
            .map(|a| a.created_at.clone());
        Ok(DraftSources { facts: s.episode_facts(&ep)?, since })
    })
}

#[tauri::command]
fn report_drafts(state: State<'_, AppState>, report_id: String) -> CmdResult<Vec<SectionDraft>> {
    state.with(|s| s.section_drafts(&report_id))
}

#[derive(Serialize)]
struct DraftResult {
    drafts: Vec<SectionDraft>,
    sentences: usize,
    dropped: usize,
    /// Sections drafted; fewer than `total` when cancelled.
    done: usize,
    total: usize,
    cancelled: bool,
}

/// Sends the previewed requests (one streamed request per section) and stores the checked
/// drafts. `digest` must match the preview the therapist saw; nothing is stored if any request
/// fails. `llm_cancel(request_id)` stops it: sections drafted before are kept.
#[tauri::command]
async fn draft_run(app: AppHandle, report_id: String, choice: DraftChoice, digest: String, request_id: String) -> CmdResult<DraftResult> {
    let events = app.clone();
    blocking(app, move |state| {
        let run = LlmRun::start(state, &request_id)?;
        let (provider, model) = active_llm(state)?;
        let (prepared, facts, input) = state.with(|s| prepare_draft(s, &report_id, &choice))?;
        if prepared.digest != digest {
            return Err(CmdError::Other("Der Text hat sich seit der Vorschau geändert. Bitte die Vorschau erneut prüfen.".into()));
        }
        let total = prepared.sections.len();
        let progress = |stage, done| {
            let _ = events.emit("draft-progress", ExtractProgress { report_id: report_id.clone(), stage, done, total });
        };
        // Sections without a fact for them get an empty draft without a request.
        let skipped = || prepared.skipped.iter().map(|k| draft::SectionResult { key: k.clone(), parsed: draft::Parsed::default(), with_existing: false });
        if provider == ProviderId::Local {
            progress("loading", 0);
        }
        let nothing = || {
            let drafts = state.with(|s| s.section_drafts(&report_id))?;
            Ok(DraftResult { drafts, sentences: 0, dropped: 0, done: 0, total, cancelled: true })
        };
        let (mut drafts, mut sentences, mut dropped, mut answered_by) = (Vec::new(), 0, 0, model.clone());
        drafts.extend(skipped());
        let ep = if total == 0 {
            None
        } else {
            match llm_endpoint(state, provider, &run.cancel) {
                Err(_) if run.cancelled() => return nothing(),
                r => Some(r?.0),
            }
        };
        let opts = draft::chat_options();
        for (i, section) in prepared.sections.iter().enumerate() {
            if run.cancelled() {
                break;
            }
            progress("running", i);
            let messages = [(llm::Role::System, &prepared.system), (llm::Role::User, &section.user)];
            let ep = ep.as_ref().expect("an endpoint whenever there are requests");
            let Some(completion) = stream_call(state, &events, &run, "llm.draft", (provider, &model), ep, &messages, &opts)? else {
                break;
            };
            let parsed = draft::parse(&completion.text, section, &input, &prepared.map)?;
            sentences += parsed.sentences.len();
            dropped += parsed.dropped;
            drafts.push(draft::SectionResult { key: section.key.clone(), parsed, with_existing: section.with_existing });
            answered_by = completion.model;
        }
        let done = drafts.len() - prepared.skipped.len();
        if drafts.is_empty() || (done == 0 && total > 0) {
            return nothing();
        }
        progress("running", done);
        let meta = DraftMeta {
            provider: provider_str(provider),
            model: answered_by,
            prompt_hash: prepared.prompt_hash.clone(),
            instructed: draft::clean_instruction(choice.instruction.as_deref())?.is_some(),
            session_ids: choice.session_ids.clone(),
            source: DraftSource::Facts,
        };
        let drafts = state.with(|s| s.save_drafts(&report_id, &meta, &drafts, &facts))?;
        Ok(DraftResult { drafts, sentences, dropped, done, total, cancelled: done < total })
    })
    .await
}

#[tauri::command]
fn report_draft_reject(state: State<'_, AppState>, report_id: String, draft_id: String) -> CmdResult<Vec<SectionDraft>> {
    state.with(|s| s.reject_draft(&report_id, &draft_id))
}

#[tauri::command]
fn report_draft_apply(state: State<'_, AppState>, report_id: String, draft_id: String, mode: ApplyMode) -> CmdResult<Report> {
    state.with(|s| s.apply_draft(&report_id, &draft_id, mode))
}

// ---------- live dictation + Diktat-Werkstatt (slice 24) ----------

/// whisper-server stays loaded this long after the last dictation.
const WHISPER_IDLE: std::time::Duration = std::time::Duration::from_secs(300);

/// The one live dictation that may run at a time (it shares the microphone with recordings).
struct ActiveDictation {
    id: String,
    report_id: Option<String>,
    target: SttTarget,
    recorder: Recorder,
    splitter: Arc<Mutex<dictate::Splitter>>,
    pieces: std::sync::mpsc::Sender<dictate::Piece>,
    shared: Arc<DictationShared>,
}

#[derive(Default)]
struct DictationShared {
    speaking: AtomicBool,
    /// Pieces cut but not yet transcribed.
    pending: std::sync::atomic::AtomicUsize,
    /// whisper-server is loading the speech model.
    loading: AtomicBool,
    cancel: AtomicBool,
    error: Mutex<Option<String>>,
}

impl DictationShared {
    fn fail(&self, msg: String) {
        if let Ok(mut e) = self.error.lock() {
            e.get_or_insert(msg);
        }
    }
}

/// Which engines a live dictation can use right now (the therapist's voice only: the cloud is
/// allowed whenever it's set up, no patient consent is involved).
#[derive(Serialize)]
struct DictationOverview {
    default: SttTarget,
    local_problem: Option<String>,
    cloud_problem: Option<String>,
    cloud_label: String,
}

fn whisper_server_exe(state: &AppState) -> PathBuf {
    state.whisper_dir().join("whisper-server.exe")
}

fn dictation_overview_for(state: &AppState) -> CmdResult<DictationOverview> {
    let config = state.with(|s| s.stt_config())?;
    let local_problem = match local_stt(state, "de") {
        Err(e) => Some(e.to_string()),
        Ok(_) if !whisper_server_exe(state).exists() => Some("Die Diktat-Erkennung fehlt in dieser Installation".to_string()),
        Ok(_) => None,
    };
    let model = config.model();
    let cloud_problem = if model.is_none() {
        Some("Kein Cloud-Sprachmodell eingetragen (Einstellungen → Transkription)".to_string())
    } else {
        llm_endpoint(state, config.cloud_provider, &AtomicBool::new(false)).err().map(|e| e.to_string())
    };
    Ok(DictationOverview {
        default: stt::default_target(&config, SourceKind::Dictation, local_problem.is_none(), false),
        local_problem,
        cloud_problem,
        cloud_label: format!("{} · {}", llm::provider(config.cloud_provider).label, model.unwrap_or_default()),
    })
}

#[tauri::command]
fn dictation_overview(state: State<'_, AppState>) -> CmdResult<DictationOverview> {
    dictation_overview_for(&state)
}

#[derive(Clone, Serialize)]
struct DictationStarted {
    id: String,
    engine: SttTarget,
}

/// Starts a live dictation into report `report_id` (the editor's voice mode or its Werkstatt), or
/// without one into the assistant's question box.
/// Pieces come back as `dictation:piece` events; `dictation:done` follows the last one.
#[tauri::command]
fn dictation_start(
    app: AppHandle,
    state: State<'_, AppState>,
    report_id: Option<String>,
    engine: Option<SttTarget>,
    device_id: Option<String>,
) -> CmdResult<DictationStarted> {
    if lock(&state.recording)?.is_some() {
        return Err(CmdError::Other("Während einer Aufnahme ist kein Diktat möglich".into()));
    }
    let mut slot = lock(&state.dictation)?;
    if slot.is_some() {
        return Err(CmdError::Other("Es läuft bereits ein Diktat".into()));
    }
    let report = report_id.as_deref().map(|id| state.with(|s| s.report(id))).transpose()?;
    if report.is_some_and(|r| matches!(r.status, ReportStatus::Signed | ReportStatus::Exported)) {
        return Err(CmdError::Other("Unterschrieben: hier kann nichts mehr diktiert werden".into()));
    }
    let overview = dictation_overview_for(&state)?;
    let config = state.with(|s| s.stt_config())?;
    let target = stt::choose(&config, SourceKind::Dictation, overview.local_problem.is_none(), false, engine)?;
    let cloud = match target {
        SttTarget::Local => {
            if let Some(p) = overview.local_problem {
                return Err(CmdError::Other(p));
            }
            None
        }
        SttTarget::Cloud => {
            if let Some(p) = overview.cloud_problem {
                return Err(CmdError::Other(p));
            }
            let (endpoint, _) = llm_endpoint(&state, config.cloud_provider, &AtomicBool::new(false))?;
            let model = config.model().unwrap_or_default();
            Some((config.cloud_provider, CloudStt { endpoint, model, language: "de".into() }))
        }
    };

    let id = new_id();
    let shared = Arc::new(DictationShared::default());
    let splitter = Arc::new(Mutex::new(dictate::Splitter::new()));
    let (tx, rx) = std::sync::mpsc::channel::<dictate::Piece>();
    let (sink_split, sink_tx, sink_shared) = (splitter.clone(), tx.clone(), shared.clone());
    let sink = Box::new(move |pcm: &[i16]| {
        let mut split = sink_split.lock().map_err(|_| "poisoned".to_string())?;
        for piece in split.push(pcm) {
            sink_shared.pending.fetch_add(1, Ordering::SeqCst);
            let _ = sink_tx.send(piece);
        }
        sink_shared.speaking.store(split.speaking(), Ordering::Relaxed);
        Ok(())
    });
    // 100 ms buffers: the splitter decides on every level window.
    let recorder = Recorder::start(device_id.as_deref(), dictate::WINDOW_MS as usize * audio::SAMPLE_RATE as usize / 1000, sink)
        .map_err(|e| CmdError::Other(e.to_string()))?;
    let worker = (app.clone(), id.clone(), report_id.clone(), shared.clone());
    std::thread::spawn(move || {
        let (app, id, report_id, shared) = worker;
        dictation_worker(&app, &id, report_id.as_deref(), cloud, rx, &shared);
    });
    *slot = Some(ActiveDictation { id: id.clone(), report_id, target, recorder, splitter, pieces: tx, shared });
    Ok(DictationStarted { id, engine: target })
}

#[derive(Clone, Serialize)]
struct DictationPiece<'a> {
    dictation_id: &'a str,
    report_id: Option<&'a str>,
    seq: u32,
    /// `None`: nothing was said (noise, a cough).
    text: Option<&'a str>,
}

#[derive(Clone, Serialize)]
struct DictationDone<'a> {
    dictation_id: &'a str,
    report_id: Option<&'a str>,
    error: Option<String>,
}

/// Transcribes the pieces of one dictation in order, until the recorder is stopped and the last
/// piece is done. Logs the dictation (no text) at the end.
fn dictation_worker(
    app: &AppHandle,
    id: &str,
    report_id: Option<&str>,
    cloud: Option<(ProviderId, CloudStt)>,
    rx: std::sync::mpsc::Receiver<dictate::Piece>,
    shared: &DictationShared,
) {
    let state = app.state::<AppState>();
    let mut log = DictationLog { provider: cloud.as_ref().map(|(p, _)| provider_str(*p)), ..Default::default() };
    // The local model loads while she starts speaking; pieces queue meanwhile.
    if cloud.is_none() {
        shared.loading.store(true, Ordering::Relaxed);
        if let Err(e) = ensure_whisper_server(&state, &shared.cancel) {
            shared.fail(e.to_string());
        }
        shared.loading.store(false, Ordering::Relaxed);
    }
    let mut previous: Option<String> = None;
    for piece in rx {
        let failed = lock(&shared.error).map(|e| e.is_some()).unwrap_or(true);
        if !failed && !shared.cancel.load(Ordering::Relaxed) {
            let result = match &cloud {
                Some((_, c)) => {
                    log.model = c.model.clone();
                    dictate::transcribe_piece(&dictate::LiveEngine::Cloud(c), &piece, None, &shared.cancel)
                }
                None => match lock(&state.whisper_server) {
                    Ok(guard) => match guard.as_ref() {
                        Some(server) => {
                            log.model = server.model_id.clone();
                            dictate::transcribe_piece(&dictate::LiveEngine::Local(server), &piece, previous.as_deref(), &shared.cancel)
                        }
                        None => Err(thera_core::Error::Stt("Diktat-Erkennung läuft nicht".into())),
                    },
                    Err(_) => Err(thera_core::Error::Stt("state poisoned".into())),
                },
            };
            match result {
                Ok((text, usage)) => {
                    log.pieces += 1;
                    log.seconds += piece.duration_ms() as f64 / 1000.0;
                    if let Some(u) = usage {
                        log.requests += u.requests;
                        if let Some(c) = u.cost {
                            *log.cost.get_or_insert(0.0) += c;
                        }
                    } else {
                        log.requests += 1;
                    }
                    let _ = app.emit("dictation:piece", DictationPiece { dictation_id: id, report_id, seq: piece.seq, text: text.as_deref() });
                    previous = text.map(|t| t.chars().rev().take(200).collect::<Vec<_>>().into_iter().rev().collect());
                }
                Err(thera_core::Error::Cancelled) => {}
                Err(e) => shared.fail(e.to_string()),
            }
        }
        if lock(&shared.error).map(|e| e.is_some()).unwrap_or(false) {
            // Unsent pieces are dropped; the UI clears their placeholder.
            let _ = app.emit("dictation:piece", DictationPiece { dictation_id: id, report_id, seq: piece.seq, text: None });
        }
        shared.pending.fetch_sub(1, Ordering::SeqCst);
        drop(piece);
    }
    let error = lock(&shared.error).ok().and_then(|e| e.clone());
    log.error = error.as_ref().map(|_| "error".to_string());
    if log.pieces > 0 || log.error.is_some() {
        let _ = state.with(|s| s.log_dictation(report_id, &log));
    }
    let _ = app.emit("dictation:done", DictationDone { dictation_id: id, report_id, error });
    schedule_whisper_idle(app.clone());
}

/// Starts whisper-server with the chosen speech model, unless it already runs with it.
fn ensure_whisper_server(state: &AppState, cancel: &AtomicBool) -> CmdResult<()> {
    let mut slot = lock(&state.whisper_server)?;
    let model_id = active_model(state)?;
    if let Some(server) = slot.as_mut() {
        if server.model_id == model_id && server.alive() {
            return Ok(());
        }
    }
    *slot = None;
    let model = stt::model(&model_id)?;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(1).clamp(1, 8));
    let mut server = dictate::WhisperServer::spawn(&whisper_server_exe(state), &state.models_dir().join(model.file), &model_id, "de", threads)?;
    if let Some(job) = &state.job {
        job.assign(server.child());
    }
    server.wait_ready(std::time::Duration::from_secs(120), || cancel.load(Ordering::Relaxed) || state.exiting.load(Ordering::Relaxed))?;
    *slot = Some(server);
    Ok(())
}

/// Stops whisper-server once no dictation used it for [`WHISPER_IDLE`] (frees ~1 GB of RAM).
fn schedule_whisper_idle(app: AppHandle) {
    let state = app.state::<AppState>();
    if let Ok(mut at) = state.whisper_last_used.lock() {
        *at = Some(std::time::Instant::now());
    }
    std::thread::spawn(move || {
        std::thread::sleep(WHISPER_IDLE);
        let state = app.state::<AppState>();
        let idle = state.whisper_last_used.lock().ok().and_then(|at| *at).is_some_and(|at| at.elapsed() >= WHISPER_IDLE);
        let dictating = state.dictation.lock().map(|d| d.is_some()).unwrap_or(true);
        if idle && !dictating {
            if let Ok(mut slot) = state.whisper_server.lock() {
                *slot = None;
            }
        }
    });
}

#[derive(Serialize)]
struct DictationStatus {
    id: String,
    report_id: Option<String>,
    engine: SttTarget,
    duration_ms: u64,
    level: f32,
    speaking: bool,
    pending: usize,
    loading: bool,
    error: Option<String>,
}

/// Polled by the UI (~10 Hz) for the level meter and the "listening / recognizing" state.
#[tauri::command]
fn dictation_status(state: State<'_, AppState>) -> CmdResult<Option<DictationStatus>> {
    let slot = lock(&state.dictation)?;
    Ok(slot.as_ref().map(|d| DictationStatus {
        id: d.id.clone(),
        report_id: d.report_id.clone(),
        engine: d.target,
        duration_ms: d.recorder.duration_ms(),
        level: d.recorder.level(),
        speaking: d.shared.speaking.load(Ordering::Relaxed),
        pending: d.shared.pending.load(Ordering::SeqCst),
        loading: d.shared.loading.load(Ordering::Relaxed),
        error: d.recorder.error().or_else(|| d.shared.error.lock().ok().and_then(|e| e.clone())),
    }))
}

/// Stops the microphone. The utterance in progress is still transcribed; `dictation:done`
/// follows the last piece. `discard`: drop what isn't transcribed yet.
#[tauri::command]
fn dictation_stop(state: State<'_, AppState>, discard: Option<bool>) -> CmdResult<()> {
    let Some(d) = lock(&state.dictation)?.take() else { return Ok(()) };
    if discard == Some(true) {
        d.shared.cancel.store(true, Ordering::Relaxed);
    }
    if let Err(e) = d.recorder.stop() {
        d.shared.fail(e.to_string());
    }
    d.shared.speaking.store(false, Ordering::Relaxed);
    if let Some(piece) = lock(&d.splitter)?.finish() {
        d.shared.pending.fetch_add(1, Ordering::SeqCst);
        let _ = d.pieces.send(piece);
    }
    // Dropping the last sender ends the worker once the queue is done.
    Ok(())
}

#[tauri::command]
fn workshop_get(state: State<'_, AppState>, report_id: String) -> CmdResult<String> {
    state.with(|s| s.workshop_text(&report_id))
}

#[tauri::command]
fn workshop_set(state: State<'_, AppState>, report_id: String, body: String) -> CmdResult<()> {
    state.with(|s| s.set_workshop_text(&report_id, &body))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkshopChoice {
    instruction: Option<String>,
    /// Send the sections' current text along, so the proposal only adds to it.
    #[serde(default)]
    with_existing: bool,
}

/// Exactly what "Entwurf erstellen" in the Werkstatt would send.
fn prepare_workshop(s: &Store, report_id: &str, choice: &WorkshopChoice) -> thera_core::Result<(workshop::Prepared, workshop::Audience)> {
    let view = report_view(s, report_id)?;
    let audience = if view.report.kind == "sitzungsnotiz" {
        workshop::Audience::Note
    } else {
        let (Some(episode), Some(application)) = (&view.episode, &view.application) else {
            return Err(thera_core::Error::Invalid("Entwürfe gibt es nur für Berichte zu einem Antrag".into()));
        };
        workshop::Audience::Bericht(draft::Frame { method: episode.method, age_group: episode.age_group, kind: application.kind })
    };
    let text = draft::plain_text(&s.workshop_text(report_id)?);
    let existing: Vec<(String, String)> = if choice.with_existing {
        view.report.sections.iter().map(|sec| (sec.key.clone(), draft::plain_text(&sec.body))).collect()
    } else {
        vec![]
    };
    let dict = s.pseudo_dictionary(&view.patient.id)?;
    let req = workshop::Request { text: &text, existing: &existing, instruction: choice.instruction.as_deref() };
    Ok((workshop::prepare(&view.template, &audience, &req, &view.patient, dict)?, audience))
}

#[tauri::command]
fn workshop_preview(state: State<'_, AppState>, report_id: String, choice: WorkshopChoice) -> CmdResult<ExtractPreview> {
    let (prepared, _) = state.with(|s| prepare_workshop(s, &report_id, &choice))?;
    let report = state.with(|s| s.report(&report_id))?;
    let terms = state.with(|s| s.pseudo_terms(&report.patient_id))?;
    let config = state.with(|s| s.llm_config())?;
    let provider = config.active.map(llm::provider);
    let model = config.active.and_then(|id| config.settings(id).model).filter(|m| !m.is_empty());
    Ok(ExtractPreview { preview: prepared.preview(), terms, provider, model })
}

/// Sends the previewed Werkstatt request (one streamed request for all sections) and stores a
/// proposal per section that got text. `digest` must match the preview the therapist saw.
#[tauri::command]
async fn workshop_run(app: AppHandle, report_id: String, choice: WorkshopChoice, digest: String, request_id: String) -> CmdResult<DraftResult> {
    let events = app.clone();
    blocking(app, move |state| {
        let run = LlmRun::start(state, &request_id)?;
        let (provider, model) = active_llm(state)?;
        let (prepared, audience) = state.with(|s| prepare_workshop(s, &report_id, &choice))?;
        if prepared.digest != digest {
            return Err(CmdError::Other("Der Text hat sich seit der Vorschau geändert. Bitte die Vorschau erneut prüfen.".into()));
        }
        let progress = |stage, done| {
            let _ = events.emit("draft-progress", ExtractProgress { report_id: report_id.clone(), stage, done, total: 1 });
        };
        let nothing = || {
            let drafts = state.with(|s| s.section_drafts(&report_id))?;
            Ok(DraftResult { drafts, sentences: 0, dropped: 0, done: 0, total: 1, cancelled: true })
        };
        if provider == ProviderId::Local {
            progress("loading", 0);
        }
        let ep = match llm_endpoint(state, provider, &run.cancel) {
            Err(_) if run.cancelled() => return nothing(),
            r => r?.0,
        };
        progress("running", 0);
        let messages = [(llm::Role::System, &prepared.system), (llm::Role::User, &prepared.user)];
        let Some(completion) = stream_call(state, &events, &run, "llm.workshop", (provider, &model), &ep, &messages, &workshop::chat_options())? else {
            return nothing();
        };
        let results = workshop::parse(&completion.text, &prepared, &audience)?;
        progress("running", 1);
        let sentences = results.iter().map(|r| r.parsed.sentences.len()).sum();
        let dropped = results.iter().map(|r| r.parsed.dropped).sum();
        if results.is_empty() {
            let drafts = state.with(|s| s.section_drafts(&report_id))?;
            return Ok(DraftResult { drafts, sentences: 0, dropped, done: 1, total: 1, cancelled: false });
        }
        let meta = DraftMeta {
            provider: provider_str(provider),
            model: completion.model,
            prompt_hash: prepared.prompt_hash.clone(),
            instructed: draft::clean_instruction(choice.instruction.as_deref())?.is_some(),
            session_ids: None,
            source: DraftSource::Dictation,
        };
        let drafts = state.with(|s| s.save_drafts(&report_id, &meta, &results, &[]))?;
        Ok(DraftResult { drafts, sentences, dropped, done: 1, total: 1, cancelled: false })
    })
    .await
}

// ---------- voice prep (slice 26) ----------

#[derive(Serialize)]
struct LiveOverview {
    config: LiveConfig,
    voices: &'static [live::LiveVoice],
    usd_per_minute: f64,
    /// Why a prep can't start (no OpenAI key), or `None`.
    problem: Option<String>,
    eu: bool,
    /// Said when the voice model asks for backend work (client delegation).
    no_backend: &'static str,
}

fn live_endpoint(state: &AppState) -> CmdResult<Endpoint> {
    llm_endpoint(state, ProviderId::Openai, &AtomicBool::new(false))
        .map(|(ep, _)| ep)
        .map_err(|e| CmdError::Other(format!("OpenAI: {e} (Einstellungen → Stimme)")))
}

#[tauri::command]
fn live_overview(state: State<'_, AppState>) -> CmdResult<LiveOverview> {
    let config = state.with(|s| s.live_config())?;
    let eu = state.with(|s| s.llm_config())?.base_url(ProviderId::Openai) == llm::OPENAI_EU_BASE_URL;
    Ok(LiveOverview {
        config,
        voices: live::VOICES,
        usd_per_minute: live::USD_PER_MINUTE,
        problem: live_endpoint(&state).err().map(|e| e.to_string()),
        eu,
        no_backend: live::NO_BACKEND,
    })
}

#[tauri::command]
fn live_config_set(state: State<'_, AppState>, config: LiveConfig) -> CmdResult<()> {
    state.with(|s| s.set_live_config(&config))
}

/// The brief for a session's prep, from the patient's earlier signed notes, key points and applied facts.
fn prep_brief(s: &Store, session_id: &str) -> thera_core::Result<context::Brief> {
    let session = s.session(session_id)?;
    let patient = s.patient(&session.patient_id)?;
    let notes = s.prior_session_notes(session_id, context::MAX_NOTES)?;
    let summaries = s.recent_summaries(&patient.id, Some(session_id), context::MAX_NOTES)?;
    let prior = context::prior_sessions(notes, summaries, context::MAX_NOTES);
    let facts: Vec<EpisodeFact> = match &session.episode_id {
        Some(e) => s.episode_facts(e)?.into_iter().filter(|f| f.session_id != session_id).collect(),
        None => Vec::new(),
    };
    context::prep_brief(&patient, s.pseudo_dictionary(&patient.id)?, &prior, &facts)
}

#[derive(Serialize)]
struct PrepPreview {
    #[serde(flatten)]
    preview: extract::Preview,
    stats: context::BriefStats,
    terms: Vec<PseudoTerm>,
}

/// Exactly what a voice prep would send as context (besides her voice).
#[tauri::command]
fn prep_preview(state: State<'_, AppState>, session_id: String) -> CmdResult<PrepPreview> {
    state.with(|s| {
        let brief = prep_brief(s, &session_id)?;
        let patient_id = s.session(&session_id)?.patient_id;
        Ok(PrepPreview { preview: brief.preview(), stats: brief.stats.clone(), terms: s.pseudo_terms(&patient_id)? })
    })
}

/// Opens the GPT-Live session: posts the WebView's SDP offer with the brief and returns the
/// answer. Refused while a recording runs, when not opted in, or if the brief changed.
#[tauri::command]
async fn prep_start(app: AppHandle, session_id: String, digest: String, sdp: String) -> CmdResult<live::LiveSession> {
    blocking(app, move |state| {
        if lock(&state.recording)?.is_some() {
            return Err(CmdError::Other("Während einer Aufnahme ist keine Sprachvorbereitung möglich".into()));
        }
        let config = state.with(|s| s.live_config())?;
        if !config.enabled {
            return Err(CmdError::Other("Die Sprachvorbereitung ist ausgeschaltet (Einstellungen → Stimme)".into()));
        }
        let brief = state.with(|s| prep_brief(s, &session_id))?;
        if brief.digest != digest {
            return Err(CmdError::Other("Der Text hat sich seit der Vorschau geändert. Bitte die Vorschau erneut prüfen.".into()));
        }
        let ep = live_endpoint(state)?;
        let result = live::create_session(&ep, config.voice(), &brief.text, &sdp);
        let stats = (brief.stats.notes, brief.stats.facts);
        state.with(|s| s.log_prep_start(&session_id, result.as_ref().map(|_| stats).map_err(|_| "error")))?;
        Ok(result?)
    })
    .await
}

/// Stores the finished conversation (captions, restored in the WebView) and the billed seconds.
#[tauri::command]
fn prep_finish(state: State<'_, AppState>, session_id: String, turns: Vec<PrepTurn>, seconds: Option<f64>, reason: String) -> CmdResult<SessionPrep> {
    state.with(|s| {
        s.save_prep_conversation(&session_id, &turns, seconds, live::MODEL, &reason)?;
        s.session_prep(&session_id)?.ok_or_else(|| thera_core::Error::NotFound(session_id.clone()))
    })
}

/// A conversation in which nothing was said: only the billed seconds are audited.
#[tauri::command]
fn prep_abort(state: State<'_, AppState>, session_id: String, seconds: Option<f64>, reason: String) -> CmdResult<()> {
    state.with(|s| s.log_prep_empty(&session_id, seconds, live::MODEL, &reason))
}

#[tauri::command]
fn prep_get(state: State<'_, AppState>, session_id: String) -> CmdResult<Option<SessionPrep>> {
    state.with(|s| s.session_prep(&session_id))
}

#[tauri::command]
fn prep_sheet_set(state: State<'_, AppState>, session_id: String, sheet: String) -> CmdResult<()> {
    state.with(|s| s.set_prep_sheet(&session_id, &sheet, None))
}

/// Turns the stored conversation into a prep sheet with the drafting model (streamed).
/// `Ok(None)`: cancelled.
#[tauri::command]
async fn prep_sheet_run(app: AppHandle, session_id: String, request_id: String) -> CmdResult<Option<SessionPrep>> {
    let events = app.clone();
    blocking(app, move |state| {
        let run = LlmRun::start(state, &request_id)?;
        let (provider, model) = active_llm(state)?;
        let prepared = state.with(|s| {
            let prep = s.session_prep(&session_id)?.ok_or_else(|| thera_core::Error::NotFound("Kein Vorbereitungsgespräch".into()))?;
            let patient = s.patient(&s.session(&session_id)?.patient_id)?;
            live::prepare_sheet(&prep.transcript, &patient, s.pseudo_dictionary(&patient.id)?)
        })?;
        let ep = match llm_endpoint(state, provider, &run.cancel) {
            Err(_) if run.cancelled() => return Ok(None),
            r => r?.0,
        };
        let messages = [(llm::Role::System, &prepared.system), (llm::Role::User, &prepared.user)];
        let Some(completion) = stream_call(state, &events, &run, "llm.prep_sheet", (provider, &model), &ep, &messages, &live::sheet_options())? else {
            return Ok(None);
        };
        let html = live::sheet_html(&prepared.map.restore(&completion.text).text);
        state.with(|s| {
            s.set_prep_sheet(&session_id, &html, Some(&completion.model))?;
            s.session_prep(&session_id)
        })
    })
    .await
}

// ---------- assistant (slice 25) ----------

#[derive(Serialize)]
struct ChatView {
    thread: ChatThread,
    messages: Vec<ChatMessage>,
    /// "Was ich getan habe": a German line per tool call id.
    steps: HashMap<String, String>,
    proposals: Vec<AgentProposal>,
}

fn chat_view(s: &Store, id: &str) -> thera_core::Result<ChatView> {
    let thread = s.chat_thread(id)?;
    let messages = s.chat_messages(id)?;
    let steps = messages.iter().flat_map(|m| &m.tool_calls).map(|c| (c.id.clone(), agent::step_label(&c.name, &c.arguments))).collect();
    Ok(ChatView { thread, messages, steps, proposals: s.proposals(id)? })
}

/// Newest first; with `scope`, only that kind (and `ref_id`).
#[tauri::command]
fn chat_threads(state: State<'_, AppState>, scope: Option<ChatScope>, ref_id: Option<String>) -> CmdResult<Vec<ChatThread>> {
    state.with(|s| s.chat_threads(scope, ref_id.as_deref(), 100))
}

#[tauri::command]
fn chat_thread_create(state: State<'_, AppState>, scope: ChatScope, ref_id: Option<String>) -> CmdResult<ChatThread> {
    state.with(|s| s.create_chat_thread(scope, ref_id.as_deref()))
}

#[tauri::command]
fn chat_get(state: State<'_, AppState>, id: String) -> CmdResult<ChatView> {
    state.with(|s| chat_view(s, &id))
}

#[tauri::command]
fn chat_thread_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.with(|s| s.delete_chat_thread(&id))
}

#[derive(Clone, Serialize)]
struct AgentText<'a> {
    request_id: &'a str,
    thread_id: &'a str,
    /// The answer so far, restored (real names).
    text: &'a str,
}

#[derive(Clone, Serialize)]
struct AgentChanged<'a> {
    thread_id: &'a str,
}

/// The app side of the loop: store access per step, the streamed request with its audit entry.
struct AppHost<'a> {
    state: &'a AppState,
    events: &'a AppHandle,
    run: &'a LlmRun<'a>,
    thread_id: &'a str,
    provider: ProviderId,
    model: String,
    ep: Endpoint,
}

impl Host for AppHost<'_> {
    fn store<T>(&mut self, f: impl FnOnce(&Store) -> thera_core::Result<T>) -> thera_core::Result<T> {
        self.state.with(f).map_err(|e| match e {
            CmdError::Core(c) => c,
            CmdError::Other(m) => thera_core::Error::Invalid(m),
        })
    }

    fn complete(&mut self, messages: &[llm::Message], tools: &serde_json::Value, map: &thera_core::pseudo::TokenMap) -> thera_core::Result<Option<llm::Completion>> {
        if self.run.cancelled() {
            return Ok(None);
        }
        let mut text = String::new();
        let result = llm::chat_stream_tools(&self.ep, &self.model, messages, tools, &agent::chat_options(), &self.run.cancel, |delta| {
            text.push_str(delta);
            let shown = agent::restore_partial(map, &text);
            let _ = self.events.emit("agent:text", AgentText { request_id: &self.run.id, thread_id: self.thread_id, text: &shown });
        });
        let reason = if matches!(result, Err(thera_core::Error::Cancelled)) { "cancelled" } else { "error" };
        let (provider, model) = (self.provider, self.model.clone());
        self.store(|s| s.log_llm_call("llm.agent", provider, &model, result.as_ref().map_err(|_| reason)))?;
        match result {
            Err(thera_core::Error::Cancelled) => Ok(None),
            r => r.map(Some),
        }
    }

    fn changed(&mut self) {
        let _ = self.events.emit("agent:changed", AgentChanged { thread_id: self.thread_id });
    }
}

/// One question to the assistant. Streams the answer (`agent:text`), announces each stored
/// message (`agent:changed`) and returns when it answered, was cancelled (`llm_cancel`) or gave up.
#[tauri::command]
async fn chat_send(app: AppHandle, thread_id: String, text: String, zone: agent::Zone, request_id: String) -> CmdResult<&'static str> {
    let events = app.clone();
    blocking(app, move |state| {
        let run = LlmRun::start(state, &request_id)?;
        let (provider, model) = active_llm(state)?;
        let clock = agent::Clock::current(&zone)?;
        let ep = match llm_endpoint(state, provider, &run.cancel) {
            Err(_) if run.cancelled() => return Ok("cancelled"),
            r => r?.0,
        };
        let mut host = AppHost { state, events: &events, run: &run, thread_id: &thread_id, provider, model, ep };
        let end = agent::run_turn(&mut host, &thread_id, &text, &clock)?;
        Ok(match end {
            agent::TurnEnd::Answered => "answered",
            agent::TurnEnd::Cancelled => "cancelled",
            agent::TurnEnd::StepLimit => "step_limit",
        })
    })
    .await
}

/// "Übernehmen": checks again and runs the normal store command (audited `agent.apply`).
#[tauri::command]
fn proposal_apply(state: State<'_, AppState>, id: String) -> CmdResult<AgentProposal> {
    state.with(|s| s.apply_proposal(&id))
}

#[tauri::command]
fn proposal_discard(state: State<'_, AppState>, id: String) -> CmdResult<AgentProposal> {
    state.with(|s| s.discard_proposal(&id))
}

/// Whether the drafting model takes tools, from the provider's model list (`None`: unknown).
#[tauri::command]
async fn agent_tools_support(app: AppHandle) -> CmdResult<Option<bool>> {
    blocking(app, move |state| {
        let (provider, model) = active_llm(state)?;
        if provider == ProviderId::Local {
            // llama-server applies the model's chat template with tool support (--jinja).
            return Ok(Some(true));
        }
        let (ep, _) = llm_endpoint(state, provider, &AtomicBool::new(false))?;
        Ok(llm::list_models(&ep).ok().and_then(|ms| ms.into_iter().find(|m| m.id == model)).and_then(|m| m.tools))
    })
    .await
}

/// Reads an answer aloud (MP3). Pseudonymized with the thread's map; placeholders become a
/// neutral word.
#[tauri::command]
async fn chat_speak(app: AppHandle, thread_id: String, message_id: String) -> CmdResult<tauri::ipc::Response> {
    blocking(app, move |state| {
        let config = state.with(|s| s.tts_config())?;
        if !config.enabled {
            return Err(CmdError::Other("Vorlesen ist ausgeschaltet (Einstellungen → Stimme)".into()));
        }
        let model = config.model().ok_or_else(|| CmdError::Other("Kein Sprachmodell eingetragen".into()))?;
        let text = state.with(|s| {
            let m = s.chat_messages(&thread_id)?.into_iter().find(|m| m.id == message_id).ok_or_else(|| thera_core::Error::NotFound(message_id.clone()))?;
            let mut p = thera_core::pseudo::Pseudonymizer::with_map(s.assistant_dictionary()?, s.chat_thread_map(&thread_id)?);
            Ok(p.pseudonymize(&thera_core::pseudo::defuse_tokens(&m.content))?.speakable())
        })?;
        let (ep, _) = llm_endpoint(state, config.provider, &AtomicBool::new(false))?;
        let result = tts::speak(&ep, &model, &config.voice(), &text);
        state.with(|s| s.log_tts(config.provider, &model, result.as_ref().map_err(|_| "error")))?;
        Ok(tauri::ipc::Response::new(result?.audio))
    })
    .await
}

// ---------- local models (Phase 2) ----------

/// Loading 15 GB from a slow disk can take minutes.
const LOCAL_LOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// Endpoint of the local server for `model_id`, starting it and loading the model if needed.
fn local_endpoint(state: &AppState, model_id: &str, cancel: &AtomicBool) -> CmdResult<Endpoint> {
    let m = local::model(model_id)?;
    let _starting = lock(&state.llama_start)?;
    {
        let mut slot = lock(&state.llama)?;
        if let Some(server) = slot.as_mut().filter(|s| s.model_id == m.file.id) {
            if server.alive() {
                return Ok(server.endpoint());
            }
        }
        *slot = None;
    }
    let exe = state.llama_exe();
    if !exe.exists() {
        return Err(CmdError::Other("Der lokale KI-Server fehlt in dieser Installation".into()));
    }
    if !model_installed(state, &m.file) {
        return Err(CmdError::Other("Das Modell ist nicht installiert (Einstellungen → KI-Entwürfe)".into()));
    }
    *lock(&state.llama_loading)? = Some(m.file.id.to_string());
    let started = (|| {
        let mut server = LlamaServer::spawn(&exe, &state.models_dir().join(m.file.file), m)?;
        if let Some(job) = &state.job {
            job.assign(server.child());
        }
        server.wait_ready_until(LOCAL_LOAD_TIMEOUT, || {
            state.exiting.load(Ordering::Relaxed) || cancel.load(Ordering::Relaxed)
        })?;
        Ok::<_, thera_core::Error>(server)
    })();
    *lock(&state.llama_loading)? = None;
    let server = started?;
    let ep = server.endpoint();
    *lock(&state.llama)? = Some(server);
    Ok(ep)
}

fn hardware() -> &'static Hardware {
    static HW: OnceLock<Hardware> = OnceLock::new();
    HW.get_or_init(|| {
        let info = system::system_info();
        Hardware {
            ram_bytes: info.ram_bytes,
            cpu_threads: info.cpu_threads,
            gpus: info.gpus.into_iter().map(|(name, vram_bytes)| Gpu { name, vram_bytes }).collect(),
        }
    })
}

#[derive(Serialize)]
struct LocalModelView {
    id: &'static str,
    label: &'static str,
    bytes: u64,
    fit: Fit,
    experimental: bool,
    installed: bool,
    transfer: Option<Transfer>,
}

#[derive(Serialize)]
struct LocalOverview {
    /// `None`: llama-server is missing from this build.
    engine: Option<String>,
    hardware: Hardware,
    recommendation: Recommendation,
    models: Vec<LocalModelView>,
    /// The model the server holds (it unloads it after a few idle minutes on its own).
    loaded: Option<String>,
    loading: Option<String>,
}

#[tauri::command]
fn local_overview(state: State<'_, AppState>) -> CmdResult<LocalOverview> {
    let hw = hardware();
    let transfers = lock(&state.model_transfers)?.clone();
    let models = local::MODELS
        .iter()
        .map(|m| LocalModelView {
            id: m.file.id,
            label: m.label,
            bytes: m.file.bytes,
            fit: local::fit(hw, m),
            experimental: m.experimental,
            installed: model_installed(&state, &m.file),
            transfer: transfers.get(m.file.id).cloned(),
        })
        .collect();
    let loaded = lock(&state.llama)?.as_mut().and_then(|s| s.alive().then(|| s.model_id.to_string()));
    Ok(LocalOverview {
        engine: state.llama_engine(),
        hardware: hw.clone(),
        recommendation: local::recommend(hw),
        models,
        loaded,
        loading: lock(&state.llama_loading)?.clone(),
    })
}

#[tauri::command]
fn local_model_download(app: AppHandle, id: String) -> CmdResult<()> {
    download_model(&app, local::model(&id)?.file)
}

#[tauri::command]
fn local_model_import(app: AppHandle, id: String, path: String) -> CmdResult<()> {
    import_model(&app, local::model(&id)?.file, path)
}

#[tauri::command]
fn local_model_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let m = local::model(&id)?;
    {
        let mut slot = lock(&state.llama)?;
        if slot.as_ref().is_some_and(|s| s.model_id == m.file.id) {
            *slot = None;
        }
    }
    delete_model(&state, &m.file)
}

/// Stops the server and frees the model's memory; the next request loads it again.
#[tauri::command]
fn local_unload(state: State<'_, AppState>) -> CmdResult<()> {
    *lock(&state.llama)? = None;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            #[cfg_attr(not(debug_assertions), allow(unused_mut))]
            let mut data_dir = app.path().app_data_dir()?;
            #[cfg_attr(not(debug_assertions), allow(unused_mut))]
            let mut keyring_user = KEYRING_USER.to_string();
            // Debug-only isolation for first-run E2E: own data dir and keychain entry.
            #[cfg(debug_assertions)]
            {
                if let Some(dir) = std::env::var_os("THERA_DATA_DIR") {
                    data_dir = dir.into();
                }
                if let Ok(user) = std::env::var("THERA_KEYRING_USER") {
                    keyring_user = user;
                }
            }
            std::fs::create_dir_all(&data_dir)?;
            app.manage(AppState {
                data_dir,
                keyring_user,
                resource_dir: app.path().resource_dir()?,
                store: Mutex::new(None),
                recording: Mutex::new(None),
                work: (Mutex::new(false), Condvar::new()),
                transcribing: Mutex::new(None),
                exiting: AtomicBool::new(false),
                model_transfers: Mutex::new(HashMap::new()),
                job: ProcessJob::new().map(Arc::new),
                llama: Mutex::new(None),
                llama_start: Mutex::new(()),
                llama_loading: Mutex::new(None),
                llm_runs: Mutex::new(HashMap::new()),
                dictation: Mutex::new(None),
                whisper_server: Mutex::new(None),
                whisper_last_used: Mutex::new(None),
            });
            let handle = app.handle().clone();
            std::thread::spawn(move || stt_worker(handle));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_init,
            app_recover,
            recovery_kit_export,
            recovery_key_rotate,
            recovery_kit_confirm,
            app_setup,
            backup_create,
            backup_inspect,
            backup_restore,
            patients_list,
            patient_get,
            patient_create,
            patient_update,
            patient_cards,
            patient_photo,
            patient_photo_set,
            patient_photo_remove,
            patient_erase,
            consent_text,
            consent_form_export,
            consents_list,
            consent_grant,
            consent_revoke,
            audio_devices,
            recording_start,
            recording_status,
            recording_stop,
            dictation_overview,
            dictation_start,
            dictation_status,
            dictation_stop,
            workshop_get,
            workshop_set,
            workshop_preview,
            workshop_run,
            session_get,
            source_audio_wav,
            transcription_start,
            transcription_cancel,
            transcript_edit_segment,
            transcript_confirm,
            autopilot_get,
            autopilot_set,
            autopilot_run,
            autopilot_sent,
            autopilot_retry,
            episode_stage_set,
            episode_contingent,
            transcript_set_speaker,
            transcript_swap_speakers,
            transcript_rediarize,
            transcript_assign_therapist,
            diarize_overview,
            diarize_config_set,
            voice_profile_delete,
            stt_overview,
            stt_model_select,
            stt_config_set,
            tts_overview,
            tts_config_set,
            tts_sample,
            live_overview,
            live_config_set,
            prep_preview,
            prep_start,
            prep_finish,
            prep_abort,
            prep_get,
            prep_sheet_set,
            prep_sheet_run,
            chat_threads,
            chat_thread_create,
            chat_get,
            chat_thread_delete,
            chat_send,
            proposal_apply,
            proposal_discard,
            agent_tools_support,
            chat_speak,
            stt_model_delete,
            stt_model_download,
            stt_model_import,
            episodes_list,
            episode_create,
            episode_diagnoses_set,
            applications_list,
            application_create,
            sessions_list,
            session_create,
            session_note_create,
            calendar_range,
            session_queue,
            calendar_export_ics,
            sessions_create_series,
            session_update,
            session_cancel,
            session_reopen,
            session_delete,
            due_items,
            tasks_list,
            task_create,
            task_update,
            task_set_done,
            task_delete,
            day_note_get,
            day_note_set,
            templates_list,
            reports_list,
            report_get,
            report_save_section,
            report_checklist,
            report_sign,
            report_add_addendum,
            report_export,
            report_preview,
            audit_list,
            audit_verify,
            setting_get,
            setting_set,
            llm_overview,
            llm_config_set,
            llm_key_set,
            llm_models,
            llm_test,
            llm_catalog,
            llm_spend,
            extract_preview,
            extract_run,
            llm_cancel,
            pseudo_term_add,
            pseudo_term_remove,
            note_facts,
            note_facts_reject,
            note_facts_apply,
            session_summary,
            summary_preview,
            summary_run,
            summary_edit,
            patient_timeline,
            fact_tag_sections,
            draft_preview,
            draft_sources,
            draft_run,
            report_drafts,
            report_draft_reject,
            report_draft_apply,
            local_overview,
            local_model_download,
            local_model_import,
            local_model_delete,
            local_unload,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Closing the window mid-recording still flushes and closes the source cleanly.
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<AppState>();
                let _ = stop_recording(&state, "stopped");
                // Kill a running sidecar and give the worker a moment to delete its plaintext WAV.
                state.exiting.store(true, Ordering::Relaxed);
                // Stops the local model server; a load in progress sees `exiting` and stops too.
                if let Ok(mut llama) = state.llama.lock() {
                    *llama = None;
                }
                let cancel = state.transcribing.lock().ok().and_then(|g| g.as_ref().map(|r| r.cancel.clone()));
                if let Some(cancel) = cancel {
                    cancel.store(true, Ordering::Relaxed);
                    for _ in 0..30 {
                        if state.transcribing.lock().map_or(true, |g| g.is_none()) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
        });
}
