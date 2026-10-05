//! Finding and classifying the video files in a folder.

use std::path::Path;

use mi_types::{CancelFlag, ScanSummary};

use crate::Sidecars;

/// File extensions (lowercase, without the dot) treated as video files.
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "mov", "avi", "ts", "m2ts", "mts", "mpg", "mpeg", "vob",
];

/// Scan options.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Descend into subfolders (a `VIDEO_TS` folder is always entered).
    pub recursive: bool,
    /// Files shorter than this many seconds get [`mi_types::FileRole::Ignored`] (menus, logos).
    pub min_duration_s: f64,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            recursive: false,
            min_duration_s: 20.0,
        }
    }
}

/// Scans `folder`: lists video files, probes each one, marks the play-all title, guesses the show
/// name from the folder or disc name, and records warnings.
///
/// Contract:
/// - `files` are sorted by `FileId` (relative path with `/`), so order is stable across runs.
/// - Unprobeable files are kept with `probe: None`, role `Ignored`, and a `Unreadable` warning.
/// - The play-all is chosen by [`crate::detect_play_all`]; its role becomes `PlayAll`.
/// - `MissingShortTitles` is added when the play-all has more chapters than there are candidates.
/// - `show_guess` strips disc and rip markers (`D1`, `Disc 2`, `_`, `S03`) from the folder name.
pub fn scan_folder(
    sidecars: &Sidecars,
    folder: &Path,
    options: &ScanOptions,
    cancel: &CancelFlag,
) -> crate::Result<ScanSummary> {
    let _ = (sidecars, folder, options, cancel);
    todo!("media module: list, probe and classify files")
}
