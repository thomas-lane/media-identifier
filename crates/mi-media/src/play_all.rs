//! Detecting the disc's "play all" title.

use mi_types::{MediaFile, PlayAllInfo};

/// Tolerances for recognising a play-all title.
#[derive(Debug, Clone)]
pub struct PlayAllThresholds {
    /// The play-all's duration may differ from the sum of the other files by at most this
    /// fraction of that sum (rips trim a few seconds per title, and some titles are missing).
    pub max_relative_difference: f64,
    /// The play-all must be at least this many times longer than the longest other file.
    pub min_ratio_to_longest: f64,
}

impl Default for PlayAllThresholds {
    fn default() -> Self {
        Self {
            max_relative_difference: 0.25,
            min_ratio_to_longest: 3.0,
        }
    }
}

/// Picks the play-all title among probed files, if one exists.
///
/// The longest file is the play-all when its duration is within `max_relative_difference` of the
/// sum of all other probed files and at least `min_ratio_to_longest` times the next longest file.
/// Chapter count comes from its probe. Returns `None` for fewer than three probed files. This is a
/// heuristic about durations only; whether its order is trustworthy is decided later by audio
/// alignment in `mi-match`.
pub fn detect_play_all(files: &[MediaFile], thresholds: &PlayAllThresholds) -> Option<PlayAllInfo> {
    let _ = (files, thresholds);
    todo!("media module: duration-sum heuristic")
}
