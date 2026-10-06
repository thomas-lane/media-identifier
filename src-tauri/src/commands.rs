//! Tauri commands. Each returns `Result<T, ApiError>`; arguments arrive camelCased from the UI.

use std::path::PathBuf;

use mi_types::{
    ApiError, ApiKeyProvider, Attribution, ErrorCode, HistoryEntry, HistoryId, JobId, JobRequest,
    JobResults, ModelStatus, RecentJob, RenameOutcome, RenamePlan, RenamePlanRequest, ScanSummary,
    Settings, ShowCandidate, SourceStatus, SpeechModel, UndoOutcome,
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
    "attributions",
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
    "cancel_update_download",
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

/// Saves settings and applies them to the engine. The two fields the updater maintains (time of
/// the last check, skipped version) keep their stored values: the window's copy may be stale.
#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), ApiError> {
    let settings = from_window(state.settings.settings(), settings);
    state.settings.save(settings.clone()).map_err(io_error)?;
    state
        .engine
        .update_settings(settings, state.settings.api_keys());
    Ok(())
}

/// The settings the window sent, with the fields only the updater writes kept from `stored`.
fn from_window(stored: Settings, sent: Settings) -> Settings {
    Settings {
        last_update_check_ms: stored.last_update_check_ms,
        skipped_update_version: stored.skipped_update_version,
        ..sent
    }
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

/// The credits each online source requires (About screen and the credit lines where their data
/// is shown).
#[tauri::command]
pub fn attributions() -> Vec<Attribution> {
    mi_sources::attributions()
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

/// Applies a rename plan (on a background thread: copies can take a while).
#[tauri::command]
pub async fn apply_rename(
    state: State<'_, AppState>,
    plan: RenamePlan,
) -> Result<RenameOutcome, ApiError> {
    state
        .engine
        .apply_rename(plan)
        .await
        .map_err(ApiError::from)
}

/// History entries.
#[tauri::command]
pub fn list_history(state: State<'_, AppState>) -> Result<Vec<HistoryEntry>, ApiError> {
    state.engine.history().map_err(ApiError::from)
}

/// Undoes a History entry.
#[tauri::command]
pub async fn undo_history(
    state: State<'_, AppState>,
    id: HistoryId,
) -> Result<UndoOutcome, ApiError> {
    state.engine.undo(&id).await.map_err(ApiError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui_client() -> String {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/src/api/tauri.ts");
        std::fs::read_to_string(path).expect("ui/src/api/tauri.ts exists")
    }

    #[test]
    fn saving_from_the_window_keeps_the_updater_fields() {
        let stored = Settings {
            last_update_check_ms: Some(1_000),
            skipped_update_version: Some("1.3.0".into()),
            ..Settings::default()
        };
        let sent = Settings {
            speech_model: mi_types::SpeechModel::Fast,
            last_update_check_ms: None,
            skipped_update_version: None,
            ..Settings::default()
        };
        let saved = from_window(stored, sent);
        assert_eq!(saved.speech_model, mi_types::SpeechModel::Fast);
        assert_eq!(saved.last_update_check_ms, Some(1_000));
        assert_eq!(saved.skipped_update_version.as_deref(), Some("1.3.0"));
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

    /// The URL patterns `opener:allow-open-url` allows. Each must be an exact URL or end in
    /// `/*`, so this test can match them without the plugin's glob engine.
    fn allowed_urls() -> Vec<String> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities/default.json");
        let capability: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let permissions = capability["permissions"].as_array().unwrap();
        let open_url = permissions
            .iter()
            .find(|p| p["identifier"] == "opener:allow-open-url")
            .expect("a scoped opener:allow-open-url");
        open_url["allow"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["url"].as_str().unwrap().to_owned())
            .collect()
    }

    fn url_allowed(patterns: &[String], url: &str) -> bool {
        patterns.iter().any(|p| match p.strip_suffix('*') {
            Some(prefix) => url.starts_with(prefix),
            None => url == p,
        })
    }

    #[test]
    fn every_link_the_window_opens_is_allowed() {
        let patterns = allowed_urls();
        for p in &patterns {
            assert!(
                p.starts_with("https://") && !p[..p.len() - 1].contains('*'),
                "{p}"
            );
        }
        // Literal links in the UI, and the credits the app sends it.
        let mut urls: Vec<String> = Vec::new();
        let screens = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/src");
        let mut stack = vec![std::path::PathBuf::from(screens)];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                let name = path.to_string_lossy().into_owned();
                if path.is_dir() {
                    stack.push(path);
                } else if (name.ends_with(".tsx") || name.ends_with(".ts"))
                    && !name.contains(".test.")
                    && !name.contains("mock")
                {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for part in text.split('"').skip(1).step_by(2) {
                        if part.starts_with("https://") {
                            urls.push(part.to_owned());
                        }
                    }
                }
            }
        }
        for a in mi_sources::attributions() {
            urls.push(a.url);
            urls.extend(a.license_url);
        }
        assert!(urls.len() > 5, "{urls:?}");
        for url in urls {
            assert!(url_allowed(&patterns, &url), "{url} is not allowed");
        }
        assert!(!url_allowed(
            &patterns,
            "https://www.tvmaze.com.evil.example/"
        ));
        assert!(!url_allowed(&patterns, "http://www.tvmaze.com/"));
    }

    #[test]
    fn every_scanned_video_extension_can_be_played() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities/default.json");
        let text = std::fs::read_to_string(path).unwrap();
        for ext in mi_media::VIDEO_EXTENSIONS {
            assert!(
                text.contains(&format!("\"**/*.{ext}\"")),
                "opener:allow-open-path lacks {ext}"
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
