//! Matching files to episodes.
//!
//! Every file is scored against every candidate episode with several independent signals, and the
//! best overall assignment wins. Speech recognition will mishear words, so no signal requires an
//! exact match: text similarity is fuzzy at the word, sound-alike and character level.
//!
//! - [`normalize`]: lower-casing, punctuation, numbers spelled as words.
//! - [`text`]: TF-IDF cosine over word n-grams, phonetic shingles (Double Metaphone), and fuzzy
//!   phrase matching; the summary fallback.
//! - [`title_hook`]: the episode title occurring inside the transcript.
//! - [`duration`]: file length against listed runtime.
//! - [`align`]: locating each short file inside the play-all by audio, and deriving disc order.
//! - [`assign`]: order-preserving dynamic programming when disc order is trustworthy, otherwise
//!   Hungarian assignment with "no episode" columns.
//! - [`confidence`]: margin over the runner-up, the resulting verdict, and which files need more
//!   listening.
//! - [`matcher`]: the entry point, [`match_files`].
//!
//! The crate does no I/O: audio arrives as PCM samples and text as strings. How and why it works
//! is explained in `docs/identification.md`.

pub mod align;
pub mod assign;
pub mod confidence;
pub mod duration;
pub mod matcher;
pub mod normalize;
mod quote;
#[cfg(test)]
mod testutil;
pub mod text;
pub mod title_hook;

pub use align::{
    Alignment, DiscOrder, DiscOrderProblem, Fingerprint, FingerprintBuilder, MIN_ALIGNMENT_SCORE,
    derive_disc_order, locate,
};
pub use confidence::{classify, needs_more_listening};
pub use matcher::{
    DiscOrderUse, EpisodeInput, FileInput, MatchConfig, MatchInput, MatchOutcome, ScoreMatrix,
    SignalWeights, match_files, match_with_outcome, score_all,
};

/// Errors from this crate.
#[derive(Debug, thiserror::Error)]
pub enum MatchError {
    /// The input is inconsistent (for example a disc order naming an unknown file).
    #[error("invalid match input: {0}")]
    InvalidInput(String),
    /// The operation was cancelled.
    #[error("cancelled")]
    Cancelled,
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, MatchError>;
