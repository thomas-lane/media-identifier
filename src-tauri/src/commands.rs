//! Tauri commands. Each returns `Result<T, ApiError>`; arguments arrive camelCased from the UI.

use std::path::PathBuf;

use mi_types::{
    ApiError, ApiKeyProvider, ErrorCode, HistoryEntry, HistoryId, JobId, JobRequest, JobResults,
    ModelStatus, RecentJob, RenameOutcome, RenamePlan, RenamePlanRequest, ScanSummary, Settings,
    ShowCandidate, SourceStatus, SpeechModel, UndoOutcome,
};
use tauri::State;

use crate::state::AppState;

/// Every command name registered in `lib.rs`, in registration order. The UI's Tauri client
/// (`ui/src/api/tauri.ts`) must call exactly these; `tests::ui_client_calls_every_command`
/// checks it.
pub const COMMANDS: &[&str] = &[
    "app_version",
    "get_settings",
    "save_settings",
    "set_api_key",
    "source_status",
    "model_status",
    "download_model",
    "pause_model_download",
    "scan_folder",
    "search_shows",
    "start_identification",
    "cancel_identification",
    "job_results",
    "recent_jobs",
    "plan_rename",
    "apply_rename",
    "list_history",
    "undo_history",
    "check_for_update",
    "download_update",
    "install_update_and_relaunch",
    "skip_update_version",
];

fn io_error(e: std::io::Error) -> ApiError {
    ApiError::new(ErrorCode::Io, e.to_string())
}

/// The running app version (from `Cargo.toml`).
#[tauri::command]
pub fn app_version(app: tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Current settings.
#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.settings()
}

/// Saves settings and applies them to the engine.
#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), ApiError> {
    state.settings.save(settings.clone()).map_err(io_error)?;
    state
        .engine
        .update_settings(settings, state.settings.api_keys());
    Ok(())
}

/// Sets or clears an API key (`null` or empty clears).
#[tauri::command]
pub fn set_api_key(
    state: State<'_, AppState>,
    provider: ApiKeyProvider,
    key: Option<String>,
) -> Result<(), ApiError> {
    state.settings.set_key(provider, key).map_err(io_error)?;
    state
        .engine
        .update_settings(state.settings.settings(), state.settings.api_keys());
    Ok(())
}

/// Status of each online source.
#[tauri::command]
pub fn source_status(state: State<'_, AppState>) -> Vec<SourceStatus> {
    state.engine.source_status()
}

/// Download state of a speech model.
#[tauri::command]
pub fn model_status(state: State<'_, AppState>, model: SpeechModel) -> ModelStatus {
    state.engine.model_status(model)
}

/// Downloads or resumes a model; progress on the `model-download` channel.
#[tauri::command]
pub async fn download_model(
    state: State<'_, AppState>,
    model: SpeechModel,
) -> Result<(), ApiError> {
    state
        .engine
        .download_model(model)
        .await
        .map_err(ApiError::from)
}

/// Pauses the model download.
#[tauri::command]
pub fn pause_model_download(state: State<'_, AppState>) {
    state.engine.pause_model_download();
}

/// Scans a folder.
#[tauri::command]
pub async fn scan_folder(
    state: State<'_, AppState>,
    folder: PathBuf,
) -> Result<ScanSummary, ApiError> {
    state.engine.scan(&folder).await.map_err(ApiError::from)
}

/// Searches shows.
#[tauri::command]
pub async fn search_shows(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<ShowCandidate>, ApiError> {
    state
        .engine
        .search_shows(&query)
        .await
        .map_err(ApiError::from)
}

/// Starts identifying; progress on the `job-event` channel.
#[tauri::command]
pub fn start_identification(
    state: State<'_, AppState>,
    request: JobRequest,
) -> Result<JobId, ApiError> {
    state.engine.start_job(request).map_err(ApiError::from)
}

/// Cancels a job.
#[tauri::command]
pub fn cancel_identification(state: State<'_, AppState>, job_id: JobId) -> Result<(), ApiError> {
    state.engine.cancel_job(&job_id).map_err(ApiError::from)
}

/// Results of a job.
#[tauri::command]
pub fn job_results(state: State<'_, AppState>, job_id: JobId) -> Result<JobResults, ApiError> {
    state.engine.job_results(&job_id).map_err(ApiError::from)
}

/// Recent jobs.
#[tauri::command]
pub fn recent_jobs(state: State<'_, AppState>) -> Result<Vec<RecentJob>, ApiError> {
    state.engine.recent_jobs().map_err(ApiError::from)
}

/// Builds a rename preview.
#[tauri::command]
pub fn plan_rename(
    state: State<'_, AppState>,
    request: RenamePlanRequest,
) -> Result<RenamePlan, ApiError> {
    state.engine.plan_rename(&request).map_err(ApiError::from)
}

/// Applies a rename plan.
#[tauri::command]
pub fn apply_rename(
    state: State<'_, AppState>,
    plan: RenamePlan,
) -> Result<RenameOutcome, ApiError> {
    state.engine.apply_rename(&plan).map_err(ApiError::from)
}

/// History entries.
#[tauri::command]
pub fn list_history(state: State<'_, AppState>) -> Result<Vec<HistoryEntry>, ApiError> {
    state.engine.history().map_err(ApiError::from)
}

/// Undoes a History entry.
#[tauri::command]
pub fn undo_history(state: State<'_, AppState>, id: HistoryId) -> Result<UndoOutcome, ApiError> {
    state.engine.undo(&id).map_err(ApiError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui_client() -> String {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/src/api/tauri.ts");
        std::fs::read_to_string(path).expect("ui/src/api/tauri.ts exists")
    }

    #[test]
    fn ui_client_calls_every_command() {
        let client = ui_client();
        for name in COMMANDS {
            assert!(
                client.contains(&format!("\"{name}\"")),
                "tauri.ts does not call {name}"
            );
        }
    }

    #[test]
    fn registered_handler_list_matches_commands() {
        let lib = include_str!("lib.rs");
        let start = lib.find("generate_handler![").expect("handler list");
        let end = lib[start..].find("])").expect("end of handler list") + start;
        let registered: Vec<&str> = lib[start..end]
            .split(',')
            .filter_map(|item| item.trim().rsplit("::").next())
            .map(|s| s.trim_start_matches("generate_handler![").trim())
            .filter(|s| !s.is_empty())
            .collect();
        assert_eq!(registered, COMMANDS);
    }
}
