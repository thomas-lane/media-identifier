//! Speech recognition for Media Identifier, running whisper.cpp locally through `whisper-rs`.
//!
//! - [`catalog`]: the two pinned model files (size, SHA-256, revision-pinned URL).
//! - [`store`]: resumable, verified model downloads into the app's data folder.
//! - [`sampling`]: which parts of a file to transcribe.
//! - [`filter`]: dropping segments the model invents over music or silence.
//! - [`engine`]: the [`Transcriber`] trait and its whisper.cpp implementation.
//!
//! Audio never leaves the computer; only model files are downloaded.
//!
//! Owner: transcribe module (see `docs/architecture.md`).

pub mod catalog;
pub mod engine;
pub mod filter;
pub mod sampling;
pub mod store;

pub use catalog::{MODEL_REVISION, model_info};
pub use engine::{DecodeOptions, Transcriber, WhisperTranscriber, whisper_cpp_version};
pub use filter::{HallucinationFilter, compression_ratio};
pub use sampling::{SamplingPolicy, escalation_windows, plan_windows};
pub use store::ModelStore;

/// Errors from this crate.
#[derive(Debug, thiserror::Error)]
pub enum TranscribeError {
    /// The model file is not downloaded.
    #[error("speech model {0} is not downloaded")]
    ModelMissing(String),
    /// A finished download did not match its pinned SHA-256; the file was deleted.
    #[error("speech model {file} failed verification (expected {expected}, got {actual})")]
    ChecksumMismatch {
        /// File name.
        file: String,
        /// Pinned digest.
        expected: String,
        /// Digest of the downloaded bytes.
        actual: String,
    },
    /// Downloading failed.
    #[error("download failed: {0}")]
    Download(String),
    /// whisper.cpp failed to load or run.
    #[error("speech recognition failed: {0}")]
    Engine(String),
    /// A file system error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The operation was cancelled.
    #[error("cancelled")]
    Cancelled,
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, TranscribeError>;
