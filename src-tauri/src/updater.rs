//! App updates through `tauri-plugin-updater` and a signed `latest.json` on GitHub Releases.
//!
//! The endpoint and the public key that verifies downloads are in `tauri.conf.json`
//! (`plugins.updater`). The flow follows the owner's choice to ask before downloading:
//!
//! 1. **Background checks.** At launch, and then whenever the last check is 24 hours old while
//!    the app stays open, the app checks when "Check for updates automatically" is on. A found
//!    version is offered (event `update-available`) unless the user skipped it, or it was already
//!    offered in the last 24 hours ("Remind me later" simply closes the dialog, so the next offer
//!    comes a day later, also across relaunches). Failures are only logged, because the releases
//!    are private for now and every check fails until the repository goes public.
//! 2. **Check now** calls [`check_for_update`], which reports skipped versions too and returns
//!    `Failed` for the UI to show "Couldn't check for updates".
//! 3. **Install update** calls [`download_update`]; progress arrives on `update-event`, ending with
//!    `Downloaded`, and [`cancel_update_download`] stops it. The verified bytes are held in
//!    memory, because installing on Windows runs the installer and closes the app at once.
//! 4. **Relaunch now** calls [`install_update_and_relaunch`]. While an identification runs it
//!    returns `Busy` and installs and relaunches by itself when the job ends, so an update never
//!    interrupts one.
//!
//! The scheduling and offering rules are plain functions ([`is_check_due`], [`should_offer`],
//! [`ProgressThrottle`], [`wait_until_idle`]) tested without Tauri.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mi_types::{ApiError, ErrorCode, UpdateCheck, UpdateEvent, UpdateInfo, events};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::state::AppState;

/// Time between background checks: 24 hours.
pub const CHECK_INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;

/// How often the open app looks whether a background check is due.
const SCHEDULE_TICK: Duration = Duration::from_secs(60 * 60);

/// How often a deferred install looks whether the identification has finished.
const IDLE_POLL: Duration = Duration::from_secs(2);

/// File in the app config folder recording the last version offered and when.
const OFFER_FILE: &str = "update-offer.json";

/// A found update, the download in progress, and the downloaded bytes.
#[derive(Default)]
pub struct UpdaterState {
    found: Mutex<Option<Update>>,
    downloaded: Mutex<Option<(Update, Vec<u8>)>>,
    cancel: Mutex<Option<Arc<tokio::sync::Notify>>>,
    install_pending: AtomicBool,
}

/// The last version offered to the user (`<app config>/update-offer.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferRecord {
    /// Version offered.
    pub version: String,
    /// When it was offered, Unix milliseconds.
    pub offered_at_ms: i64,
}

impl OfferRecord {
    /// Reads the record; a missing or damaged file means nothing was offered.
    pub fn load(config_dir: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(config_dir.join(OFFER_FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Writes the record.
    pub fn save(&self, config_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(config_dir)?;
        let text = serde_json::to_string(self).map_err(std::io::Error::other)?;
        std::fs::write(config_dir.join(OFFER_FILE), text)
    }
}

/// Whether a background check is due: never checked, or the last check is at least
/// [`CHECK_INTERVAL_MS`] old (or in the future, after the clock was changed).
pub fn is_check_due(last_check_ms: Option<i64>, now_ms: i64) -> bool {
    match last_check_ms {
        None => true,
        Some(last) => now_ms - last >= CHECK_INTERVAL_MS || last > now_ms,
    }
}

/// Whether a background check should show the update dialog for `version`: not when the user
/// skipped that version, and not when the same version was offered less than
/// [`CHECK_INTERVAL_MS`] ago (the user chose "Remind me later" or closed the dialog).
pub fn should_offer(
    version: &str,
    skipped: Option<&str>,
    last_offer: Option<&OfferRecord>,
    now_ms: i64,
) -> bool {
    if skipped == Some(version) {
        return false;
    }
    match last_offer {
        Some(offer) if offer.version == version => {
            now_ms - offer.offered_at_ms >= CHECK_INTERVAL_MS || offer.offered_at_ms > now_ms
        }
        _ => true,
    }
}

/// Limits download progress events to about one per percent, or one per 250 ms when the size
/// is unknown, so a large download does not flood the UI with thousands of events.
#[derive(Debug, Default)]
pub struct ProgressThrottle {
    downloaded: u64,
    last_emitted_bytes: u64,
    last_emitted_at: Option<std::time::Instant>,
}

impl ProgressThrottle {
    /// Adds a chunk; returns the total downloaded so far when an event is due.
    pub fn add(
        &mut self,
        chunk: usize,
        total: Option<u64>,
        now: std::time::Instant,
    ) -> Option<u64> {
        self.downloaded += chunk as u64;
        let due = match (total, self.last_emitted_at) {
            (_, None) => true,
            (Some(total), _) if self.downloaded >= total => true,
            (Some(total), _) => (self.downloaded - self.last_emitted_bytes) * 100 >= total.max(1),
            (None, Some(at)) => now.duration_since(at) >= Duration::from_millis(250),
        };
        if due {
            self.last_emitted_bytes = self.downloaded;
            self.last_emitted_at = Some(now);
            Some(self.downloaded)
        } else {
            None
        }
    }
}

/// Waits until `is_busy` returns false, checking every `poll`.
pub async fn wait_until_idle(is_busy: impl Fn() -> bool, poll: Duration) {
    while is_busy() {
        tokio::time::sleep(poll).await;
    }
}

fn info(update: &Update) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone().unwrap_or_default(),
        // `latest.json` carries the date as RFC 3339 text; pass it on unchanged.
        date: update
            .raw_json
            .get("pub_date")
            .and_then(|d| d.as_str())
            .map(str::to_owned),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

fn config_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok()
}

fn remember_offer(app: &AppHandle, version: &str) {
    if let Some(dir) = config_dir(app) {
        let record = OfferRecord {
            version: version.to_owned(),
            offered_at_ms: now_ms(),
        };
        if let Err(e) = record.save(&dir) {
            tracing::warn!("could not record the offered update: {e}");
        }
    }
}

/// Asks the update endpoint, keeps a found update for downloading, and records the check time.
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

/// One background check: offers the update when [`should_offer`] allows; logs failures.
async fn background_check(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let skipped = state.settings.settings().skipped_update_version;
    match check(app).await {
        UpdateCheck::Available { info } => {
            let last_offer = config_dir(app).and_then(|d| OfferRecord::load(&d));
            if should_offer(
                &info.version,
                skipped.as_deref(),
                last_offer.as_ref(),
                now_ms(),
            ) {
                remember_offer(app, &info.version);
                if let Err(e) = app.emit(events::UPDATE_AVAILABLE_EVENT, info) {
                    tracing::warn!("could not emit update-available: {e}");
                }
            }
        }
        UpdateCheck::Failed { message } => {
            tracing::info!("background update check failed: {message}")
        }
        UpdateCheck::UpToDate { .. } => {}
    }
}

/// Starts the background checks: one at launch, then one whenever [`is_check_due`] while the app
/// stays open. Each check first reads "Check for updates automatically", so turning it off or on
/// takes effect without a restart.
pub fn spawn_background_check(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut at_launch = true;
        loop {
            if let Some(state) = app.try_state::<AppState>() {
                let settings = state.settings.settings();
                let due = at_launch || is_check_due(settings.last_update_check_ms, now_ms());
                if settings.check_updates_automatically && due {
                    background_check(&app).await;
                }
            }
            at_launch = false;
            tokio::time::sleep(SCHEDULE_TICK).await;
        }
    });
}

/// "Check now". Reports skipped versions too, because the user asked explicitly.
#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> UpdateCheck {
    let result = check(&app).await;
    match &result {
        UpdateCheck::Available { info } => remember_offer(&app, &info.version),
        UpdateCheck::Failed { message } => tracing::warn!("update check failed: {message}"),
        UpdateCheck::UpToDate { .. } => {}
    }
    result
}

/// Downloads the found update after the user chose "Install update". Progress arrives on
/// `update-event`; the download is verified against the public key before `Downloaded`.
#[tauri::command]
pub async fn download_update(app: AppHandle, state: State<'_, AppState>) -> Result<(), ApiError> {
    let update = state
        .updater
        .found
        .lock()
        .expect("updater lock")
        .clone()
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "No update has been found."))?;
    let cancel = Arc::new(tokio::sync::Notify::new());
    {
        let mut slot = state.updater.cancel.lock().expect("updater lock");
        if slot.is_some() {
            return Err(ApiError::new(
                ErrorCode::Busy,
                "The update is already downloading.",
            ));
        }
        *slot = Some(cancel.clone());
    }

    let progress_app = app.clone();
    let mut throttle = ProgressThrottle::default();
    let download = update.download(
        move |chunk, total| {
            if let Some(downloaded) = throttle.add(chunk, total, std::time::Instant::now()) {
                let _ = progress_app.emit(
                    events::UPDATE_EVENT,
                    UpdateEvent::Downloading { downloaded, total },
                );
            }
        },
        || {},
    );
    let result = tokio::select! {
        result = download => Some(result),
        () = cancel.notified() => None,
    };
    *state.updater.cancel.lock().expect("updater lock") = None;

    match result {
        Some(Ok(bytes)) => {
            let version = update.version.clone();
            *state.updater.downloaded.lock().expect("updater lock") = Some((update, bytes));
            let _ = app.emit(events::UPDATE_EVENT, UpdateEvent::Downloaded { version });
            Ok(())
        }
        Some(Err(e)) => {
            let message = format!("The update could not be downloaded: {e}");
            let _ = app.emit(
                events::UPDATE_EVENT,
                UpdateEvent::Failed {
                    message: message.clone(),
                },
            );
            Err(ApiError::new(ErrorCode::Network, message))
        }
        None => Err(ApiError::new(
            ErrorCode::Cancelled,
            "The download was cancelled.",
        )),
    }
}

/// "Cancel" while an update downloads: stops the download; nothing is kept.
#[tauri::command]
pub fn cancel_update_download(state: State<'_, AppState>) {
    if let Some(cancel) = state.updater.cancel.lock().expect("updater lock").as_ref() {
        cancel.notify_one();
    }
}

/// "Relaunch now": installs the downloaded update and restarts.
///
/// While an identification runs, it returns `Busy` and installs and relaunches by itself as soon
/// as the job ends.
#[tauri::command]
pub fn install_update_and_relaunch(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    if state
        .updater
        .downloaded
        .lock()
        .expect("updater lock")
        .is_none()
    {
        return Err(ApiError::new(
            ErrorCode::NotFound,
            "No update has been downloaded.",
        ));
    }
    if state.engine.is_job_running() {
        if !state.updater.install_pending.swap(true, Ordering::SeqCst) {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let busy_app = app.clone();
                wait_until_idle(
                    move || {
                        busy_app
                            .try_state::<AppState>()
                            .is_some_and(|s| s.engine.is_job_running())
                    },
                    IDLE_POLL,
                )
                .await;
                if let Some(state) = app.try_state::<AppState>()
                    && let Err(e) = install_now(&app, &state)
                {
                    tracing::warn!("deferred update install failed: {}", e.message);
                    state.updater.install_pending.store(false, Ordering::SeqCst);
                }
            });
        }
        return Err(ApiError::new(
            ErrorCode::Busy,
            "Media Identifier will relaunch to finish the update when identification finishes.",
        ));
    }
    install_now(&app, &state)
}

fn install_now(app: &AppHandle, state: &AppState) -> Result<(), ApiError> {
    let (update, bytes) = state
        .updater
        .downloaded
        .lock()
        .expect("updater lock")
        .take()
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "No update has been downloaded."))?;
    update.install(bytes).map_err(|e| {
        ApiError::new(
            ErrorCode::Internal,
            format!("The update could not be installed: {e}"),
        )
    })?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const HOUR: i64 = 60 * 60 * 1000;

    #[test]
    fn a_check_is_due_after_24_hours_or_when_never_checked() {
        let now = 1_000 * HOUR;
        assert!(is_check_due(None, now));
        assert!(!is_check_due(Some(now - 23 * HOUR), now));
        assert!(is_check_due(Some(now - 24 * HOUR), now));
        assert!(is_check_due(Some(now + HOUR), now), "clock moved back");
    }

    #[test]
    fn skipped_versions_are_never_offered_in_the_background() {
        assert!(!should_offer("1.3.0", Some("1.3.0"), None, 0));
        assert!(should_offer("1.4.0", Some("1.3.0"), None, 0));
    }

    #[test]
    fn remind_later_waits_a_day_for_the_same_version() {
        let now = 1_000 * HOUR;
        let offer = OfferRecord {
            version: "1.3.0".into(),
            offered_at_ms: now - 2 * HOUR,
        };
        assert!(!should_offer("1.3.0", None, Some(&offer), now));
        assert!(should_offer("1.3.0", None, Some(&offer), now + 22 * HOUR));
        assert!(
            should_offer("1.4.0", None, Some(&offer), now),
            "a newer version is offered at once"
        );
    }

    #[test]
    fn the_offer_record_survives_a_restart_and_damage_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(OfferRecord::load(dir.path()), None);
        let record = OfferRecord {
            version: "1.3.0".into(),
            offered_at_ms: 42,
        };
        record.save(dir.path()).unwrap();
        assert_eq!(OfferRecord::load(dir.path()), Some(record));
        std::fs::write(dir.path().join(OFFER_FILE), "{not json").unwrap();
        assert_eq!(OfferRecord::load(dir.path()), None);
    }

    #[test]
    fn progress_is_reported_about_once_per_percent() {
        let start = Instant::now();
        let mut throttle = ProgressThrottle::default();
        let total = Some(100_000);
        let reported: Vec<u64> = (0..100)
            .filter_map(|_| throttle.add(250, total, start))
            .collect();
        // 100 chunks of 250 bytes: the first chunk, then every 1 000 bytes (1%).
        assert_eq!(reported.first(), Some(&250));
        assert_eq!(reported.len(), 25);
        assert_eq!(
            throttle.add(75_000, total, start),
            Some(100_000),
            "the end is always reported"
        );
    }

    #[test]
    fn progress_without_a_size_is_reported_by_time() {
        let start = Instant::now();
        let mut throttle = ProgressThrottle::default();
        assert_eq!(throttle.add(10, None, start), Some(10));
        assert_eq!(
            throttle.add(10, None, start + Duration::from_millis(100)),
            None
        );
        assert_eq!(
            throttle.add(10, None, start + Duration::from_millis(300)),
            Some(30)
        );
    }

    #[tokio::test]
    async fn a_deferred_install_waits_for_the_job_to_finish() {
        let polls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counter = polls.clone();
        wait_until_idle(
            move || counter.fetch_add(1, Ordering::SeqCst) < 3,
            Duration::from_millis(1),
        )
        .await;
        assert_eq!(
            polls.load(Ordering::SeqCst),
            4,
            "busy three times, then idle"
        );
    }
}
