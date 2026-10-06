//! Speech recognition for Media Identifier, running whisper.cpp locally through `whisper-rs`.
//!
//! - [`catalog`]: the pinned model files (size, SHA-256, revision-pinned URL).
//! - [`store`]: resumable, verified model downloads into the app's data folder.
//! - [`sampling`]: which parts of a file to transcribe, and when to use voice activity detection.
//! - [`engine`]: the [`Transcriber`] trait and its whisper.cpp implementation.
//! - [`filter`]: marking segments the model invents over music or silence.
//! - [`eta`]: transcription speed and time estimates.
//! - [`add_window`]: collecting the segments of each window into one [`Transcript`].
//!
//! Audio never leaves the computer; only model files are downloaded.

pub mod catalog;
pub mod engine;
pub mod eta;
pub mod filter;
pub mod sampling;
pub mod store;
mod vad;

use mi_types::{SampleWindow, Segment, Transcript};

pub use catalog::{MODEL_REVISION, PinnedFile, VAD_REVISION, model_info, vad_model_file};
pub use engine::{
    DecodeOptions, SAMPLE_RATE, Transcriber, WhisperTranscriber, missing_processor_features,
    whisper_cpp_version,
};
pub use eta::{SpeedEstimator, audio_cost_seconds};
pub use filter::{HallucinationFilter, compression_ratio};
pub use sampling::{SamplingPolicy, escalation_windows, merge_windows, plan_windows, use_vad};
pub use store::ModelStore;

/// Errors from this crate.
#[derive(Debug, thiserror::Error)]
pub enum TranscribeError {
    /// The model file is not downloaded.
    #[error("speech model {0} is not downloaded")]
    ModelMissing(String),
    /// A finished download did not match its pinned SHA-256; the file was deleted.
    #[error("speech model {file} failed verification (expected {expected}, got {actual})")]
    ChecksumMismatch {
        /// File name.
        file: String,
        /// Pinned digest.
        expected: String,
        /// Digest of the downloaded bytes.
        actual: String,
    },
    /// Downloading failed.
    #[error("download failed: {0}")]
    Download(String),
    /// whisper.cpp failed to load or run.
    #[error("speech recognition failed: {0}")]
    Engine(String),
    /// The processor lacks instructions whisper.cpp was compiled to use (listed); running it
    /// would crash the app.
    #[error("this computer's processor lacks {0}, which speech recognition needs")]
    UnsupportedProcessor(String),
    /// A file system error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The operation was cancelled.
    #[error("cancelled")]
    Cancelled,
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, TranscribeError>;

/// Adds the segments of a newly transcribed `window` to `transcript`.
///
/// Windows are merged and kept in time order; segments are sorted by start time; then `filter`
/// is applied to all segments, so a line repeated across the boundary of two windows is marked
/// like any other back-to-back repeat. Use it for the first window and for every escalation
/// window alike.
pub fn add_window(
    transcript: &mut Transcript,
    window: SampleWindow,
    segments: Vec<Segment>,
    filter: &HallucinationFilter,
) {
    let mut windows = std::mem::take(&mut transcript.windows);
    windows.push(window);
    transcript.windows = merge_windows(&windows);
    transcript.segments.extend(segments);
    transcript.segments.sort_by(|a, b| {
        a.start_s
            .total_cmp(&b.start_s)
            .then(a.end_s.total_cmp(&b.end_s))
    });
    filter.apply(&mut transcript.segments);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_types::{FileId, FilterReason, SpeechModel};

    fn seg(start_s: f64, text: &str) -> Segment {
        Segment {
            start_s,
            end_s: start_s + 2.0,
            text: text.into(),
            avg_logprob: -0.2,
            no_speech_prob: 0.01,
            filtered: None,
        }
    }

    #[test]
    fn windows_are_collected_in_time_order_and_filtered_across_boundaries() {
        let mut t = Transcript {
            file_id: FileId("a.mkv".into()),
            model: SpeechModel::Fast,
            language: "en".into(),
            windows: Vec::new(),
            segments: Vec::new(),
        };
        let filter = HallucinationFilter::default();
        add_window(
            &mut t,
            SampleWindow {
                start_s: 200.0,
                end_s: 300.0,
            },
            vec![
                seg(201.0, "Lolly, lolly, lolly, get your adverbs here"),
                seg(290.0, "Thank you."),
            ],
            &filter,
        );
        add_window(
            &mut t,
            SampleWindow {
                start_s: 100.0,
                end_s: 200.0,
            },
            vec![
                seg(150.0, "Hello"),
                seg(197.0, "lolly lolly lolly get your adverbs here"),
            ],
            &filter,
        );
        assert_eq!(
            t.windows,
            vec![SampleWindow {
                start_s: 100.0,
                end_s: 300.0
            }]
        );
        let starts: Vec<f64> = t.segments.iter().map(|s| s.start_s).collect();
        assert_eq!(starts, vec![150.0, 197.0, 201.0, 290.0]);
        let marks: Vec<_> = t.segments.iter().map(|s| s.filtered).collect();
        assert_eq!(
            marks,
            vec![
                None,
                None,
                Some(FilterReason::Repeated),
                Some(FilterReason::KnownHallucination)
            ]
        );
        assert_eq!(
            t.matching_text(),
            "Hello lolly lolly lolly get your adverbs here"
        );
    }
}
