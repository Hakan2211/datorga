//! Tauri shell: thin command wrappers over `thera_core`. No business logic lives here.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{Manager, State};
use thera_core::crypto::{self, KeyStore, MasterKey, Unlock};
use thera_core::domain::*;
use thera_core::pii::{self, CheckItem};
use thera_core::render;
use thera_core::store::Store;
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

struct OsKeyStore;

impl KeyStore for OsKeyStore {
    fn load(&self) -> thera_core::Result<Option<MasterKey>> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(kr)?;
        match entry.get_password() {
            Ok(hex) => Ok(Some(MasterKey::from_hex(&hex)?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(kr(e)),
        }
    }

    fn store(&self, key: &MasterKey) -> thera_core::Result<()> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(kr)?.set_password(&key.to_hex()).map_err(kr)
    }
}

fn kr(e: keyring::Error) -> thera_core::Error {
    thera_core::Error::Crypto(format!("keychain: {e}"))
}

// ---------- state ----------

struct AppState {
    data_dir: PathBuf,
    store: Mutex<Option<Store>>,
}

impl AppState {
    fn db_path(&self) -> PathBuf {
        self.data_dir.join("thera.db")
    }
    fn wrapped_path(&self) -> PathBuf {
        self.data_dir.join("master.wrapped")
    }
    fn with<T>(&self, f: impl FnOnce(&Store) -> thera_core::Result<T>) -> CmdResult<T> {
        let guard = self.store.lock().map_err(|_| CmdError::Other("state poisoned".into()))?;
        let store = guard.as_ref().ok_or_else(|| CmdError::Other("locked".into()))?;
        Ok(f(store)?)
    }
}

#[derive(Serialize)]
struct InitResult {
    /// "ready" | "recovery_required"
    status: &'static str,
    /// Shown once on first run; the user must write it down / print it.
    recovery_key: Option<String>,
    encrypted: bool,
}

#[tauri::command]
fn app_init(state: State<'_, AppState>) -> CmdResult<InitResult> {
    if state.store.lock().map_err(|_| CmdError::Other("state poisoned".into()))?.is_some() {
        return Ok(InitResult { status: "ready", recovery_key: None, encrypted: Store::is_encrypted() });
    }
    let (key, recovery_key) = match crypto::unlock_or_init(&OsKeyStore, &state.wrapped_path()) {
        Ok(Unlock::Existing(k)) => (k, None),
        Ok(Unlock::Created { key, recovery }) => (key, Some(recovery.display())),
        Err(thera_core::Error::Crypto(m)) if m == "recovery_required" => {
            return Ok(InitResult { status: "recovery_required", recovery_key: None, encrypted: Store::is_encrypted() })
        }
        Err(e) => return Err(e.into()),
    };
    let store = Store::open(&state.db_path(), &key)?;
    *state.store.lock().map_err(|_| CmdError::Other("state poisoned".into()))? = Some(store);
    Ok(InitResult { status: "ready", recovery_key, encrypted: Store::is_encrypted() })
}

#[tauri::command]
fn app_recover(state: State<'_, AppState>, recovery_key: String) -> CmdResult<()> {
    let key = crypto::recover(&OsKeyStore, &state.wrapped_path(), &recovery_key)?;
    let store = Store::open(&state.db_path(), &key)?;
    *state.store.lock().map_err(|_| CmdError::Other("state poisoned".into()))? = Some(store);
    Ok(())
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
    state.with(|s| s.erase_patient(&id))
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
}

fn report_view(s: &Store, id: &str) -> thera_core::Result<ReportView> {
    let report = s.report(id)?;
    let patient = s.patient(&report.patient_id)?;
    let episode = report.episode_id.as_deref().map(|e| s.episode(e)).transpose()?;
    let application = match (&episode, &report.application_id) {
        (Some(ep), Some(app_id)) => s.list_applications(&ep.id)?.into_iter().find(|a| &a.id == app_id),
        _ => None,
    };
    let mut template = s.template(&report.template_key)?;
    if let (Some(ep), Some(app)) = (&episode, &application) {
        template = template.resolve(&Context { method: ep.method, age_group: ep.age_group, kind: app.kind });
    }
    Ok(ReportView { report, template, patient, episode, application })
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
#[tauri::command]
fn report_export(state: State<'_, AppState>, id: String, format: String, path: String) -> CmdResult<()> {
    state.with(|s| {
        let v = report_view(s, &id)?;
        let practice = s.setting("practice_header")?.and_then(|v| v.as_str().map(str::to_string));
        let doc = render::build_doc(&v.report, &v.template, &v.patient, v.episode.as_ref(), v.application.as_ref(), practice);
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
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            app.manage(AppState { data_dir, store: Mutex::new(None) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_init,
            app_recover,
            patients_list,
            patient_get,
            patient_create,
            patient_update,
            patient_erase,
            episodes_list,
            episode_create,
            applications_list,
            application_create,
            sessions_list,
            session_create,
            templates_list,
            reports_list,
            report_get,
            report_save_section,
            report_checklist,
            report_sign,
            report_export,
            audit_list,
            audit_verify,
            setting_get,
            setting_set,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
