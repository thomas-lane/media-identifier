//! Shows and episodes from online episode lists.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// An online source the app talks to, or the file itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProviderId {
    /// TVmaze: episode lists and DVD-order lists. No key.
    Tvmaze,
    /// TMDb: optional episode numbering with the user's own key.
    Tmdb,
    /// SubDL: subtitles.
    Subdl,
    /// LRCLIB: song lyrics.
    Lrclib,
    /// A text subtitle stream inside the file being identified.
    Embedded,
}

/// A show as one provider identifies it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShowRef {
    /// The provider whose id this is.
    pub provider: ProviderId,
    /// The provider's show id, as a string (TVmaze `139`, TMDb `1234`).
    pub id: String,
}

/// A show's descriptive details.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Show {
    /// Provider identity.
    pub show_ref: ShowRef,
    /// Display name, for example `Schoolhouse Rock!`.
    pub name: String,
    /// First-aired year, used in folder names (`Schoolhouse Rock! (1973)`).
    pub year: Option<u16>,
    /// Show type as the provider labels it (for example `Animation`), for display.
    pub kind: Option<String>,
    /// Number of seasons, when known.
    pub season_count: Option<u32>,
    /// Number of regular episodes (specials excluded), when known.
    pub episode_count: Option<u32>,
    /// Link to the show's page at the provider, used for the attribution credit.
    pub url: Option<String>,
}

/// A search result on the Confirm show screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShowCandidate {
    /// The show.
    pub show: Show,
    /// Provider relevance score, normalised to `0.0..=1.0`.
    pub score: f32,
    /// True when this result came from the folder-name guess rather than typed text.
    pub guessed_from_folder: bool,
}

/// Which episode numbering to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum EpisodeOrdering {
    /// Original broadcast order (TVmaze's main list; TMDb's default numbering).
    #[default]
    Aired,
    /// DVD order (a TVmaze alternate list marked as DVD).
    Dvd,
}

/// Season and episode number within one ordering. Season 0 holds specials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeKey {
    /// Season number; 0 for specials.
    pub season: u32,
    /// Episode number within the season, starting at 1.
    pub number: u32,
}

/// One episode in a show's list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    /// The show this belongs to.
    pub show_ref: ShowRef,
    /// The ordering `key` is numbered in.
    pub ordering: EpisodeOrdering,
    /// Season and number.
    pub key: EpisodeKey,
    /// Episode title.
    pub title: String,
    /// Listed runtime in seconds, when known.
    pub runtime_s: Option<f64>,
    /// First air date as `YYYY-MM-DD`, when known.
    pub airdate: Option<String>,
    /// Plain-text summary (HTML removed), when known.
    pub summary: Option<String>,
    /// The provider's own episode id, stable across orderings.
    pub provider_episode_id: String,
}
