//! What the speech model heard.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::media::FileId;

/// Which speech model to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SpeechModel {
    /// `ggml-small.en-q5_1` (about 190 MB): faster, English only.
    Fast,
    /// `ggml-large-v3-turbo-q5_0` (about 550 MB): the default.
    #[default]
    Accurate,
}

/// A time range of a file that was transcribed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SampleWindow {
    /// Start in seconds from the beginning of the file.
    pub start_s: f64,
    /// End in seconds.
    pub end_s: f64,
}

/// Why a segment was dropped from matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FilterReason {
    /// A phrase the model is known to invent over silence or music ("Thank you.",
    /// "Subtitles by ...").
    KnownHallucination,
    /// The same line repeated back to back.
    Repeated,
    /// Text that compresses too well (gzip ratio above the threshold), a sign of looping output.
    HighCompressionRatio,
    /// The model itself rated the segment as probably not speech.
    NoSpeech,
}

/// One recognised segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    /// Start in seconds from the beginning of the file (not the window).
    pub start_s: f64,
    /// End in seconds from the beginning of the file.
    pub end_s: f64,
    /// Recognised text.
    pub text: String,
    /// Average token log-probability reported by the model.
    pub avg_logprob: f32,
    /// The model's probability that the segment is not speech.
    pub no_speech_prob: f32,
    /// Set when the hallucination filter dropped this segment; such segments are kept for display
    /// but excluded from matching.
    pub filtered: Option<FilterReason>,
}

/// Everything heard in one file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    /// The file.
    pub file_id: FileId,
    /// The model used.
    pub model: SpeechModel,
    /// Language decoded (ISO 639-1).
    pub language: String,
    /// The windows that were transcribed, in time order. One window covering the whole file when
    /// it was transcribed whole.
    pub windows: Vec<SampleWindow>,
    /// Segments in time order.
    pub segments: Vec<Segment>,
}

impl Transcript {
    /// The text used for matching: unfiltered segments joined with single spaces.
    pub fn matching_text(&self) -> String {
        self.segments
            .iter()
            .filter(|s| s.filtered.is_none())
            .map(|s| s.text.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, filtered: Option<FilterReason>) -> Segment {
        Segment {
            start_s: 0.0,
            end_s: 1.0,
            text: text.into(),
            avg_logprob: -0.2,
            no_speech_prob: 0.0,
            filtered,
        }
    }

    #[test]
    fn matching_text_skips_filtered_and_blank_segments() {
        let t = Transcript {
            file_id: FileId("a.mkv".into()),
            model: SpeechModel::Accurate,
            language: "en".into(),
            windows: vec![],
            segments: vec![
                seg(" conjunction junction ", None),
                seg("Thank you.", Some(FilterReason::KnownHallucination)),
                seg("  ", None),
                seg("what's your function", None),
            ],
        };
        assert_eq!(
            t.matching_text(),
            "conjunction junction what's your function"
        );
    }
}
