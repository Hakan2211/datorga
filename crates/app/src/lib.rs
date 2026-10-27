//! Tauri shell: thin command wrappers over `thera_core`. No business logic lives here.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, State};
use thera_capture::{InputDevice, Recorder};
use thera_core::audio::{self, ChunkWriter};
use thera_core::backup::{self, BackupSummary, Unlocked};
use thera_core::consent::{self, ConsentText};
use thera_core::crypto::{self, KeyStore, MasterKey, Unlock};
use thera_core::domain::*;
use thera_core::pii::{self, CheckItem};
use thera_core::render;
use thera_core::store::Store;
use thera_core::stt::{self, WhisperCli};
use thera_core::templates::{Context, Template};

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
    /// Bundled resources; holds the whisper.cpp sidecar in `whisper/`.
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
}

struct RunningJob {
    job_id: String,
    source_id: String,
    cancel: Arc<AtomicBool>,
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
fn patient_erase(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    // A running transcription of this patient is stopped first, which removes its plaintext WAV.
    let running = lock(&state.transcribing)?.as_ref().map(|r| (r.source_id.clone(), r.cancel.clone()));
    if let Some((source_id, cancel)) = running {
        let owner = state.with(|s| Ok(s.session(&s.source(&source_id)?.session_id)?.patient_id))?;
        if owner == id {
            cancel.store(true, Ordering::Relaxed);
        }
    }
    state.with(|s| s.erase_patient(&id))
}

// ---------- consent ----------

#[tauri::command]
fn consent_text(kind: ConsentKind) -> ConsentText {
    consent::current(kind)
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
    state.with(|s| s.revoke_consent(&id))
}

// ---------- capture ----------

#[tauri::command]
fn audio_devices() -> CmdResult<Vec<InputDevice>> {
    thera_capture::input_devices().map_err(|e| CmdError::Other(e.to_string()))
}

#[tauri::command]
fn recording_start(
    state: State<'_, AppState>,
    session_id: String,
    kind: SourceKind,
    device_id: Option<String>,
) -> CmdResult<SessionSource> {
    let mut slot = state.recording.lock().map_err(|_| CmdError::Other("state poisoned".into()))?;
    if slot.is_some() {
        return Err(CmdError::Other("Es läuft bereits eine Aufnahme".into()));
    }
    let patient_id = state.with(|s| s.session(&session_id))?.patient_id;
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
    *slot = Some(ActiveRecording { source_id: source.id.clone(), session_id, patient_id, kind, recorder, writer });
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
    state.with(|s| s.finish_source(&active.source_id, reason, &w))
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
}

#[tauri::command]
fn session_get(state: State<'_, AppState>, id: String) -> CmdResult<SessionView> {
    state.with(|s| {
        let session = s.session(&id)?;
        let sources = s
            .list_sources(&id)?
            .into_iter()
            .map(|src| Ok(SourceView { job: s.latest_job(&src.id)?, transcript: s.transcript_for_source(&src.id)?, source: src }))
            .collect::<thera_core::Result<_>>()?;
        Ok(SessionView { patient: s.patient(&session.patient_id)?, sources, session })
    })
}

// ---------- transcription ----------

#[tauri::command]
fn transcription_start(state: State<'_, AppState>, source_id: String) -> CmdResult<Job> {
    let job = state.with(|s| s.enqueue_transcription(&source_id))?;
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
    state.with(|s| s.confirm_transcript(&id))
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
            run_job(&state, job);
        }
    }
}

fn run_job(state: &AppState, job: Job) {
    let Some(source_id) = job.source_id.clone() else { return };
    let cancel = Arc::new(AtomicBool::new(false));
    if let Ok(mut slot) = state.transcribing.lock() {
        *slot = Some(RunningJob { job_id: job.id.clone(), source_id: source_id.clone(), cancel: cancel.clone() });
    }
    let result = transcribe_job(state, &job, &source_id, &cancel)
        .and_then(|(engine, model, segments)| state.with(|s| s.finish_transcription(&job.id, &engine, &model, &segments)));
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
}

fn transcribe_job(state: &AppState, job: &Job, source_id: &str, cancel: &AtomicBool) -> CmdResult<(String, String, Vec<stt::Segment>)> {
    let engine = state
        .engine()
        .ok_or_else(|| CmdError::Other("Das Transkriptionsprogramm fehlt in dieser Installation".into()))?;
    let model_id = active_model(state)?;
    let model = stt::model(&model_id)?;
    let model_path = state.models_dir().join(model.file);
    if !model_path.exists() {
        return Err(CmdError::Other("Kein Sprachmodell installiert (Einstellungen → Transkription)".into()));
    }
    let ((dir, key), language) = state.with(|s| Ok((s.audio_location(source_id)?, s.source(source_id)?.language)))?;
    // Leave one core for the UI; beyond 8 threads whisper.cpp stops getting faster.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(1).clamp(1, 8));
    let cli = WhisperCli { exe: state.whisper_dir().join("whisper-cli.exe"), model: model_path, language, threads };
    let mut last = None;
    let on_progress = |p: u8| {
        if last != Some(p) {
            last = Some(p);
            let _ = state.with(|s| s.set_job_progress(&job.id, p));
        }
    };
    let segments = stt::transcribe(&dir, &key, &cli, &state.stt_tmp().join(&job.id), on_progress, cancel)?;
    Ok((engine, model_id, segments))
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
            // Size only here; the full hash is checked once, when the file arrives.
            installed: std::fs::metadata(state.models_dir().join(m.file)).is_ok_and(|md| md.len() == m.bytes),
            transfer: transfers.get(m.id).cloned(),
        })
        .collect();
    Ok(SttOverview { engine: state.engine(), active: active_model(&state)?, models })
}

#[tauri::command]
fn stt_model_select(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    stt::model(&id)?;
    state.with(|s| s.set_setting("stt_model", &serde_json::json!(id)))
}

#[tauri::command]
fn stt_model_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let m = stt::model(&id)?;
    if lock(&state.model_transfers)?.get(&id).is_some_and(|t| t.error.is_none()) {
        return Err(CmdError::Other("Das Modell wird gerade übertragen".into()));
    }
    match std::fs::remove_file(state.models_dir().join(m.file)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(thera_core::Error::from(e).into()),
        _ => Ok(()),
    }
}

/// Fetches a model file from Hugging Face. Only the model travels; no patient data is sent.
#[tauri::command]
fn stt_model_download(app: AppHandle, id: String) -> CmdResult<()> {
    let m = *stt::model(&id)?;
    start_transfer(&app, m, Box::new(move || {
        let resp = ureq::get(m.url).call().map_err(|e| format!("Download fehlgeschlagen: {e}"))?;
        Ok(Box::new(resp.into_body().into_with_config().limit(m.bytes + 1).reader()) as Box<dyn std::io::Read>)
    }))
}

/// Copies a model file the user already has (offline installs). It must match the catalog hash.
#[tauri::command]
fn stt_model_import(app: AppHandle, id: String, path: String) -> CmdResult<()> {
    let m = *stt::model(&id)?;
    start_transfer(&app, m, Box::new(move || {
        Ok(Box::new(std::fs::File::open(&path).map_err(|e| e.to_string())?) as Box<dyn std::io::Read>)
    }))
}

type OpenSource = Box<dyn FnOnce() -> Result<Box<dyn std::io::Read>, String> + Send>;

/// Runs a download or import on its own thread; progress is read through `stt_overview`.
fn start_transfer(app: &AppHandle, m: stt::ModelInfo, open: OpenSource) -> CmdResult<()> {
    let state = app.state::<AppState>();
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
fn transfer_model(state: &AppState, m: &stt::ModelInfo, open: OpenSource) -> Result<(), String> {
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
    state.with(|s| s.list_reports(status))
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
        s.sign_report(&id, signed_by.trim())
    })
}

/// Writes PDF or DOCX to `path` (chosen by the user in a save dialog).
/// Nachtrag to a signed Sitzungsnotiz, signed on submit. Returns the updated note.
#[tauri::command]
fn report_add_addendum(state: State<'_, AppState>, id: String, body: String, signed_by: String) -> CmdResult<Report> {
    state.with(|s| s.add_addendum(&id, &body, &signed_by))
}

#[tauri::command]
fn report_export(state: State<'_, AppState>, id: String, format: String, path: String) -> CmdResult<()> {
    state.with(|s| {
        let v = report_view(s, &id)?;
        let practice = s.setting("practice_header")?.and_then(|v| v.as_str().map(str::to_string));
        let doc = render::build_doc(
            &v.report,
            &v.template,
            &v.patient,
            v.episode.as_ref(),
            v.application.as_ref(),
            v.session.as_ref(),
            practice,
        );
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
            patient_erase,
            consent_text,
            consents_list,
            consent_grant,
            consent_revoke,
            audio_devices,
            recording_start,
            recording_status,
            recording_stop,
            session_get,
            transcription_start,
            transcription_cancel,
            transcript_edit_segment,
            transcript_confirm,
            stt_overview,
            stt_model_select,
            stt_model_delete,
            stt_model_download,
            stt_model_import,
            episodes_list,
            episode_create,
            applications_list,
            application_create,
            sessions_list,
            session_create,
            session_note_create,
            templates_list,
            reports_list,
            report_get,
            report_save_section,
            report_checklist,
            report_sign,
            report_add_addendum,
            report_export,
            audit_list,
            audit_verify,
            setting_get,
            setting_set,
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
