//! Match results and the evidence shown on the Review screen.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::catalog::EpisodeKey;
use crate::media::FileId;

/// Per-signal similarity between one file and one episode, each in `0.0..=1.0`.
///
/// A signal is `None` when it could not be measured (no reference text, no play-all, no listed
/// runtime); missing signals are left out of the combined score rather than counted as zero, so a
/// file is never penalised for data the provider lacks.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Signals {
    /// Fuzzy similarity of what was heard to the episode's reference text.
    pub dialogue: Option<f32>,
    /// How well the episode title occurs inside the transcript.
    pub title_hook: Option<f32>,
    /// How well the file's duration fits the episode's listed runtime.
    pub duration: Option<f32>,
    /// How well the file's position in the play-all fits this episode's place in the order.
    pub disc_order: Option<f32>,
}

/// A piece of a quote; `matched` pieces are highlighted as overlapping with the other side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct QuotePart {
    /// The text of this piece.
    pub text: String,
    /// True when this piece also occurs in the other quote.
    pub matched: bool,
}

/// The file's position inside the play-all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlayAllPosition {
    /// Zero-based chapter of the play-all the file was found in, when chapters exist.
    pub chapter: Option<u32>,
    /// Start of the located audio inside the play-all, seconds.
    pub start_s: f64,
    /// End of the located audio, seconds.
    pub end_s: f64,
    /// Zero-based rank of this file among all located files, in play-all order.
    pub order_index: u32,
    /// Alignment strength in `0.0..=1.0`.
    pub alignment_score: f32,
}

/// A short, typed explanation the UI turns into a sentence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EvidenceNote {
    /// Mostly music, so there was little dialogue to compare.
    MostlyMusic,
    /// No speech was heard.
    NoSpeech,
    /// No reference text was available for this episode.
    NoReferenceText,
    /// The file's position on the disc agrees with this episode.
    DiscOrderAgrees {
        /// Chapter of the play-all.
        chapter: Option<u32>,
    },
    /// The file's position on the disc disagrees with this episode.
    DiscOrderDisagrees,
    /// The play-all was found but its order did not look trustworthy, so it was ignored.
    PlayAllIgnored,
    /// The episode title was heard in the file.
    TitleHeard,
    /// The runtime differs a lot from the listed runtime.
    LengthMismatch,
    /// Only samples of a long file were transcribed.
    Sampled {
        /// Number of windows transcribed.
        windows: u32,
    },
}

/// The evidence for one file/episode pair.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    /// The per-signal similarities.
    pub signals: Signals,
    /// Excerpt of what was heard around the best overlap.
    pub heard: Vec<QuotePart>,
    /// Excerpt of the reference text around the best overlap.
    pub reference: Vec<QuotePart>,
    /// Where the file was found in the play-all.
    pub play_all_position: Option<PlayAllPosition>,
    /// Explanations, most important first.
    pub notes: Vec<EvidenceNote>,
}

/// One episode a file could be.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// The episode.
    pub episode: EpisodeKey,
    /// Episode title, for display.
    pub title: String,
    /// Combined score in `0.0..=1.0`.
    pub score: f32,
    /// Why.
    pub evidence: Evidence,
}

/// How sure the app is about a file's suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    /// The best candidate leads the runner-up clearly; pre-approved.
    Confident,
    /// The lead is too small; the user should check.
    Check,
    /// No episode matches well; probably a bonus feature.
    Extra,
    /// The play-all title (answer key); never renamed.
    PlayAll,
}

/// The confidence of a file's suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Confidence {
    /// Combined score of the suggested episode.
    pub score: f32,
    /// Score of the suggestion minus the runner-up's (the runner-up includes "no episode").
    pub margin: f32,
    /// The resulting verdict.
    pub verdict: Verdict,
}

/// What the app suggests for a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Suggestion {
    /// This episode.
    Episode {
        /// The episode.
        episode: EpisodeKey,
    },
    /// Not an episode (an extra).
    NotAnEpisode,
    /// The play-all title.
    PlayAll,
}

/// The identification result for one file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileMatch {
    /// The file.
    pub file_id: FileId,
    /// The suggestion from the best overall assignment.
    pub suggestion: Suggestion,
    /// Confidence of the suggestion.
    pub confidence: Confidence,
    /// Alternatives, best first, including the suggested episode; at most 5.
    pub candidates: Vec<Candidate>,
}

/// The user's decision for a file on the Review screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ReviewDecision {
    /// Not decided yet (a `Check` file before the user approves it).
    Pending,
    /// Rename to this episode.
    Approved {
        /// The episode.
        episode: EpisodeKey,
    },
    /// Not an episode; left where it is.
    NotAnEpisode,
    /// Leave this file alone.
    Skip,
}
