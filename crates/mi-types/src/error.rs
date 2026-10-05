//! The error shape returned by every Tauri command.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Broad error category, so the UI can choose wording without parsing messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The request was malformed or refers to something that does not fit (bad path, no job).
    InvalidInput,
    /// A file, job, history entry or show was not found.
    NotFound,
    /// The network or a provider failed.
    Network,
    /// A provider is rate limiting; retry later.
    RateLimited,
    /// A provider rejected the user's API key.
    KeyRejected,
    /// The operation was cancelled.
    Cancelled,
    /// A file system operation failed.
    Io,
    /// ffmpeg or ffprobe is missing or failed.
    MediaTool,
    /// The speech model is missing, corrupt or failed.
    SpeechModel,
    /// The target of a rename or copy already exists.
    Conflict,
    /// Another operation is running (for example a second identification).
    Busy,
    /// Anything else; a bug.
    Internal,
}

/// Error returned by every command: a category and a plain-language message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    /// Category.
    pub code: ErrorCode,
    /// Message suitable for showing to the user.
    pub message: String,
}

impl ApiError {
    /// Creates an error.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}
