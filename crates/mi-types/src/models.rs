//! Speech model files and their download state.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::transcript::SpeechModel;

/// A downloadable model file, pinned by size and SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// Which model.
    pub model: SpeechModel,
    /// File name on disk and on Hugging Face.
    pub file_name: String,
    /// Exact size in bytes.
    pub size_bytes: u64,
    /// Lowercase hex SHA-256 of the whole file.
    pub sha256: String,
    /// Download URL, pinned to a repository revision.
    pub url: String,
}

/// Where a model's download stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ModelState {
    /// Not on disk and no partial download.
    Missing,
    /// Downloading.
    Downloading {
        /// Bytes on disk so far.
        downloaded: u64,
        /// Total bytes.
        total: u64,
        /// Recent speed in bytes per second, for the time estimate.
        bytes_per_second: f64,
    },
    /// A partial download exists and will resume from where it stopped.
    Paused {
        /// Bytes on disk so far.
        downloaded: u64,
        /// Total bytes.
        total: u64,
    },
    /// Checking the SHA-256 of a finished download.
    Verifying,
    /// Downloaded and verified.
    Ready,
    /// The download failed; it can be retried and resumes.
    Failed {
        /// Plain-language reason.
        message: String,
    },
}

/// A model and its state; also the payload of the `model-download` event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    /// The pinned file.
    pub info: ModelInfo,
    /// Its state.
    pub state: ModelState,
}
