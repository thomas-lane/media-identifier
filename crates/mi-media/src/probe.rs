//! Reading durations, streams and chapters with ffprobe.

use std::path::Path;

use mi_types::{CancelFlag, Probe};

use crate::Sidecars;

/// Probes one file.
///
/// Runs `ffprobe -v error -print_format json -show_format -show_streams -show_chapters <path>`
/// and converts the result. `duration_s` is the container duration; when the container lacks one,
/// the longest stream duration is used. Subtitle streams are marked `is_text` for the text codecs
/// `subrip`, `ass`, `ssa`, `webvtt`, `mov_text` and `text`.
///
/// Errors: [`crate::MediaError::ToolFailed`] when ffprobe exits non-zero,
/// [`crate::MediaError::BadProbe`] when its JSON lacks a duration, `Cancelled` when `cancel` is set.
pub fn probe(sidecars: &Sidecars, path: &Path, cancel: &CancelFlag) -> crate::Result<Probe> {
    let _ = (sidecars, path, cancel);
    todo!("media module: run ffprobe and parse its JSON")
}
