//! Reading durations, streams and chapters with ffprobe.

use std::collections::HashMap;
use std::path::Path;

use mi_types::{AudioStream, CancelFlag, Chapter, Probe, SubtitleStream, VideoInfo};
use serde::Deserialize;

use crate::{MediaError, Sidecars, Tool};

/// Subtitle codecs whose dialogue is stored as text (ffprobe `codec_name`). Every other subtitle
/// codec (`dvd_subtitle`, `hdmv_pgs_subtitle`, `dvb_subtitle`, ...) stores pictures of the text,
/// which would need OCR.
pub const TEXT_SUBTITLE_CODECS: &[&str] = &["subrip", "ass", "ssa", "webvtt", "mov_text", "text"];

/// Probes one file.
///
/// Runs `ffprobe -v error -print_format json -show_format -show_streams -show_chapters <path>`
/// and converts the result with [`parse_probe`].
///
/// Errors: [`crate::MediaError::ToolFailed`] when ffprobe exits non-zero,
/// [`crate::MediaError::BadProbe`] when its JSON lacks a duration, `Cancelled` when `cancel` is set.
pub fn probe(sidecars: &Sidecars, path: &Path, cancel: &CancelFlag) -> crate::Result<Probe> {
    let mut args: Vec<std::ffi::OsString> = [
        "-v",
        "error",
        "-print_format",
        "json",
        "-show_format",
        "-show_streams",
        "-show_chapters",
    ]
    .iter()
    .map(Into::into)
    .collect();
    args.push(path.as_os_str().to_owned());
    let json = crate::run::output(sidecars, Tool::Ffprobe, path, args, cancel)?;
    parse_probe(&json, path)
}

/// Converts ffprobe's JSON (`-show_format -show_streams -show_chapters`) into a [`Probe`].
///
/// - `duration_s` is the container duration. When the container has none, the longest stream
///   duration is used (Matroska stores it in a `DURATION` tag), then the end of the last chapter.
///   A file with none of these is rejected with `BadProbe`, because every later step needs a
///   length.
/// - `video` is the first video stream that is not cover art (`attached_pic`) and has a size.
/// - A language tag of `und` (undetermined) is treated as absent.
/// - Subtitle streams are `is_text` for the codecs in [`TEXT_SUBTITLE_CODECS`].
/// - Chapters are sorted by start time and renumbered from zero; blank titles become `None`.
pub fn parse_probe(json: &[u8], path: &Path) -> crate::Result<Probe> {
    let bad = |message: String| MediaError::BadProbe {
        path: path.to_path_buf(),
        message,
    };
    let raw: RawProbe = serde_json::from_slice(json).map_err(|e| bad(e.to_string()))?;

    let mut video = None;
    let mut audio_streams = Vec::new();
    let mut subtitle_streams = Vec::new();
    let mut longest_stream = None::<f64>;
    for stream in &raw.streams {
        if let Some(d) = stream.duration_s() {
            longest_stream = Some(longest_stream.map_or(d, |l: f64| l.max(d)));
        }
        match stream.codec_type.as_deref() {
            Some("video") => {
                let is_cover = stream.disposition.get("attached_pic").copied() == Some(1);
                if video.is_none()
                    && !is_cover
                    && let (Some(width), Some(height)) = (stream.width, stream.height)
                    && width > 0
                    && height > 0
                {
                    video = Some(VideoInfo { width, height });
                }
            }
            Some("audio") => audio_streams.push(AudioStream {
                index: stream.index,
                codec: stream.codec_name.clone().unwrap_or_default(),
                channels: stream.channels.unwrap_or(0),
                sample_rate: stream
                    .sample_rate
                    .as_deref()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
                language: stream.language(),
                is_default: stream.disposition.get("default").copied() == Some(1),
            }),
            Some("subtitle") => {
                let codec = stream.codec_name.clone().unwrap_or_default();
                subtitle_streams.push(SubtitleStream {
                    index: stream.index,
                    is_text: TEXT_SUBTITLE_CODECS.contains(&codec.as_str()),
                    codec,
                    language: stream.language(),
                    title: stream.tag("title").map(str::to_owned),
                });
            }
            _ => {}
        }
    }

    let mut chapters: Vec<Chapter> = raw
        .chapters
        .iter()
        .filter_map(|c| {
            let start_s = parse_seconds(c.start_time.as_deref()?)?;
            let end_s = parse_seconds(c.end_time.as_deref()?)?;
            Some(Chapter {
                index: 0,
                start_s,
                end_s,
                title: c
                    .tags
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("title"))
                    .map(|(_, t)| t.trim().to_owned())
                    .filter(|t| !t.is_empty()),
            })
        })
        .collect();
    chapters.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
    for (i, chapter) in chapters.iter_mut().enumerate() {
        chapter.index = i as u32;
    }

    let format = raw.format.unwrap_or_default();
    let duration_s = format
        .duration
        .as_deref()
        .and_then(parse_seconds)
        .filter(|d| *d > 0.0)
        .or(longest_stream)
        .or_else(|| chapters.last().map(|c| c.end_s))
        .filter(|d| *d > 0.0)
        .ok_or_else(|| bad("no duration in the container, its streams or its chapters".into()))?;

    Ok(Probe {
        duration_s,
        container: format.format_name.unwrap_or_default(),
        video,
        audio_streams,
        subtitle_streams,
        chapters,
    })
}

/// Parses ffprobe seconds (`"191.000000"`) or a Matroska `DURATION` tag (`"00:03:11.500000000"`).
fn parse_seconds(text: &str) -> Option<f64> {
    let text = text.trim();
    let value = if text.contains(':') {
        let mut total = 0.0;
        for part in text.split(':') {
            total = total * 60.0 + part.parse::<f64>().ok()?;
        }
        total
    } else {
        text.parse::<f64>().ok()?
    };
    value.is_finite().then_some(value).filter(|v| *v >= 0.0)
}

#[derive(Debug, Default, Deserialize)]
struct RawProbe {
    #[serde(default)]
    streams: Vec<RawStream>,
    #[serde(default)]
    chapters: Vec<RawChapter>,
    format: Option<RawFormat>,
}

#[derive(Debug, Default, Deserialize)]
struct RawStream {
    index: u32,
    codec_name: Option<String>,
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    duration: Option<String>,
    #[serde(default)]
    disposition: HashMap<String, i64>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

impl RawStream {
    /// Tag lookup ignoring case: Matroska writes `DURATION`, MP4 writes `language`.
    fn tag(&self, key: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    fn language(&self) -> Option<String> {
        self.tag("language")
            .map(|l| l.trim().to_ascii_lowercase())
            .filter(|l| !l.is_empty() && l != "und")
    }

    fn duration_s(&self) -> Option<f64> {
        self.duration
            .as_deref()
            .and_then(parse_seconds)
            .or_else(|| self.tag("duration").and_then(parse_seconds))
            .filter(|d| *d > 0.0)
    }
}

#[derive(Debug, Default, Deserialize)]
struct RawChapter {
    start_time: Option<String>,
    end_time: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawFormat {
    format_name: Option<String>,
    duration: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> crate::Result<Probe> {
        parse_probe(json.as_bytes(), Path::new("/x/title_t00.mkv"))
    }

    /// The shape of ffprobe output for a MakeMKV file, trimmed to the fields that matter.
    const MAKEMKV: &str = r#"{
      "streams": [
        {"index": 0, "codec_name": "mpeg2video", "codec_type": "video", "width": 720, "height": 480,
         "disposition": {"default": 1, "attached_pic": 0}, "tags": {"DURATION": "00:03:12.045000000"}},
        {"index": 1, "codec_name": "ac3", "codec_type": "audio", "sample_rate": "48000", "channels": 2,
         "disposition": {"default": 1}, "tags": {"language": "eng", "DURATION": "00:03:12.032000000"}},
        {"index": 2, "codec_name": "ac3", "codec_type": "audio", "sample_rate": "48000", "channels": 2,
         "disposition": {"default": 0}, "tags": {"language": "spa"}},
        {"index": 3, "codec_name": "dvd_subtitle", "codec_type": "subtitle",
         "disposition": {"default": 0}, "tags": {"language": "eng"}},
        {"index": 4, "codec_name": "subrip", "codec_type": "subtitle",
         "disposition": {"default": 0}, "tags": {"language": "und", "title": "SDH"}}
      ],
      "chapters": [
        {"id": 1, "start_time": "95.500000", "end_time": "192.045000", "tags": {"title": "Chapter 02"}},
        {"id": 0, "start_time": "0.000000", "end_time": "95.500000", "tags": {"title": " "}}
      ],
      "format": {"format_name": "matroska,webm", "duration": "192.045000"}
    }"#;

    #[test]
    fn reads_duration_streams_and_chapters() {
        let p = parse(MAKEMKV).unwrap();
        assert_eq!(p.duration_s, 192.045);
        assert_eq!(p.container, "matroska,webm");
        assert_eq!(
            p.video,
            Some(VideoInfo {
                width: 720,
                height: 480
            })
        );
        assert_eq!(p.audio_streams.len(), 2);
        let a = &p.audio_streams[0];
        assert_eq!(
            (a.index, a.codec.as_str(), a.channels, a.sample_rate),
            (1, "ac3", 2, 48_000)
        );
        assert_eq!(a.language.as_deref(), Some("eng"));
        assert!(a.is_default);
        assert!(!p.audio_streams[1].is_default);
    }

    #[test]
    fn marks_text_subtitles_and_drops_und_language() {
        let p = parse(MAKEMKV).unwrap();
        let subs = &p.subtitle_streams;
        assert_eq!(subs.len(), 2);
        assert!(!subs[0].is_text, "dvd_subtitle is a bitmap format");
        assert!(subs[1].is_text);
        assert_eq!(subs[1].language, None);
        assert_eq!(subs[1].title.as_deref(), Some("SDH"));
    }

    #[test]
    fn sorts_and_renumbers_chapters_and_drops_blank_titles() {
        let p = parse(MAKEMKV).unwrap();
        assert_eq!(p.chapters.len(), 2);
        assert_eq!(p.chapters[0].index, 0);
        assert_eq!(p.chapters[0].start_s, 0.0);
        assert_eq!(p.chapters[0].title, None);
        assert_eq!(p.chapters[1].index, 1);
        assert_eq!(p.chapters[1].title.as_deref(), Some("Chapter 02"));
    }

    #[test]
    fn falls_back_to_stream_duration_tag_then_chapters() {
        let from_tag = parse(
            r#"{"streams":[{"index":0,"codec_type":"audio","tags":{"DURATION":"01:02:03.500000000"}}],
                "format":{"format_name":"matroska,webm"}}"#,
        )
        .unwrap();
        assert_eq!(from_tag.duration_s, 3723.5);

        let from_chapters = parse(
            r#"{"streams":[],"chapters":[{"start_time":"0.0","end_time":"60.0"},{"start_time":"60.0","end_time":"125.0"}],
                "format":{"format_name":"mpeg","duration":"N/A"}}"#,
        )
        .unwrap();
        assert_eq!(from_chapters.duration_s, 125.0);
    }

    #[test]
    fn missing_duration_is_bad_probe() {
        let err = parse(r#"{"streams":[],"format":{"format_name":"avi"}}"#).unwrap_err();
        assert!(matches!(err, MediaError::BadProbe { .. }), "{err:?}");
        let err = parse("not json").unwrap_err();
        assert!(matches!(err, MediaError::BadProbe { .. }), "{err:?}");
    }

    #[test]
    fn cover_art_is_not_the_video_stream() {
        let p = parse(
            r#"{"streams":[
                {"index":0,"codec_type":"video","width":600,"height":600,"disposition":{"attached_pic":1}},
                {"index":1,"codec_type":"video","width":1920,"height":1080,"disposition":{"attached_pic":0}}],
                "format":{"duration":"10.0"}}"#,
        )
        .unwrap();
        assert_eq!(
            p.video,
            Some(VideoInfo {
                width: 1920,
                height: 1080
            })
        );
    }

    #[test]
    fn parses_both_time_notations() {
        assert_eq!(parse_seconds("191.5"), Some(191.5));
        assert_eq!(parse_seconds("00:03:11.500000000"), Some(191.5));
        assert_eq!(parse_seconds("N/A"), None);
        assert_eq!(parse_seconds("-1"), None);
    }
}
