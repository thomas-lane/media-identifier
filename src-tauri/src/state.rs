//! Managed application state.

use std::sync::Arc;

use mi_core::{Engine, EngineConfig};
use mi_media::SidecarLookup;
use tauri::{AppHandle, Manager};

use crate::settings_store::SettingsStore;
use crate::sink::TauriSink;
use crate::updater::UpdaterState;

/// Everything commands need, registered with `app.manage`.
pub struct AppState {
    /// The identification engine.
    pub engine: Engine,
    /// Persisted settings and API keys.
    pub settings: SettingsStore,
    /// A found or downloaded update.
    pub updater: UpdaterState,
}

impl AppState {
    /// Loads settings from the app config folder and creates the engine over the app data folder.
    /// Sidecars are looked up next to the executable; development builds also search `PATH`.
    pub fn initialise(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let config_dir = app.path().app_config_dir()?;
        let data_dir = app.path().app_data_dir()?;
        let settings = SettingsStore::load(&config_dir)?;
        let exe_dir = std::env::current_exe()?
            .parent()
            .map(std::path::Path::to_path_buf);
        let engine = Engine::new(
            EngineConfig {
                data_dir,
                sidecars: SidecarLookup {
                    exe_dir,
                    allow_path_fallback: cfg!(debug_assertions),
                },
                settings: settings.settings(),
                keys: settings.api_keys(),
            },
            Arc::new(TauriSink::new(app.clone())),
        )?;
        Ok(Self {
            engine,
            settings,
            updater: UpdaterState::default(),
        })
    }
}
