//! TMDb (<https://developer.themoviedb.org>): optional episode numbering matching Jellyfin.
//!
//! Used only with the user's own key (Settings > Add key...); the app ships no key because TMDb's
//! terms make each key holder responsible for their use. Responses are cached for at most six
//! months, as TMDb's terms require. The UI shows TMDb's attribution when this source is used.

use mi_types::{Episode, ShowRef};

use crate::{Cache, HttpClient};

/// Base URL.
pub const BASE_URL: &str = "https://api.themoviedb.org/3";

/// Longest time a TMDb response may be served from the cache (six months).
pub const MAX_CACHE_AGE_MS: i64 = 183 * 24 * 60 * 60 * 1000;

/// Episode list of a TMDb TV show in TMDb's default numbering.
pub async fn episodes(
    http: &HttpClient,
    cache: &Cache,
    key: &str,
    show: &ShowRef,
) -> crate::Result<Vec<Episode>> {
    let _ = (http, cache, key, show);
    todo!("sources module: TMDb episodes")
}

/// Checks that a key works (Settings shows `KeyRejected` otherwise).
pub async fn validate_key(http: &HttpClient, key: &str) -> crate::Result<()> {
    let _ = (http, key);
    todo!("sources module: TMDb key check")
}
