//! Matching files to episodes.
//!
//! Every file is scored against every candidate episode with several independent signals, and the
//! best overall assignment wins. Speech recognition will mishear words, so no signal requires an
//! exact match: text similarity is fuzzy at the word, sound-alike and character level.
//!
//! - [`text`]: TF-IDF cosine over word n-grams, phonetic shingles (Double Metaphone), and fuzzy
//!   partial matching.
//! - [`title_hook`]: the episode title occurring inside the transcript.
//! - [`duration`]: file length against listed runtime.
//! - [`align`]: locating each short file inside the play-all by audio, and deriving disc order.
//! - [`assign`]: order-preserving dynamic programming when disc order is trustworthy, otherwise
//!   Hungarian assignment with "no episode" columns.
//! - [`confidence`]: margin over the runner-up and the resulting verdict.
//! - [`matcher`]: the entry point, [`match_files`].
//!
//! How and why it works is explained in `docs/identification.md`.
//!
//! Owner: match module (see `docs/architecture.md`).

pub mod align;
pub mod assign;
pub mod confidence;
pub mod duration;
pub mod matcher;
pub mod text;
pub mod title_hook;

pub use align::{Alignment, DiscOrder, Fingerprint, derive_disc_order, locate};
pub use confidence::classify;
pub use matcher::{
    EpisodeInput, FileInput, MatchConfig, MatchInput, ScoreMatrix, SignalWeights, match_files,
    score_all,
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
