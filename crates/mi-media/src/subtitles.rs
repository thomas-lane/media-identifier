//! Reading text subtitle streams embedded in a file.

use std::ffi::OsString;
use std::path::Path;

use mi_types::CancelFlag;

use crate::{Sidecars, Tool};

/// Extracts one text subtitle stream as SubRip (`.srt`) text.
///
/// Runs `ffmpeg -nostdin -hide_banner -v error -i <path> -map 0:<stream_index> -c:s srt -f srt -`.
/// ffmpeg converts every text format it can decode (SubRip, ASS/SSA, WebVTT, MP4 `mov_text`,
/// plain text) into SubRip, so callers handle one format; ASS styling becomes SubRip tags such as
/// `<i>`. Only streams with `is_text` can be converted; a bitmap stream makes ffmpeg fail, which is
/// returned as [`crate::MediaError::ToolFailed`]. Turning the SRT into plain dialogue lines is
/// `mi-sources`' job (`mi_sources::text::srt_to_dialogue`), which also normalises downloaded
/// subtitles the same way. Invalid UTF-8 is replaced rather than rejected.
pub fn extract_text_subtitles(
    sidecars: &Sidecars,
    path: &Path,
    stream_index: u32,
    cancel: &CancelFlag,
) -> crate::Result<String> {
    let mut args: Vec<OsString> = ["-nostdin", "-hide_banner", "-v", "error", "-i"]
        .iter()
        .map(Into::into)
        .collect();
    args.push(path.as_os_str().to_owned());
    for a in [
        "-map".to_owned(),
        format!("0:{stream_index}"),
        "-c:s".into(),
        "srt".into(),
        "-f".into(),
        "srt".into(),
        "-".into(),
    ] {
        args.push(a.into());
    }
    let bytes = crate::run::output(sidecars, Tool::Ffmpeg, path, args, cancel)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
