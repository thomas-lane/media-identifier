//! Locating short files inside the play-all by audio, and deriving disc order.

use mi_types::{Chapter, FileId, PlayAllPosition};

/// A compact audio fingerprint computed from decoded PCM in Rust (no external tool).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fingerprint {
    /// Fingerprint frames (implementation-defined features).
    pub frames: Vec<u32>,
    /// Seconds per frame.
    pub frame_s: f64,
}

impl Fingerprint {
    /// Computes the fingerprint of mono PCM at `sample_rate` Hz.
    pub fn from_pcm(samples: &[f32], sample_rate: u32) -> Self {
        let _ = (samples, sample_rate);
        todo!("match module: audio fingerprint")
    }
}

/// Where a short file was found inside the play-all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Alignment {
    /// Offset of the file's start in the play-all, seconds.
    pub offset_s: f64,
    /// Length of the located range, seconds.
    pub length_s: f64,
    /// Strength in `0.0..=1.0`; below the configured minimum the file counts as not found.
    pub score: f32,
}

/// Finds `needle` (a short file) inside `haystack` (the play-all).
pub fn locate(needle: &Fingerprint, haystack: &Fingerprint) -> Option<Alignment> {
    let _ = (needle, haystack);
    todo!("match module: alignment search")
}

/// The disc order derived from alignments.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DiscOrder {
    /// Located files with their positions, sorted by start time.
    pub positions: Vec<(FileId, PlayAllPosition)>,
    /// False when the order is unusable: located ranges overlap, too few files were found, or the
    /// play-all appears shuffled relative to its chapters. An untrustworthy order is ignored, and
    /// files get an `EvidenceNote::PlayAllIgnored`.
    pub trustworthy: bool,
}

/// Builds the disc order from each file's alignment and the play-all's chapters (used to report
/// the chapter number of each position).
pub fn derive_disc_order(
    alignments: &[(FileId, Option<Alignment>)],
    chapters: &[Chapter],
) -> DiscOrder {
    let _ = (alignments, chapters);
    todo!("match module: disc order")
}
