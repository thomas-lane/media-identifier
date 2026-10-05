//! Text that tells us what is said in an episode.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::catalog::{EpisodeKey, EpisodeOrdering, ProviderId, ShowRef};

/// What kind of text a [`ReferenceText`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TextKind {
    /// Subtitle dialogue (SubDL or an embedded text stream).
    Subtitles,
    /// Song lyrics (LRCLIB), for musical shorts.
    Lyrics,
    /// The episode's summary from the episode list. Used only when no dialogue text exists.
    Summary,
}

/// Normalised reference text for one episode.
///
/// `text` is plain dialogue: subtitle timing lines, markup, speaker labels and sound
/// descriptions such as `[music]` removed, one cue per line, original words and case kept.
/// Tokenisation for matching happens in `mi-match`, not here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceText {
    /// The show.
    pub show_ref: ShowRef,
    /// The ordering `episode` is numbered in.
    pub ordering: EpisodeOrdering,
    /// The episode this text belongs to.
    pub episode: EpisodeKey,
    /// What kind of text it is.
    pub kind: TextKind,
    /// Where it came from.
    pub provider: ProviderId,
    /// The provider's identifier for the item (subtitle file id, lyrics id, stream index), so
    /// cached text can be traced and never downloaded twice.
    pub provider_ref: String,
    /// The normalised text.
    pub text: String,
    /// Language tag (ISO 639-1, for example `en`).
    pub language: String,
    /// When it was fetched, Unix milliseconds.
    pub fetched_at_ms: i64,
}
