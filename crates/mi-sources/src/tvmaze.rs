//! TVmaze (<https://www.tvmaze.com/api>): show search, episode lists and DVD-order lists.
//!
//! No key. Data is CC BY-SA 4.0: the UI shows a visible credit linking to TVmaze. Endpoints used:
//! `/search/shows?q=`, `/shows/:id`, `/shows/:id/episodes?specials=1`,
//! `/shows/:id/alternatelists`, `/alternatelists/:id/alternateepisodes?embed=episodes`.
//! Rate limit: at least 20 calls per 10 seconds per IP; HTTP 429 is retried by
//! [`crate::HttpClient`].

use std::sync::Arc;

use async_trait::async_trait;
use mi_types::{Episode, EpisodeKey, EpisodeOrdering, ProviderId, Show, ShowCandidate, ShowRef};
use serde::Deserialize;

use crate::fetch::{self, Freshness};
use crate::http::Request;
use crate::provider::{EpisodeProvider, ShowIds};
use crate::{Cache, HttpClient, SourceError};

/// Base URL.
pub const BASE_URL: &str = "https://api.tvmaze.com";

/// Search results are reused for a day.
pub const SEARCH_FRESHNESS: Freshness = Freshness::days(1);
/// Show details, episode lists and alternate lists are reused for a week; new episodes of a
/// running show appear after at most that long.
pub const LIST_FRESHNESS: Freshness = Freshness::days(7);

/// How many search results get their season and episode counts filled in (one cached episode
/// list request each).
pub const COUNTED_RESULTS: usize = 5;

/// The TVmaze episode-list provider.
#[derive(Debug, Clone)]
pub struct Tvmaze {
    http: HttpClient,
    cache: Arc<Cache>,
}

impl Tvmaze {
    /// Creates the provider.
    pub fn new(http: HttpClient, cache: Arc<Cache>) -> Self {
        Self { http, cache }
    }

    /// Show details, including identifiers at other services (`externals`).
    pub async fn show(&self, id: &str) -> crate::Result<(Show, ShowIds)> {
        let request = Request::get(ProviderId::Tvmaze, &format!("{BASE_URL}/shows/{id}"), &[])?;
        let show: TvShow = fetch::json(&self.http, &self.cache, &request, LIST_FRESHNESS).await?;
        let ids = show.ids();
        Ok((show.into_show(), ids))
    }

    async fn raw_episodes(&self, id: &str) -> crate::Result<Vec<TvEpisode>> {
        let request = Request::get(
            ProviderId::Tvmaze,
            &format!("{BASE_URL}/shows/{id}/episodes"),
            &[("specials", "1")],
        )?;
        fetch::json(&self.http, &self.cache, &request, LIST_FRESHNESS).await
    }

    async fn dvd_episodes(&self, show: &ShowRef) -> crate::Result<Vec<Episode>> {
        let request = Request::get(
            ProviderId::Tvmaze,
            &format!("{BASE_URL}/shows/{}/alternatelists", show.id),
            &[],
        )?;
        let lists: Vec<AlternateList> =
            fetch::json(&self.http, &self.cache, &request, LIST_FRESHNESS).await?;
        let Some(list) = lists.iter().find(|l| l.dvd_release) else {
            return Err(SourceError::BadResponse {
                provider: ProviderId::Tvmaze,
                message: "this show has no DVD order on TVmaze".to_owned(),
            });
        };
        let request = Request::get(
            ProviderId::Tvmaze,
            &format!("{BASE_URL}/alternatelists/{}/alternateepisodes", list.id),
            &[("embed", "episodes")],
        )?;
        let alternate: Vec<AlternateEpisode> =
            fetch::json(&self.http, &self.cache, &request, LIST_FRESHNESS).await?;
        Ok(dvd_list(show, alternate))
    }
}

#[async_trait]
impl EpisodeProvider for Tvmaze {
    fn id(&self) -> ProviderId {
        ProviderId::Tvmaze
    }

    /// Searches shows. Scores are TVmaze's relevance scores divided by the best score. The first
    /// [`COUNTED_RESULTS`] results get season and episode counts from their (cached) episode
    /// lists; a failure there leaves the counts empty rather than failing the search.
    async fn search_shows(&self, query: &str) -> crate::Result<Vec<ShowCandidate>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let request = Request::get(
            ProviderId::Tvmaze,
            &format!("{BASE_URL}/search/shows"),
            &[("q", query)],
        )?;
        let results: Vec<SearchResult> =
            fetch::json(&self.http, &self.cache, &request, SEARCH_FRESHNESS).await?;
        let mut candidates = search_candidates(results);
        for candidate in candidates.iter_mut().take(COUNTED_RESULTS) {
            match self.raw_episodes(&candidate.show.show_ref.id).await {
                Ok(episodes) => {
                    let (seasons, count) = counts(&episodes);
                    candidate.show.season_count = Some(seasons);
                    candidate.show.episode_count = Some(count);
                }
                Err(e) => tracing::info!(error = %e, "could not count episodes"),
            }
        }
        Ok(candidates)
    }

    /// Episode list of a TVmaze show. In `Aired` order, specials (episodes without a number)
    /// become season 0, numbered in air-date order. `Dvd` uses the show's alternate list marked
    /// as a DVD release and fails with `BadResponse` when there is none.
    async fn episodes(
        &self,
        show: &ShowRef,
        ordering: EpisodeOrdering,
    ) -> crate::Result<Vec<Episode>> {
        match ordering {
            EpisodeOrdering::Aired => Ok(aired_list(show, self.raw_episodes(&show.id).await?)),
            EpisodeOrdering::Dvd => self.dvd_episodes(show).await,
        }
    }
}

#[derive(Debug, Deserialize)]
struct SearchResult {
    score: f64,
    show: TvShow,
}

#[derive(Debug, Deserialize)]
struct TvShow {
    id: u64,
    name: String,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    premiered: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    externals: Option<Externals>,
}

#[derive(Debug, Default, Deserialize)]
struct Externals {
    #[serde(default)]
    thetvdb: Option<u64>,
    #[serde(default)]
    imdb: Option<String>,
}

impl TvShow {
    fn ids(&self) -> ShowIds {
        let ext = self.externals.as_ref();
        ShowIds {
            imdb: ext.and_then(|e| e.imdb.clone()),
            tvdb: ext.and_then(|e| e.thetvdb),
            tmdb: None,
        }
    }

    fn into_show(self) -> Show {
        Show {
            show_ref: ShowRef {
                provider: ProviderId::Tvmaze,
                id: self.id.to_string(),
            },
            year: year_of(self.premiered.as_deref()),
            name: self.name,
            kind: self.kind,
            season_count: None,
            episode_count: None,
            url: self.url,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TvEpisode {
    id: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    season: Option<u32>,
    #[serde(default)]
    number: Option<u32>,
    #[serde(default)]
    airdate: Option<String>,
    #[serde(default)]
    runtime: Option<f64>,
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlternateList {
    id: u64,
    #[serde(default)]
    dvd_release: bool,
}

#[derive(Debug, Deserialize)]
struct AlternateEpisode {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    season: Option<u32>,
    #[serde(default)]
    number: Option<u32>,
    #[serde(default)]
    runtime: Option<f64>,
    #[serde(default, rename = "_embedded")]
    embedded: Option<AlternateEmbedded>,
}

#[derive(Debug, Deserialize)]
struct AlternateEmbedded {
    #[serde(default)]
    episodes: Vec<TvEpisode>,
}

/// `"1973-01-06"` → 1973.
pub(crate) fn year_of(date: Option<&str>) -> Option<u16> {
    date?.get(..4)?.parse().ok()
}

fn search_candidates(results: Vec<SearchResult>) -> Vec<ShowCandidate> {
    let best = results.iter().map(|r| r.score).fold(0.0_f64, f64::max);
    results
        .into_iter()
        .map(|r| ShowCandidate {
            score: if best > 0.0 {
                (r.score / best).clamp(0.0, 1.0) as f32
            } else {
                0.0
            },
            show: r.show.into_show(),
            guessed_from_folder: false,
        })
        .collect()
}

/// Number of seasons and regular (numbered) episodes.
fn counts(episodes: &[TvEpisode]) -> (u32, u32) {
    let regular: Vec<&TvEpisode> = episodes.iter().filter(|e| e.number.is_some()).collect();
    let mut seasons: Vec<u32> = regular.iter().filter_map(|e| e.season).collect();
    seasons.sort_unstable();
    seasons.dedup();
    (seasons.len() as u32, regular.len() as u32)
}

fn episode_from(
    show: &ShowRef,
    ordering: EpisodeOrdering,
    key: EpisodeKey,
    e: &TvEpisode,
) -> Episode {
    Episode {
        show_ref: show.clone(),
        ordering,
        key,
        title: e.name.clone().unwrap_or_default(),
        runtime_s: e.runtime.map(|m| m * 60.0),
        airdate: e.airdate.clone().filter(|d| !d.is_empty()),
        summary: e
            .summary
            .as_deref()
            .map(crate::text::strip_html)
            .filter(|s| !s.is_empty()),
        provider_episode_id: e.id.to_string(),
    }
}

fn aired_list(show: &ShowRef, raw: Vec<TvEpisode>) -> Vec<Episode> {
    let mut out = Vec::with_capacity(raw.len());
    let mut specials = Vec::new();
    for e in &raw {
        match (e.season, e.number) {
            (Some(season), Some(number)) => out.push(episode_from(
                show,
                EpisodeOrdering::Aired,
                EpisodeKey { season, number },
                e,
            )),
            _ => specials.push(e),
        }
    }
    // Specials without an air date go last; ties keep TVmaze's id order.
    specials.sort_by(|a, b| {
        let key = |e: &TvEpisode| {
            (
                e.airdate.clone().filter(|d| !d.is_empty()).is_none(),
                e.airdate.clone().unwrap_or_default(),
                e.id,
            )
        };
        key(a).cmp(&key(b))
    });
    for (i, e) in specials.into_iter().enumerate() {
        let key = EpisodeKey {
            season: 0,
            number: i as u32 + 1,
        };
        out.push(episode_from(show, EpisodeOrdering::Aired, key, e));
    }
    out.sort_by_key(|e| e.key);
    out
}

fn dvd_list(show: &ShowRef, alternate: Vec<AlternateEpisode>) -> Vec<Episode> {
    let mut out = Vec::with_capacity(alternate.len());
    for alt in alternate {
        let (Some(season), Some(number)) = (alt.season, alt.number) else {
            continue;
        };
        let Some(original) = alt.embedded.as_ref().and_then(|e| e.episodes.first()) else {
            continue;
        };
        let mut episode = episode_from(
            show,
            EpisodeOrdering::Dvd,
            EpisodeKey { season, number },
            original,
        );
        if let Some(name) = alt.name.filter(|n| !n.is_empty()) {
            episode.title = name;
        }
        if let Some(runtime) = alt.runtime {
            episode.runtime_s = Some(runtime * 60.0);
        }
        out.push(episode);
    }
    out.sort_by_key(|e| e.key);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show_ref(id: &str) -> ShowRef {
        ShowRef {
            provider: ProviderId::Tvmaze,
            id: id.into(),
        }
    }

    #[test]
    fn specials_become_season_zero_in_air_date_order() {
        let raw: Vec<TvEpisode> = serde_json::from_str(include_str!(
            "../tests/fixtures/tvmaze/sherlock-episodes.json"
        ))
        .unwrap();
        let list = aired_list(&show_ref("335"), raw);
        let specials: Vec<(u32, &str)> = list
            .iter()
            .filter(|e| e.key.season == 0)
            .map(|e| (e.key.number, e.title.as_str()))
            .collect();
        assert_eq!(
            specials,
            vec![
                (1, "Unaired Pilot"),
                (2, "Unlocking Sherlock"),
                (3, "Sherlock Uncovered"),
                (4, "Many Happy Returns"),
                (5, "Unlocking Sherlock (2013)"),
                (6, "The Abominable Bride"),
            ]
        );
        assert_eq!(list.len(), 18);
        assert_eq!(
            list[0].key,
            EpisodeKey {
                season: 0,
                number: 1
            }
        );
        let pink = list.iter().find(|e| e.title == "A Study in Pink").unwrap();
        assert_eq!(
            pink.key,
            EpisodeKey {
                season: 1,
                number: 1
            }
        );
        assert_eq!(pink.runtime_s, Some(5400.0));
        assert!(!pink.summary.as_deref().unwrap_or("").contains('<'));
    }

    #[test]
    fn dvd_list_keeps_the_original_episode_ids() {
        let alt: Vec<AlternateEpisode> = serde_json::from_str(include_str!(
            "../tests/fixtures/tvmaze/firefly-alternateepisodes.json"
        ))
        .unwrap();
        let list = dvd_list(&show_ref("180"), alt);
        assert_eq!(list.len(), 14);
        assert_eq!(list[0].title, "Serenity");
        assert_eq!(
            list[0].key,
            EpisodeKey {
                season: 1,
                number: 1
            }
        );
        assert_eq!(list[0].provider_episode_id, "13005");
        assert_eq!(list[0].ordering, EpisodeOrdering::Dvd);
        assert_eq!(list[13].title, "Objects in Space");
        assert_eq!(list[13].provider_episode_id, "13004");
    }

    #[test]
    fn year_parsing() {
        assert_eq!(year_of(Some("1973-01-06")), Some(1973));
        assert_eq!(year_of(Some("")), None);
        assert_eq!(year_of(None), None);
    }
}
