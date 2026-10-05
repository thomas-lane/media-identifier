//! LRCLIB (<https://lrclib.net/docs>): song lyrics, for musical shorts whose episodes are songs.
//!
//! No key. Searched by episode title (as the track name) and show name (as the artist or album);
//! plain lyrics are preferred over synced ones.

use mi_types::{Episode, ReferenceText, Show};

use crate::{Cache, HttpClient};

/// Search endpoint.
pub const SEARCH_URL: &str = "https://lrclib.net/api/search";

/// Lyrics for one episode, if LRCLIB has a confident match for its title.
pub async fn episode_lyrics(
    http: &HttpClient,
    cache: &Cache,
    show: &Show,
    episode: &Episode,
) -> crate::Result<Option<ReferenceText>> {
    let _ = (http, cache, show, episode);
    todo!("sources module: LRCLIB lookup")
}
