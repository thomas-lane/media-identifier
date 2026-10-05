//! TVmaze (<https://www.tvmaze.com/api>): show search, episode lists and DVD-order lists.
//!
//! No key. Data is CC BY-SA 4.0: the UI shows a visible credit linking to TVmaze. Endpoints used:
//! `/search/shows?q=`, `/shows/:id/episodes?specials=1`, `/shows/:id/alternatelists`,
//! `/alternatelists/:id/alternateepisodes?embed=episodes`. Rate limit: at least 20 calls per
//! 10 seconds per IP; HTTP 429 is retried by [`crate::HttpClient`].

use mi_types::{Episode, EpisodeOrdering, ShowCandidate, ShowRef};

use crate::{Cache, HttpClient};

/// Base URL.
pub const BASE_URL: &str = "https://api.tvmaze.com";

/// Searches shows. Scores are TVmaze's relevance scores divided by the best score.
pub async fn search_shows(
    http: &HttpClient,
    cache: &Cache,
    query: &str,
) -> crate::Result<Vec<ShowCandidate>> {
    let _ = (http, cache, query);
    todo!("sources module: TVmaze search")
}

/// Episode list of a TVmaze show. Specials (TVmaze `type: "significant_special"`/`"insignificant_special"`)
/// become season 0, numbered in air-date order.
pub async fn episodes(
    http: &HttpClient,
    cache: &Cache,
    show: &ShowRef,
    ordering: EpisodeOrdering,
) -> crate::Result<Vec<Episode>> {
    let _ = (http, cache, show, ordering);
    todo!("sources module: TVmaze episodes")
}
