//! The two provider traits: episode lists and reference text.

use std::collections::HashMap;

use async_trait::async_trait;
use mi_types::{
    CancelFlag, Episode, EpisodeKey, EpisodeOrdering, ProviderId, ReferenceText, Show,
    ShowCandidate, ShowRef,
};

/// A source of shows and episode lists (TVmaze, TMDb).
#[async_trait]
pub trait EpisodeProvider: Send + Sync + std::fmt::Debug {
    /// Which provider this is.
    fn id(&self) -> ProviderId;

    /// Shows matching `query`, best first, with scores normalised so the best is 1.0.
    async fn search_shows(&self, query: &str) -> crate::Result<Vec<ShowCandidate>>;

    /// The show's episode list in `ordering`, specials as season 0, sorted by key.
    async fn episodes(
        &self,
        show: &ShowRef,
        ordering: EpisodeOrdering,
    ) -> crate::Result<Vec<Episode>>;
}

/// Identifiers of one show at other services, used to look it up there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShowIds {
    /// IMDb id (`tt0069627`).
    pub imdb: Option<String>,
    /// TheTVDB id.
    pub tvdb: Option<u64>,
    /// TMDb TV id.
    pub tmdb: Option<u64>,
}

/// What a [`ReferenceProvider`] is asked for: text for some episodes of one season-or-more.
#[derive(Debug)]
pub struct ReferenceRequest<'a> {
    /// The show.
    pub show: &'a Show,
    /// Its identifiers at other services.
    pub ids: &'a ShowIds,
    /// The episodes that still lack dialogue text (all from `show`, in one ordering).
    pub episodes: &'a [Episode],
    /// Broadcast (aired) numbering of each episode, by `Episode::provider_episode_id`. Sources
    /// that number episodes themselves (subtitle sites) use it to find the right file when
    /// `episodes` are in DVD order; results are reported under the requested ordering.
    pub aired: &'a HashMap<String, EpisodeKey>,
    /// Language tag (ISO 639-1).
    pub language: &'a str,
    /// Checked between requests.
    pub cancel: &'a CancelFlag,
}

impl ReferenceRequest<'_> {
    /// The aired key of `episode` (its own key when it is already in aired order or unknown).
    pub fn aired_key(&self, episode: &Episode) -> EpisodeKey {
        if episode.ordering == EpisodeOrdering::Aired {
            return episode.key;
        }
        self.aired
            .get(&episode.provider_episode_id)
            .copied()
            .unwrap_or(episode.key)
    }

    /// Fails with `Cancelled` when the job was cancelled.
    pub fn check_cancel(&self) -> crate::Result<()> {
        if self.cancel.is_cancelled() {
            Err(crate::SourceError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// A source of reference text (SubDL subtitles, LRCLIB lyrics, local files).
#[async_trait]
pub trait ReferenceProvider: Send + Sync + std::fmt::Debug {
    /// Which provider this is.
    fn id(&self) -> ProviderId;

    /// Reference texts for as many of `request.episodes` as this source has, each labelled with
    /// the episode's show, ordering and key. Missing episodes are simply absent; an error means
    /// the source could not be used at all.
    async fn reference_texts(
        &self,
        request: &ReferenceRequest<'_>,
    ) -> crate::Result<Vec<ReferenceText>>;
}

/// Builds a [`ReferenceText`] for `episode`.
pub(crate) fn reference_text(
    episode: &Episode,
    kind: mi_types::TextKind,
    provider: ProviderId,
    provider_ref: String,
    text: String,
    language: &str,
) -> ReferenceText {
    ReferenceText {
        show_ref: episode.show_ref.clone(),
        ordering: episode.ordering,
        episode: episode.key,
        kind,
        provider,
        provider_ref,
        text,
        language: language.to_owned(),
        fetched_at_ms: crate::cache::now_ms(),
    }
}
