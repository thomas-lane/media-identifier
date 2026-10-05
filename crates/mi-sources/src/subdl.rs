//! SubDL (<https://subdl.com/api-doc>): subtitles, preferably whole-season packs.
//!
//! The search API needs a key (requests without `api_key` were answered HTTP 403
//! `not_authorized` when this module was designed); users enter their own free key in Settings.
//! Season packs are requested with `full_season=1` and downloaded once per season.

use mi_types::{Episode, ReferenceText, Show};

use crate::{Cache, HttpClient};

/// Search endpoint.
pub const SEARCH_URL: &str = "https://api.subdl.com/api/v1/subtitles";

/// Fetches subtitles for the episodes of one season, normalised with [`crate::text`].
pub async fn season_texts(
    http: &HttpClient,
    cache: &Cache,
    key: &str,
    show: &Show,
    season: u32,
    episodes: &[Episode],
    language: &str,
) -> crate::Result<Vec<ReferenceText>> {
    let _ = (http, cache, key, show, season, episodes, language);
    todo!("sources module: SubDL season packs")
}
