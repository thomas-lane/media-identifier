//! User settings and the status of online sources.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::catalog::ProviderId;
use crate::rename::{NamingScheme, SaveModeKind};
use crate::transcript::SpeechModel;

/// Persisted user settings (stored as JSON in the app's config folder).
///
/// API keys are stored separately (see [`ApiKeyProvider`]) and never sent back to the UI; the UI
/// learns only whether a key is set, through [`SourceStatus::has_key`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Speech model to use.
    pub speech_model: SpeechModel,
    /// Transcribe a few windows of files longer than six minutes instead of the whole file.
    pub sample_long_files: bool,
    /// Default language (ISO 639-1).
    pub language: String,
    /// Check for updates in the background at launch.
    pub check_updates_automatically: bool,
    /// When the last check finished, Unix milliseconds.
    pub last_update_check_ms: Option<i64>,
    /// A version the user chose to skip; that version is not offered again.
    pub skipped_update_version: Option<String>,
    /// Default naming scheme on the Rename screen.
    pub naming: NamingScheme,
    /// Default way to save results.
    pub save_mode: SaveModeKind,
    /// Also write an `.srt` subtitle file from what was heard next to each renamed file.
    pub save_heard_subtitles: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            speech_model: SpeechModel::Accurate,
            sample_long_files: true,
            language: "en".to_owned(),
            check_updates_automatically: true,
            last_update_check_ms: None,
            skipped_update_version: None,
            naming: NamingScheme::JellyfinPlex,
            save_mode: SaveModeKind::RenameInPlace,
            save_heard_subtitles: false,
        }
    }
}

/// Providers that take a user-entered API key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ApiKeyProvider {
    /// SubDL (a free account key is required for its API).
    Subdl,
    /// TMDb (optional; enables Jellyfin-consistent numbering).
    Tmdb,
}

/// The state of one online source on the Settings screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SourceState {
    /// Usable.
    Ready,
    /// Needs a key the user has not entered.
    NeedsKey,
    /// The provider rejected the entered key.
    KeyRejected,
    /// The last request failed (network down, provider error, rate limited).
    Unavailable {
        /// Plain-language reason.
        message: String,
    },
}

/// Status of one online source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    /// The source.
    pub provider: ProviderId,
    /// Its state.
    pub state: SourceState,
    /// Whether a user key is stored for it (always false for sources without keys).
    pub has_key: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_owner_decisions() {
        let s = Settings::default();
        assert_eq!(s.speech_model, SpeechModel::Accurate);
        assert_eq!(s.language, "en");
        assert!(s.sample_long_files);
        assert!(s.check_updates_automatically);
        assert_eq!(s.save_mode, SaveModeKind::RenameInPlace);
        assert_eq!(s.naming, NamingScheme::JellyfinPlex);
    }

    #[test]
    fn missing_fields_take_defaults_so_old_settings_files_still_load() {
        let s: Settings = serde_json::from_str(r#"{"speechModel":"fast"}"#).unwrap();
        assert_eq!(s.speech_model, SpeechModel::Fast);
        assert_eq!(s.language, "en");
    }
}
