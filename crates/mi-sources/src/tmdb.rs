//! TMDb (<https://developer.themoviedb.org>): optional episode numbering matching Jellyfin.
//!
//! Used only with the user's own key (Settings > Add key...). The app ships no key: TMDb's API
//! terms prohibit using TMDb "in connection with ... a machine learning (ML) or artificial
//! intelligence (AI) based Application", and the app's speech recognition is machine learning, so
//! TMDb is left to users who accept TMDb's terms for their own use. Both key kinds work: a 32-character v3
//! API key is sent as the `api_key` query parameter, anything else is sent as a v4 read access
//! token in `Authorization: Bearer`. Responses are cached for at most six months, as TMDb's terms
//! require ([`MAX_CACHE_AGE_MS`]). The UI shows TMDb's notice and logo when this source is used
//! ([`crate::attribution()`]).
//!
//! Endpoints used: `/authentication` (key check), `/search/tv`, `/find/{id}` (from TheTVDB or
//! IMDb ids), `/tv/{id}`, `/tv/{id}/season/{n}`, `/tv/{id}/episode_groups`,
//! `/tv/episode_group/{id}`.

use std::sync::Arc;

use async_trait::async_trait;
use mi_types::{Episode, EpisodeKey, EpisodeOrdering, ProviderId, Show, ShowCandidate, ShowRef};
use serde::Deserialize;

use crate::fetch::{self, DAY_MS, Freshness};
use crate::http::{Request, Secret};
use crate::provider::{EpisodeProvider, ShowIds};
use crate::{Cache, HttpClient, SourceError};

/// Base URL.
pub const BASE_URL: &str = "https://api.themoviedb.org/3";

/// Longest time a TMDb response may be served from the cache (six months).
pub const MAX_CACHE_AGE_MS: i64 = 183 * DAY_MS;

/// TMDb responses are refreshed after 30 days and never used beyond [`MAX_CACHE_AGE_MS`], even
/// offline.
pub const FRESHNESS: Freshness = Freshness {
    ttl_ms: 30 * DAY_MS,
    stale_limit_ms: Some(MAX_CACHE_AGE_MS),
};

/// TMDb episode group type for DVD order.
pub const GROUP_TYPE_DVD: u32 = 3;

/// How a key is sent.
pub fn secret_for(key: &str) -> Secret {
    let key = key.trim();
    if key.len() == 32 && key.chars().all(|c| c.is_ascii_hexdigit()) {
        Secret::Query {
            name: "api_key",
            value: key.to_owned(),
        }
    } else {
        Secret::Bearer(key.to_owned())
    }
}

/// Checks that a key works (`KeyRejected` otherwise). Never cached.
pub async fn validate_key(http: &HttpClient, key: &str) -> crate::Result<()> {
    let request = Request::get(ProviderId::Tmdb, &format!("{BASE_URL}/authentication"), &[])?
        .with_secret(secret_for(key));
    http.send(&request).await.map(|_| ())
}

/// The TMDb episode-list provider. Its `Debug` output hides the key.
#[derive(Clone)]
pub struct Tmdb {
    http: HttpClient,
    cache: Arc<Cache>,
    key: String,
}

impl std::fmt::Debug for Tmdb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tmdb")
            .field("key", &"<hidden>")
            .finish_non_exhaustive()
    }
}

impl Tmdb {
    /// Creates the provider with the user's key.
    pub fn new(http: HttpClient, cache: Arc<Cache>, key: String) -> Self {
        Self { http, cache, key }
    }

    fn request(&self, path: &str, params: &[(&str, &str)]) -> crate::Result<Request> {
        Ok(
            Request::get(ProviderId::Tmdb, &format!("{BASE_URL}{path}"), params)?
                .with_secret(secret_for(&self.key)),
        )
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> crate::Result<T> {
        let request = self.request(path, params)?;
        fetch::json(&self.http, &self.cache, &request, FRESHNESS).await
    }

    /// The TMDb id of a show known elsewhere: through TheTVDB id, then IMDb id, then a name
    /// search accepted only when exactly one result has the same name and first-air year.
    pub async fn find_show(
        &self,
        ids: &ShowIds,
        name: &str,
        year: Option<u16>,
    ) -> crate::Result<Option<u64>> {
        if let Some(tmdb) = ids.tmdb {
            return Ok(Some(tmdb));
        }
        let mut lookups: Vec<(String, &str)> = Vec::new();
        if let Some(tvdb) = ids.tvdb {
            lookups.push((tvdb.to_string(), "tvdb_id"));
        }
        if let Some(imdb) = &ids.imdb {
            lookups.push((imdb.clone(), "imdb_id"));
        }
        for (id, source) in lookups {
            let found: FindResponse = self
                .get(&format!("/find/{id}"), &[("external_source", source)])
                .await?;
            if let Some(show) = found.tv_results.first() {
                return Ok(Some(show.id));
            }
        }
        let results = self.search(name, year).await?;
        let same: Vec<&TvResult> = results
            .iter()
            .filter(|r| crate::names::same_title(&r.name, name))
            .filter(|r| {
                year.is_none() || crate::tvmaze::year_of(r.first_air_date.as_deref()) == year
            })
            .collect();
        Ok(match same.as_slice() {
            [one] => Some(one.id),
            _ => None,
        })
    }

    async fn search(&self, query: &str, year: Option<u16>) -> crate::Result<Vec<TvResult>> {
        let year = year.map(|y| y.to_string());
        let mut params = vec![("query", query)];
        if let Some(year) = &year {
            params.push(("first_air_date_year", year));
        }
        let page: SearchPage = self.get("/search/tv", &params).await?;
        Ok(page.results)
    }

    /// The show's episode groups (alternative orders), as `(id, name, type)`.
    pub async fn episode_groups(&self, show_id: &str) -> crate::Result<Vec<(String, String, u32)>> {
        let groups: GroupList = self
            .get(&format!("/tv/{show_id}/episode_groups"), &[])
            .await?;
        Ok(groups
            .results
            .into_iter()
            .map(|g| (g.id, g.name, g.kind))
            .collect())
    }

    async fn aired(&self, show: &ShowRef) -> crate::Result<Vec<Episode>> {
        let details: TvDetails = self.get(&format!("/tv/{}", show.id), &[]).await?;
        let mut out = Vec::new();
        for season in &details.seasons {
            let season: SeasonDetails = self
                .get(
                    &format!("/tv/{}/season/{}", show.id, season.season_number),
                    &[],
                )
                .await?;
            for e in season.episodes {
                let key = EpisodeKey {
                    season: e.season_number,
                    number: e.episode_number,
                };
                out.push(e.into_episode(show, EpisodeOrdering::Aired, key));
            }
        }
        out.sort_by_key(|e| e.key);
        Ok(out)
    }

    async fn dvd(&self, show: &ShowRef) -> crate::Result<Vec<Episode>> {
        let groups = self.episode_groups(&show.id).await?;
        let Some((group_id, _, _)) = groups.iter().find(|(_, _, kind)| *kind == GROUP_TYPE_DVD)
        else {
            return Err(SourceError::BadResponse {
                provider: ProviderId::Tmdb,
                message: "this show has no DVD order on TMDb".to_owned(),
            });
        };
        let group: GroupDetails = self
            .get(&format!("/tv/episode_group/{group_id}"), &[])
            .await?;
        Ok(group_episodes(show, group))
    }
}

#[async_trait]
impl EpisodeProvider for Tmdb {
    fn id(&self) -> ProviderId {
        ProviderId::Tmdb
    }

    /// TV search; scores fall from 1.0 by result position, because TMDb returns no score.
    async fn search_shows(&self, query: &str) -> crate::Result<Vec<ShowCandidate>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let results = self.search(query, None).await?;
        let n = results.len().max(1) as f32;
        Ok(results
            .into_iter()
            .enumerate()
            .map(|(i, r)| ShowCandidate {
                score: 1.0 - i as f32 / n,
                show: r.into_show(),
                guessed_from_folder: false,
            })
            .collect())
    }

    /// `Aired`: TMDb's default numbering, every season including season 0 (specials). `Dvd`: the
    /// show's first episode group of type DVD; groups become seasons (a group named "Specials"
    /// is season 0, otherwise the number in the group's name, otherwise its position) and
    /// episodes are numbered by their order in the group. Fails with `BadResponse` when the
    /// show has no DVD group.
    async fn episodes(
        &self,
        show: &ShowRef,
        ordering: EpisodeOrdering,
    ) -> crate::Result<Vec<Episode>> {
        match ordering {
            EpisodeOrdering::Aired => self.aired(show).await,
            EpisodeOrdering::Dvd => self.dvd(show).await,
        }
    }
}

#[derive(Debug, Deserialize)]
struct FindResponse {
    #[serde(default)]
    tv_results: Vec<TvResult>,
}

#[derive(Debug, Deserialize)]
struct SearchPage {
    #[serde(default)]
    results: Vec<TvResult>,
}

#[derive(Debug, Deserialize)]
struct TvResult {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    first_air_date: Option<String>,
}

impl TvResult {
    fn into_show(self) -> Show {
        Show {
            show_ref: ShowRef {
                provider: ProviderId::Tmdb,
                id: self.id.to_string(),
            },
            year: crate::tvmaze::year_of(self.first_air_date.as_deref()),
            url: Some(format!("https://www.themoviedb.org/tv/{}", self.id)),
            name: self.name,
            kind: None,
            season_count: None,
            episode_count: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct TvDetails {
    #[serde(default)]
    seasons: Vec<SeasonSummary>,
}

#[derive(Debug, Deserialize)]
struct SeasonSummary {
    season_number: u32,
}

#[derive(Debug, Deserialize)]
struct SeasonDetails {
    #[serde(default)]
    episodes: Vec<TmdbEpisode>,
}

#[derive(Debug, Deserialize)]
struct TmdbEpisode {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    season_number: u32,
    #[serde(default)]
    episode_number: u32,
    #[serde(default)]
    air_date: Option<String>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    runtime: Option<f64>,
    #[serde(default)]
    order: Option<u32>,
}

impl TmdbEpisode {
    fn into_episode(self, show: &ShowRef, ordering: EpisodeOrdering, key: EpisodeKey) -> Episode {
        Episode {
            show_ref: show.clone(),
            ordering,
            key,
            title: self.name,
            runtime_s: self.runtime.map(|m| m * 60.0),
            airdate: self.air_date.filter(|d| !d.is_empty()),
            summary: self.overview.filter(|o| !o.is_empty()),
            provider_episode_id: self.id.to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct GroupList {
    #[serde(default)]
    results: Vec<GroupSummary>,
}

#[derive(Debug, Deserialize)]
struct GroupSummary {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    kind: u32,
}

#[derive(Debug, Deserialize)]
struct GroupDetails {
    #[serde(default)]
    groups: Vec<Group>,
}

#[derive(Debug, Deserialize)]
struct Group {
    #[serde(default)]
    name: String,
    #[serde(default)]
    order: u32,
    #[serde(default)]
    episodes: Vec<TmdbEpisode>,
}

fn group_episodes(show: &ShowRef, details: GroupDetails) -> Vec<Episode> {
    let mut groups = details.groups;
    groups.sort_by_key(|g| g.order);
    let mut out = Vec::new();
    let mut position = 0;
    for group in groups {
        let name = group.name.to_lowercase();
        let season = if name.contains("special") {
            0
        } else {
            position += 1;
            name.split(|c: char| !c.is_ascii_digit())
                .find(|s| !s.is_empty())
                .and_then(|s| s.parse().ok())
                .unwrap_or(position)
        };
        let mut episodes = group.episodes;
        episodes.sort_by_key(|e| e.order.unwrap_or(u32::MAX));
        for (i, e) in episodes.into_iter().enumerate() {
            let key = EpisodeKey {
                season,
                number: i as u32 + 1,
            };
            out.push(e.into_episode(show, EpisodeOrdering::Dvd, key));
        }
    }
    out.sort_by_key(|e| e.key);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_hides_the_key() {
        let cache = Arc::new(Cache::in_memory().unwrap());
        let tmdb = Tmdb::new(HttpClient::new().unwrap(), cache, "tmdb-secret-123".into());
        assert!(!format!("{tmdb:?}").contains("tmdb-secret-123"));
    }

    #[test]
    fn v3_keys_go_in_the_query_and_tokens_in_a_header() {
        assert!(matches!(
            secret_for("0123456789abcdef0123456789ABCDEF"),
            Secret::Query {
                name: "api_key",
                ..
            }
        ));
        assert!(matches!(
            secret_for("eyJhbGciOiJIUzI1NiJ9.x.y"),
            Secret::Bearer(_)
        ));
    }

    #[test]
    fn episode_groups_become_seasons() {
        let details: GroupDetails = serde_json::from_str(include_str!(
            "../tests/fixtures/tmdb/episode-group-dvd.json"
        ))
        .unwrap();
        let show = ShowRef {
            provider: ProviderId::Tmdb,
            id: "1437".into(),
        };
        let list = group_episodes(&show, details);
        let keys: Vec<(u32, u32, &str)> = list
            .iter()
            .map(|e| (e.key.season, e.key.number, e.title.as_str()))
            .collect();
        assert_eq!(
            keys,
            vec![
                (0, 1, "Here's How It All Happened"),
                (1, 1, "Serenity"),
                (1, 2, "The Train Job"),
                (1, 3, "Bushwhacked"),
            ]
        );
        assert_eq!(list[1].provider_episode_id, "71148");
        assert_eq!(list[1].runtime_s, Some(5160.0));
    }
}
