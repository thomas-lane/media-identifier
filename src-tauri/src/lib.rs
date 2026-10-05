//! The Tauri shell: window, plugins, commands, events, settings persistence and the updater.
//!
//! All identification logic lives in `mi-core`; this crate translates between Tauri and the
//! engine. Command names are listed in [`commands::COMMANDS`] and mirrored by the UI's Tauri
//! client in `ui/src/api/tauri.ts` (a test keeps them in sync).

pub mod commands;
pub mod settings_store;
pub mod sink;
pub mod state;
pub mod updater;

use tauri::Manager;

/// Starts the app.
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let state = state::AppState::initialise(app.handle())?;
            app.manage(state);
            updater::spawn_background_check(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_version,
            commands::get_settings,
            commands::save_settings,
            commands::set_api_key,
            commands::source_status,
            commands::model_status,
            commands::download_model,
            commands::pause_model_download,
            commands::scan_folder,
            commands::search_shows,
            commands::start_identification,
            commands::cancel_identification,
            commands::job_results,
            commands::recent_jobs,
            commands::plan_rename,
            commands::apply_rename,
            commands::list_history,
            commands::undo_history,
            updater::check_for_update,
            updater::download_update,
            updater::install_update_and_relaunch,
            updater::skip_update_version,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Media Identifier");
}
