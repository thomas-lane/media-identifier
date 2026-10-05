//! Names of the Tauri event channels. The bindings test also writes them to
//! `ui/src/types/generated/events.ts`, so the UI never spells them by hand.

/// Channel carrying [`crate::JobEvent`] payloads.
pub const JOB_EVENT: &str = "job-event";
/// Channel carrying [`crate::ModelStatus`] payloads while a model downloads.
pub const MODEL_DOWNLOAD_EVENT: &str = "model-download";
/// Channel carrying [`crate::UpdateEvent`] payloads.
pub const UPDATE_EVENT: &str = "update-event";
/// Channel carrying an [`crate::UpdateInfo`] when a background check finds a new version.
pub const UPDATE_AVAILABLE_EVENT: &str = "update-available";

/// Every channel as `(TypeScript constant name, channel name)`, for code generation.
pub const ALL: &[(&str, &str)] = &[
    ("JOB_EVENT", JOB_EVENT),
    ("MODEL_DOWNLOAD_EVENT", MODEL_DOWNLOAD_EVENT),
    ("UPDATE_EVENT", UPDATE_EVENT),
    ("UPDATE_AVAILABLE_EVENT", UPDATE_AVAILABLE_EVENT),
];
