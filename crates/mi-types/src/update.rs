//! App updates.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A newer version found by the updater.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// The new version, for example `1.3.0`.
    pub version: String,
    /// The running version.
    pub current_version: String,
    /// Release notes from `latest.json`, plain text or Markdown.
    pub notes: String,
    /// Publication date (RFC 3339), when given.
    pub date: Option<String>,
}

/// The answer to an update check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UpdateCheck {
    /// A newer version exists (and was not skipped by the user).
    Available {
        /// The update.
        info: UpdateInfo,
    },
    /// The running version is current.
    UpToDate {
        /// The running version.
        current_version: String,
    },
    /// The check failed. Manual checks show "Couldn't check for updates"; background checks
    /// only log it.
    Failed {
        /// Technical reason, for the log.
        message: String,
    },
}

/// Progress of an accepted update, emitted on the `update-event` channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UpdateEvent {
    /// Downloading.
    Downloading {
        /// Bytes so far.
        downloaded: u64,
        /// Total bytes, when the server sent a length.
        total: Option<u64>,
    },
    /// Downloaded and verified; the app relaunches when the user chooses.
    Downloaded {
        /// The version that will run after relaunch.
        version: String,
    },
    /// The download or install failed.
    Failed {
        /// Plain-language reason.
        message: String,
    },
}
