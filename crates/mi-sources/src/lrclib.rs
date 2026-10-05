//! LRCLIB (<https://lrclib.net/docs>): song lyrics, for musical shorts whose episodes are songs.
//!
//! No key. LRCLIB asks clients to send requests one at a time with a short pause and to identify
//! themselves in the User-Agent; [`crate::HttpClient`] does both. Each episode is searched by its
//! title as the track name, first with the show name as the artist and then as the album, and a
//! record is accepted only when its track name matches the title and its artist or album names
//! the show. Plain lyrics are preferred over synced ones because matching needs only the words.

use std::sync::Arc;

use async_trait::async_trait;
use mi_types::{Episode, ProviderId, ReferenceText, Show, TextKind};
use serde::Deserialize;

use crate::fetch::{self, Freshness};
use crate::http::Request;
use crate::names;
use crate::provider::{ReferenceProvider, ReferenceRequest, reference_text};
use crate::{Cache, HttpClient};

/// Search endpoint.
pub const SEARCH_URL: &str = "https://lrclib.net/api/search";

/// Search results are reused for 90 days; song lyrics rarely change.
pub const FRESHNESS: Freshness = Freshness::days(90);

/// Episodes longer than this (seconds) are not looked up: lyrics are evidence only for shorts
/// that are songs, and skipping long episodes keeps a drama's 100 episodes from costing 200
/// requests. Episodes without a listed runtime are looked up.
pub const MAX_RUNTIME_S: f64 = 10.0 * 60.0;

/// One LRCLIB lyrics record.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsRecord {
    /// LRCLIB's stable record id.
    pub id: u64,
    /// Track title.
    #[serde(default)]
    pub track_name: String,
    /// Artist.
    #[serde(default)]
    pub artist_name: String,
    /// Album.
    #[serde(default)]
    pub album_name: String,
    /// Track length in seconds.
    #[serde(default)]
    pub duration: Option<f64>,
    /// True when the track has no lyrics.
    #[serde(default)]
    pub instrumental: bool,
    /// Unsynced lyrics.
    #[serde(default)]
    pub plain_lyrics: Option<String>,
    /// LRC lyrics with line timestamps.
    #[serde(default)]
    pub synced_lyrics: Option<String>,
}

impl LyricsRecord {
    /// The lyrics as plain lines: the plain lyrics, else the synced lyrics without timestamps.
    pub fn text(&self) -> Option<String> {
        let plain = self
            .plain_lyrics
            .as_deref()
            .map(crate::text::plain_lyrics)
            .filter(|t| !t.is_empty());
        plain.or_else(|| {
            self.synced_lyrics
                .as_deref()
                .map(crate::text::lrc_to_lyrics)
                .filter(|t| !t.is_empty())
        })
    }

    /// The synced lyrics as `(seconds, line)` pairs (empty when only plain lyrics exist).
    pub fn synced_lines(&self) -> Vec<(f64, String)> {
        self.synced_lyrics
            .as_deref()
            .map(crate::text::lrc_lines)
            .unwrap_or_default()
    }
}

/// The LRCLIB reference-text provider.
#[derive(Debug, Clone)]
pub struct Lrclib {
    http: HttpClient,
    cache: Arc<Cache>,
}

impl Lrclib {
    /// Creates the provider.
    pub fn new(http: HttpClient, cache: Arc<Cache>) -> Self {
        Self { http, cache }
    }

    /// Searches by track name, optionally narrowed by artist or album (cached).
    pub async fn search(
        &self,
        track: &str,
        artist: Option<&str>,
        album: Option<&str>,
    ) -> crate::Result<Vec<LyricsRecord>> {
        let mut params = vec![("track_name", track)];
        if let Some(artist) = artist {
            params.push(("artist_name", artist));
        }
        if let Some(album) = album {
            params.push(("album_name", album));
        }
        let request = Request::get(ProviderId::Lrclib, SEARCH_URL, &params)?;
        fetch::json(&self.http, &self.cache, &request, FRESHNESS).await
    }

    /// The best lyrics record for one episode, if LRCLIB has an acceptable one.
    pub async fn episode_record(
        &self,
        show: &Show,
        episode: &Episode,
    ) -> crate::Result<Option<LyricsRecord>> {
        if episode.title.trim().is_empty() {
            return Ok(None);
        }
        let by_artist = self.search(&episode.title, Some(&show.name), None).await?;
        if let Some(best) = best_match(&by_artist, &episode.title, &show.name, episode.runtime_s) {
            return Ok(Some(best.clone()));
        }
        let by_album = self.search(&episode.title, None, Some(&show.name)).await?;
        Ok(best_match(&by_album, &episode.title, &show.name, episode.runtime_s).cloned())
    }

    /// Lyrics for one episode as reference text, if LRCLIB has an acceptable record.
    pub async fn episode_lyrics(
        &self,
        show: &Show,
        episode: &Episode,
        language: &str,
    ) -> crate::Result<Option<ReferenceText>> {
        let Some(record) = self.episode_record(show, episode).await? else {
            return Ok(None);
        };
        Ok(record.text().map(|text| {
            reference_text(
                episode,
                TextKind::Lyrics,
                ProviderId::Lrclib,
                record.id.to_string(),
                text,
                language,
            )
        }))
    }
}

#[async_trait]
impl ReferenceProvider for Lrclib {
    fn id(&self) -> ProviderId {
        ProviderId::Lrclib
    }

    /// Looks up each episode whose runtime is unknown or at most [`MAX_RUNTIME_S`].
    async fn reference_texts(
        &self,
        request: &ReferenceRequest<'_>,
    ) -> crate::Result<Vec<ReferenceText>> {
        let mut out = Vec::new();
        for episode in request.episodes {
            request.check_cancel()?;
            if episode.runtime_s.is_some_and(|r| r > MAX_RUNTIME_S) {
                continue;
            }
            if let Some(text) = self
                .episode_lyrics(request.show, episode, request.language)
                .await?
            {
                out.push(text);
            }
        }
        Ok(out)
    }
}

/// The best acceptable record for an episode titled `title` of show `show`.
///
/// Acceptable: not instrumental, has lyrics, its track name equals the title or contains it as
/// whole words (`"Grammar Rock - Conjunction Junction"`), and its artist or album names the show.
/// Ranking: exact title first, then plain lyrics, then duration closest to `runtime_s` (when
/// known), then the lowest id so the choice is stable.
pub fn best_match<'a>(
    records: &'a [LyricsRecord],
    title: &str,
    show: &str,
    runtime_s: Option<f64>,
) -> Option<&'a LyricsRecord> {
    records
        .iter()
        .filter(|r| !r.instrumental && r.text().is_some())
        .filter(|r| {
            names::same_title(&r.track_name, title) || names::contains_words(&r.track_name, title)
        })
        .filter(|r| names::refers_to(&r.artist_name, show) || names::refers_to(&r.album_name, show))
        .min_by(|a, b| {
            let rank = |r: &LyricsRecord| {
                let exact = !names::same_title(&r.track_name, title);
                let plain = r.plain_lyrics.as_deref().is_none_or(str::is_empty);
                let distance = match (runtime_s, r.duration) {
                    (Some(runtime), Some(duration)) => (runtime - duration).abs(),
                    _ => f64::MAX,
                };
                (exact, plain, distance, r.id)
            };
            let (ra, rb) = (rank(a), rank(b));
            (ra.0, ra.1)
                .cmp(&(rb.0, rb.1))
                .then(ra.2.total_cmp(&rb.2))
                .then(ra.3.cmp(&rb.3))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: u64, track: &str, artist: &str, album: &str, duration: f64) -> LyricsRecord {
        LyricsRecord {
            id,
            track_name: track.into(),
            artist_name: artist.into(),
            album_name: album.into(),
            duration: Some(duration),
            instrumental: false,
            plain_lyrics: Some("words".into()),
            synced_lyrics: None,
        }
    }

    #[test]
    fn recorded_search_picks_an_exact_title_by_the_show() {
        let records: Vec<LyricsRecord> = serde_json::from_str(include_str!(
            "../tests/fixtures/lrclib/search-conjunction-junction.json"
        ))
        .unwrap();
        let best = best_match(
            &records,
            "Conjunction Junction",
            "Schoolhouse Rock!",
            Some(180.0),
        )
        .unwrap();
        assert_eq!(best.track_name, "Conjunction Junction");
        assert!((best.duration.unwrap() - 180.0).abs() < 1.0);
        assert!(best.text().unwrap().starts_with("Conjunction Junction"));
    }

    #[test]
    fn records_by_other_artists_or_titles_are_rejected() {
        let records = vec![
            record(
                1,
                "Conjunction Junction",
                "Couch",
                "Conjunction Junction",
                276.0,
            ),
            record(
                2,
                "Zeros and Ones",
                "Schoolhouse Rock",
                "Schoolhouse Rock!",
                180.0,
            ),
        ];
        assert!(best_match(&records, "Conjunction Junction", "Schoolhouse Rock!", None).is_none());
        assert!(best_match(&records, "Zero", "Schoolhouse Rock!", None).is_none());
    }

    #[test]
    fn ranking_prefers_exact_title_then_plain_then_duration() {
        let mut synced_only = record(1, "Conjunction Junction", "Schoolhouse Rock", "", 180.0);
        synced_only.plain_lyrics = None;
        synced_only.synced_lyrics = Some("[00:01.00]Conjunction Junction".into());
        let medley = record(
            2,
            "Grammar Rock - Conjunction Junction",
            "Schoolhouse Rock",
            "",
            180.0,
        );
        let far = record(3, "Conjunction Junction", "Schoolhouse Rock", "", 300.0);
        let near = record(4, "Conjunction Junction", "School House Rock", "", 179.0);
        let records = vec![synced_only.clone(), medley.clone(), far, near];
        let best = best_match(
            &records,
            "Conjunction Junction",
            "Schoolhouse Rock!",
            Some(180.0),
        );
        assert_eq!(best.map(|r| r.id), Some(4));
        let pair = [synced_only.clone(), medley];
        let best = best_match(&pair, "Conjunction Junction", "Schoolhouse Rock!", None);
        assert_eq!(best.map(|r| r.id), Some(1));
        assert_eq!(synced_only.text().as_deref(), Some("Conjunction Junction"));
        assert_eq!(
            synced_only.synced_lines(),
            vec![(1.0, "Conjunction Junction".to_owned())]
        );
    }

    #[test]
    fn instrumental_records_are_skipped() {
        let mut r = record(1, "Title", "Show", "", 100.0);
        r.instrumental = true;
        assert!(best_match(&[r], "Title", "Show", None).is_none());
    }
}
