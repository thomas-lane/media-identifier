//! Reading text subtitle streams embedded in a file.

use std::path::Path;

use mi_types::CancelFlag;

use crate::Sidecars;

/// Extracts one text subtitle stream as SubRip (`.srt`) text.
///
/// Runs `ffmpeg -nostdin -v error -i <path> -map 0:<stream_index> -f srt -`. Only streams with
/// `is_text` are supported; bitmap subtitles return [`crate::MediaError::ToolFailed`]. Parsing and
/// normalising the SRT is `mi-sources`' job (`mi_sources::embedded`).
pub fn extract_text_subtitles(
    sidecars: &Sidecars,
    path: &Path,
    stream_index: u32,
    cancel: &CancelFlag,
) -> crate::Result<String> {
    let _ = (sidecars, path, stream_index, cancel);
    todo!("media module: extract subtitle stream as SRT")
}
