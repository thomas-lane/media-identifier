//! Media access for Media Identifier: finding video files, probing them with ffprobe, detecting
//! the play-all title, and decoding audio to 16 kHz mono `f32` PCM with ffmpeg.
//!
//! ffmpeg and ffprobe are separate executables (Tauri sidecars), resolved by [`Sidecars`].
//! Everything here is synchronous and blocking; callers run it on blocking threads. Every
//! long-running function takes a [`CancelFlag`] and kills its child process when cancelled.

pub mod audio;
pub mod play_all;
pub mod probe;
mod run;
pub mod scan;
pub mod sidecar;
pub mod subtitles;

pub use audio::{
    AudioChunk, ExtractOptions, Pcm, SAMPLE_RATE, choose_audio_stream, extract_audio,
    languages_match, stream_audio,
};
pub use play_all::{PlayAllThresholds, detect_play_all};
pub use probe::{TEXT_SUBTITLE_CODECS, parse_probe, probe};
pub use scan::{ScanOptions, VIDEO_EXTENSIONS, guess_show, scan_folder};
pub use sidecar::{SidecarLookup, Sidecars, Tool};
pub use subtitles::extract_text_subtitles;

use mi_types::CancelFlag;

/// Errors from this crate.
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    /// ffmpeg or ffprobe could not be found.
    #[error("{tool} was not found (looked in: {searched})")]
    SidecarMissing {
        /// Which tool.
        tool: Tool,
        /// Places searched, for the log.
        searched: String,
    },
    /// A tool ran but failed.
    #[error("{tool} failed on {path}: {message}")]
    ToolFailed {
        /// Which tool.
        tool: Tool,
        /// The input file.
        path: std::path::PathBuf,
        /// Its stderr tail or exit status.
        message: String,
    },
    /// ffprobe output could not be understood.
    #[error("could not read probe output for {path}: {message}")]
    BadProbe {
        /// The input file.
        path: std::path::PathBuf,
        /// What was wrong.
        message: String,
    },
    /// The file has no audio stream.
    #[error("{0} has no audio")]
    NoAudio(std::path::PathBuf),
    /// A file system error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The operation was cancelled through its [`CancelFlag`].
    #[error("cancelled")]
    Cancelled,
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, MediaError>;

/// Returns `Err(MediaError::Cancelled)` when `cancel` is set.
pub fn check_cancel(cancel: &CancelFlag) -> Result<()> {
    if cancel.is_cancelled() {
        Err(MediaError::Cancelled)
    } else {
        Ok(())
    }
}
