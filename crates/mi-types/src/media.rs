//! Files on disk and what probing them reveals.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Stable identifier of a file within one scan: the file's path relative to the scanned folder,
/// using `/` separators on every OS (for example `title_t03.mkv` or `VIDEO_TS/VTS_01_1.VOB`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
pub struct FileId(pub String);

/// One video file found in the scanned folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MediaFile {
    /// Identifier, unique within the scan.
    pub id: FileId,
    /// Absolute path.
    pub path: PathBuf,
    /// File name with extension, for display.
    pub file_name: String,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Probe result; `None` when the file could not be probed (a [`ScanWarning`] says why).
    pub probe: Option<Probe>,
    /// What the scan decided this file is.
    pub role: FileRole,
}

/// What the scan decided a file is, before any identification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FileRole {
    /// A file to identify as an episode (or an extra).
    Candidate,
    /// The disc's "play all" title: its duration is close to the sum of the others. Used as an
    /// answer key for disc order and never renamed.
    PlayAll,
    /// Not identified: unreadable, no audio, or too short to contain an episode.
    Ignored,
}

/// The result of probing a file with ffprobe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    /// Container duration in seconds.
    pub duration_s: f64,
    /// Container format name as ffprobe reports it (for example `matroska,webm`).
    pub container: String,
    /// The first video stream, if any.
    pub video: Option<VideoInfo>,
    /// Audio streams in file order.
    pub audio_streams: Vec<AudioStream>,
    /// Subtitle streams in file order.
    pub subtitle_streams: Vec<SubtitleStream>,
    /// Chapters in time order; empty when the file has none.
    pub chapters: Vec<Chapter>,
}

/// Picture size of a video stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VideoInfo {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// One audio stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioStream {
    /// Absolute stream index in the container (ffprobe `index`).
    pub index: u32,
    /// Codec name (for example `ac3`).
    pub codec: String,
    /// Channel count.
    pub channels: u32,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// ISO 639-2 language tag, when the container has one.
    pub language: Option<String>,
    /// Whether the container marks it as the default stream.
    pub is_default: bool,
}

/// One subtitle stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleStream {
    /// Absolute stream index in the container.
    pub index: u32,
    /// Codec name (for example `subrip`, `ass`, `mov_text`, `dvd_subtitle`).
    pub codec: String,
    /// ISO 639-2 language tag, when the container has one.
    pub language: Option<String>,
    /// Stream title, when the container has one.
    pub title: Option<String>,
    /// True for text codecs whose dialogue can be read directly; false for bitmap subtitles
    /// (DVD/Blu-ray), which would need OCR and are not used.
    pub is_text: bool,
}

/// One chapter of a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    /// Zero-based position in the chapter list.
    pub index: u32,
    /// Start time in seconds.
    pub start_s: f64,
    /// End time in seconds.
    pub end_s: f64,
    /// Chapter title, when present.
    pub title: Option<String>,
}

/// Everything the Confirm show screen needs about a scanned folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    /// The folder that was scanned.
    pub folder: PathBuf,
    /// Every video file found, sorted by [`FileId`].
    pub files: Vec<MediaFile>,
    /// The detected play-all title, if any.
    pub play_all: Option<PlayAllInfo>,
    /// Number of [`FileRole::Candidate`] files.
    pub candidate_count: u32,
    /// Show name guessed from the folder or disc name, used as the initial search text.
    pub show_guess: Option<String>,
    /// Problems the user should see before identifying.
    pub warnings: Vec<ScanWarning>,
}

/// The detected play-all title.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlayAllInfo {
    /// The play-all file.
    pub file_id: FileId,
    /// Its duration in seconds.
    pub duration_s: f64,
    /// Number of chapters it has.
    pub chapter_count: u32,
    /// Sum of the candidate files' durations in seconds, for comparison with `duration_s`.
    pub candidates_total_s: f64,
}

/// A problem found while scanning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ScanWarning {
    /// The play-all has more chapters than short files were found. MakeMKV skips titles shorter
    /// than its minimum length (120 s by default), so some episodes were probably not ripped.
    MissingShortTitles {
        /// Chapters in the play-all.
        chapters: u32,
        /// Candidate files found.
        short_files: u32,
    },
    /// A file could not be probed.
    Unreadable {
        /// The file.
        file_id: FileId,
        /// Why, in plain words.
        reason: String,
    },
    /// A file has no audio stream, so it cannot be listened to.
    NoAudio {
        /// The file.
        file_id: FileId,
    },
}
