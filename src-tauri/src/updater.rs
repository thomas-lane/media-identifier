//! App updates through `tauri-plugin-updater` and a signed `latest.json` on GitHub Releases.
//!
//! Flow (owner decision: ask before downloading):
//! 1. A background check at launch (when enabled) emits `update-available` with an
//!    [`UpdateInfo`] for a version the user has not skipped. Failures are only logged, because
//!    the releases are private for now and every check fails until the repository goes public.
//! 2. "Check now" calls [`check_for_update`], which returns `Failed` for the UI to show
//!    "Couldn't check for updates".
//! 3. "Install update" calls [`download_update`]; progress arrives on `update-event`, ending with
//!    `Downloaded`. The bytes are held in memory, not installed yet, because installing on
//!    Windows runs the installer and closes the app.
//! 4. "Relaunch now" calls [`install_update_and_relaunch`], which refuses with `Busy` while an
//!    identification runs, so an update never interrupts one.

use std::sync::Mutex;

use mi_types::{ApiError, ErrorCode, UpdateCheck, UpdateEvent, UpdateInfo, events};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::state::AppState;

/// A found update and, once downloaded, its verified bytes.
#[derive(Default)]
pub struct UpdaterState {
    found: Mutex<Option<Update>>,
    downloaded: Mutex<Option<(Update, Vec<u8>)>>,
}

fn info(update: &Update) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone().unwrap_or_default(),
        date: update.date.map(|d| d.to_string()),
    }
}

async fn check(app: &AppHandle) -> UpdateCheck {
    let current_version = app.package_info().version.to_string();
    let result = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let check = match result {
        Ok(Some(update)) => {
            let found = UpdateCheck::Available {
                info: info(&update),
            };
            if let Some(state) = app.try_state::<AppState>() {
                *state.updater.found.lock().expect("updater lock") = Some(update);
            }
            found
        }
        Ok(None) => UpdateCheck::UpToDate { current_version },
        Err(e) => UpdateCheck::Failed {
            message: e.to_string(),
        },
    };
    if let Some(state) = app.try_state::<AppState>() {
        let mut settings = state.settings.settings();
        settings.last_update_check_ms = Some(now_ms());
        if let Err(e) = state.settings.save(settings) {
            tracing::warn!("could not record the update check time: {e}");
        }
    }
    check
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Runs the launch-time check when "Check for updates automatically" is on.
pub fn spawn_background_check(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let settings = state.settings.settings();
        if !settings.check_updates_automatically {
            return;
        }
        match check(&app).await {
            UpdateCheck::Available { info }
                if settings.skipped_update_version.as_deref() != Some(info.version.as_str()) =>
            {
                if let Err(e) = app.emit(events::UPDATE_AVAILABLE_EVENT, info) {
                    tracing::warn!("could not emit update-available: {e}");
                }
            }
            UpdateCheck::Failed { message } => {
                tracing::info!("background update check failed: {message}")
            }
            _ => {}
        }
    });
}

/// "Check now". Shows skipped versions too, because the user asked explicitly.
#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> UpdateCheck {
    check(&app).await
}

/// Downloads the found update (after the user chose "Install update").
#[tauri::command]
pub async fn download_update(app: AppHandle, state: State<'_, AppState>) -> Result<(), ApiError> {
    let update = state
        .updater
        .found
        .lock()
        .expect("updater lock")
        .clone()
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "No update has been found."))?;
    let mut downloaded: u64 = 0;
    let progress_app = app.clone();
    let result = update
        .download(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = progress_app.emit(
                    events::UPDATE_EVENT,
                    UpdateEvent::Downloading { downloaded, total },
                );
            },
            || {},
        )
        .await;
    match result {
        Ok(bytes) => {
            let version = update.version.clone();
            *state.updater.downloaded.lock().expect("updater lock") = Some((update, bytes));
            let _ = app.emit(events::UPDATE_EVENT, UpdateEvent::Downloaded { version });
            Ok(())
        }
        Err(e) => {
            let message = e.to_string();
            let _ = app.emit(
                events::UPDATE_EVENT,
                UpdateEvent::Failed {
                    message: message.clone(),
                },
            );
            Err(ApiError::new(ErrorCode::Network, message))
        }
    }
}

/// "Relaunch now": installs the downloaded update and restarts. Refuses while a job runs.
#[tauri::command]
pub fn install_update_and_relaunch(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    if state.engine.is_job_running() {
        return Err(ApiError::new(
            ErrorCode::Busy,
            "The update will install when identification finishes.",
        ));
    }
    let (update, bytes) = state
        .updater
        .downloaded
        .lock()
        .expect("updater lock")
        .take()
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "No update has been downloaded."))?;
    update
        .install(bytes)
        .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?;
    app.restart();
}

/// "Skip this version": background checks stop offering it.
#[tauri::command]
pub fn skip_update_version(state: State<'_, AppState>, version: String) -> Result<(), ApiError> {
    let mut settings = state.settings.settings();
    settings.skipped_update_version = Some(version);
    state
        .settings
        .save(settings)
        .map_err(|e| ApiError::new(ErrorCode::Io, e.to_string()))
}
