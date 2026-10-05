//! The facade `mi-core` uses.

use mi_types::{
    CancelFlag, Episode, EpisodeOrdering, ReferenceText, Show, ShowCandidate, ShowRef, SourceStatus,
};

use crate::{Cache, HttpClient};

/// User-entered API keys. Keys are never logged or sent to the UI.
#[derive(Clone, Default)]
pub struct ApiKeys {
    /// SubDL key.
    pub subdl: Option<String>,
    /// TMDb key (v3 API key or v4 read access token).
    pub tmdb: Option<String>,
}

impl std::fmt::Debug for ApiKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiKeys")
            .field("subdl", &self.subdl.as_ref().map(|_| "<set>"))
            .field("tmdb", &self.tmdb.as_ref().map(|_| "<set>"))
            .finish()
    }
}

/// All online sources behind one cache.
#[derive(Debug)]
pub struct Sources {
    http: HttpClient,
    cache: Cache,
    keys: ApiKeys,
}

impl Sources {
    /// Creates the facade.
    pub fn new(http: HttpClient, cache: Cache, keys: ApiKeys) -> Self {
        Self { http, cache, keys }
    }

    /// Replaces the API keys (after the user edits them in Settings).
    pub fn set_keys(&mut self, keys: ApiKeys) {
        self.keys = keys;
    }

    /// Searches TVmaze for shows matching `query`, best first.
    pub async fn search_shows(&self, query: &str) -> crate::Result<Vec<ShowCandidate>> {
        let _ = (&self.http, &self.cache, &self.keys, query);
        todo!("sources module: TVmaze /search/shows")
    }

    /// The episode list in `ordering`, specials as season 0. Uses TMDb numbering when a TMDb key
    /// is set and the show can be mapped to TMDb (through TVmaze's externals), otherwise TVmaze.
    /// `Dvd` uses TVmaze's alternate list marked as DVD and fails with `BadResponse` when the
    /// show has none. Cached; TMDb data is refreshed after at most six months.
    pub async fn episodes(
        &self,
        show: &ShowRef,
        ordering: EpisodeOrdering,
    ) -> crate::Result<Vec<Episode>> {
        let _ = (show, ordering);
        todo!("sources module: episode lists")
    }

    /// Fetches reference text for `episodes`: SubDL season packs first (when a key is set), then
    /// LRCLIB lyrics for episodes without subtitles, and the episode summary as a last resort.
    /// Embedded subtitle streams are added by `mi-core` through [`crate::embedded`]. Calls
    /// `on_progress(done, total)` per episode. Everything fetched is cached first, so a repeated
    /// call makes no requests.
    pub async fn reference_texts(
        &self,
        show: &Show,
        episodes: &[Episode],
        language: &str,
        on_progress: &(dyn Fn(u32, u32) + Send + Sync),
        cancel: &CancelFlag,
    ) -> crate::Result<Vec<ReferenceText>> {
        let _ = (show, episodes, language, on_progress, cancel);
        todo!("sources module: reference text")
    }

    /// Status of each source for the Settings screen (TVmaze, LRCLIB, SubDL, TMDb), from the
    /// presence of keys and the outcome of the most recent request to each.
    pub fn status(&self) -> Vec<SourceStatus> {
        todo!("sources module: source status")
    }
}
