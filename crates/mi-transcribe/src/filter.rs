//! Removing text the model invents.

use mi_types::Segment;

/// Marks segments that should not be used for matching (sets [`Segment::filtered`]).
///
/// Whisper models invent fixed phrases over music and silence ("Thank you.", "Subtitles by ...",
/// "Please subscribe") and sometimes loop the same line. Such text would match every episode
/// equally and hide the real signal, so it is dropped before scoring and kept only for display.
#[derive(Debug, Clone)]
pub struct HallucinationFilter {
    /// Phrases (compared case-insensitively after removing punctuation) that are dropped when a
    /// segment consists only of them.
    pub phrases: Vec<String>,
    /// Segments whose gzip compression ratio exceeds this are dropped as looping (whisper.cpp's
    /// own default is 2.4).
    pub max_compression_ratio: f32,
    /// Segments with `no_speech_prob` above this and low `avg_logprob` are dropped.
    pub no_speech_threshold: f32,
}

impl Default for HallucinationFilter {
    fn default() -> Self {
        Self {
            phrases: [
                "thank you",
                "thanks for watching",
                "thank you for watching",
                "subtitles by",
                "please subscribe",
                "you",
            ]
            .map(String::from)
            .to_vec(),
            max_compression_ratio: 2.4,
            no_speech_threshold: 0.6,
        }
    }
}

impl HallucinationFilter {
    /// Marks known phrases, back-to-back repeats, high-compression text and likely non-speech.
    /// Segments are never removed, only marked.
    pub fn apply(&self, segments: &mut [Segment]) {
        let _ = segments;
        todo!("transcribe module: hallucination filter")
    }
}

/// Ratio of UTF-8 length to gzip-compressed length; repetitive text scores high.
pub fn compression_ratio(text: &str) -> f32 {
    let _ = text;
    todo!("transcribe module: gzip compression ratio")
}
