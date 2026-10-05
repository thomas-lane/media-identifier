//! The facade `mi-core` uses.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mi_types::{
    ApiKeyProvider, CancelFlag, Episode, EpisodeKey, EpisodeOrdering, ProviderId, ReferenceText,
    Show, ShowCandidate, ShowRef, SourceState, SourceStatus, TextKind,
};

use crate::attribution::provider_name;
use crate::http::Outcome;
use crate::lrclib::Lrclib;
use crate::provider::{
    EpisodeProvider, ReferenceProvider, ReferenceRequest, ShowIds, reference_text,
};
use crate::subdl::Subdl;
use crate::tmdb::Tmdb;
use crate::tvmaze::Tvmaze;
use crate::{Cache, HttpClient, SourceError};

/// User-entered API keys. Keys are never logged or sent to the UI.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ApiKeys {
    /// SubDL key.
    pub subdl: Option<String>,
    /// TMDb key (v3 API key or v4 read access token).
    pub tmdb: Option<String>,
}

impl ApiKeys {
    fn get(&self, provider: ProviderId) -> Option<&str> {
        let key = match provider {
            ProviderId::Subdl => self.subdl.as_deref(),
            ProviderId::Tmdb => self.tmdb.as_deref(),
            _ => None,
        };
        key.map(str::trim).filter(|k| !k.is_empty())
    }
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
///
/// Thread-safe (`Send + Sync`); methods take `&self` except
/// [`Sources::add_reference_provider`]. Keys can be replaced while requests run; a request uses
/// the keys current when it started.
#[derive(Debug)]
pub struct Sources {
    http: HttpClient,
    cache: Arc<Cache>,
    keys: std::sync::RwLock<ApiKeys>,
    extra: Vec<Arc<dyn ReferenceProvider>>,
}

impl Sources {
    /// Creates the facade.
    pub fn new(http: HttpClient, cache: Cache, keys: ApiKeys) -> Self {
        Self {
            http,
            cache: Arc::new(cache),
            keys: std::sync::RwLock::new(keys),
            extra: Vec::new(),
        }
    }

    /// Replaces the API keys (after the user edits them in Settings). The recorded outcome of a
    /// provider whose key changed is forgotten, so a rejected key stops showing as rejected.
    pub fn set_keys(&self, keys: ApiKeys) {
        let mut current = self.keys.write().unwrap_or_else(|p| p.into_inner());
        for provider in [ProviderId::Subdl, ProviderId::Tmdb] {
            if current.get(provider) != keys.get(provider) {
                self.http.reset_outcome(provider);
            }
        }
        *current = keys;
    }

    /// The key currently set for `provider`, trimmed; `None` when unset or blank.
    fn key(&self, provider: ProviderId) -> Option<String> {
        self.keys
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(provider)
            .map(str::to_owned)
    }

    /// Adds a reference-text source tried before the online ones, for example
    /// [`crate::local::LocalReferences`]. Its texts are not cached.
    pub fn add_reference_provider(&mut self, provider: Arc<dyn ReferenceProvider>) {
        self.extra.push(provider);
    }

    /// The cache.
    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    /// The TVmaze provider.
    pub fn tvmaze(&self) -> Tvmaze {
        Tvmaze::new(self.http.clone(), Arc::clone(&self.cache))
    }

    /// The TMDb provider, when a TMDb key is set.
    pub fn tmdb(&self) -> Option<Tmdb> {
        self.key(ProviderId::Tmdb)
            .map(|key| Tmdb::new(self.http.clone(), Arc::clone(&self.cache), key))
    }

    /// Searches TVmaze for shows matching `query`, best first.
    pub async fn search_shows(&self, query: &str) -> crate::Result<Vec<ShowCandidate>> {
        self.tvmaze().search_shows(query).await
    }

    /// Identifiers of a show at other services (from TVmaze's `externals`, or the TMDb id).
    pub async fn show_ids(&self, show: &ShowRef) -> crate::Result<ShowIds> {
        match show.provider {
            ProviderId::Tvmaze => Ok(self.tvmaze().show(&show.id).await?.1),
            ProviderId::Tmdb => Ok(ShowIds {
                tmdb: show.id.parse().ok(),
                ..ShowIds::default()
            }),
            other => Err(SourceError::BadResponse {
                provider: other,
                message: "this source has no shows".to_owned(),
            }),
        }
    }

    /// The episode list in `ordering`, specials as season 0, sorted by key.
    ///
    /// For a TVmaze show with a TMDb key set, the show is looked up on TMDb (through TVmaze's
    /// TheTVDB and IMDb ids, then by exact name and year) and TMDb's numbering is returned, so
    /// file names match what Jellyfin shows; those episodes carry TMDb's show and episode ids.
    /// When the show cannot be found on TMDb, or TMDb fails, or (for `Dvd`) TMDb has no DVD
    /// group, TVmaze's list is returned instead. `Dvd` from TVmaze uses its alternate list
    /// marked as DVD and fails with `BadResponse` when the show has none. Cached; TMDb data is
    /// refreshed after at most six months.
    pub async fn episodes(
        &self,
        show: &ShowRef,
        ordering: EpisodeOrdering,
    ) -> crate::Result<Vec<Episode>> {
        match show.provider {
            ProviderId::Tvmaze => {
                if let Some(tmdb) = self.tmdb() {
                    match self.tmdb_episodes_for_tvmaze(&tmdb, show, ordering).await {
                        Ok(Some(list)) => return Ok(list),
                        Ok(None) => {}
                        Err(SourceError::Cancelled) => return Err(SourceError::Cancelled),
                        Err(e) => {
                            tracing::warn!(error = %e, "TMDb numbering unavailable; using TVmaze")
                        }
                    }
                }
                self.tvmaze().episodes(show, ordering).await
            }
            ProviderId::Tmdb => match self.tmdb() {
                Some(tmdb) => tmdb.episodes(show, ordering).await,
                None => Err(SourceError::KeyMissing(ProviderId::Tmdb)),
            },
            other => Err(SourceError::BadResponse {
                provider: other,
                message: "this source has no episode lists".to_owned(),
            }),
        }
    }

    async fn tmdb_episodes_for_tvmaze(
        &self,
        tmdb: &Tmdb,
        show: &ShowRef,
        ordering: EpisodeOrdering,
    ) -> crate::Result<Option<Vec<Episode>>> {
        let (details, ids) = self.tvmaze().show(&show.id).await?;
        let Some(tmdb_id) = tmdb.find_show(&ids, &details.name, details.year).await? else {
            tracing::info!(show = %details.name, "show not found on TMDb");
            return Ok(None);
        };
        let tmdb_ref = ShowRef {
            provider: ProviderId::Tmdb,
            id: tmdb_id.to_string(),
        };
        match tmdb.episodes(&tmdb_ref, ordering).await {
            Ok(list) if !list.is_empty() => Ok(Some(list)),
            Ok(_) | Err(SourceError::BadResponse { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Fetches reference text for `episodes` (all of one show and one ordering).
    ///
    /// For each episode, in this order, until it has dialogue text (subtitles or lyrics):
    /// 1. text cached by an earlier call;
    /// 2. providers added with [`Sources::add_reference_provider`] (local files);
    /// 3. SubDL subtitles, when a SubDL key is set;
    /// 4. LRCLIB lyrics (episodes of ten minutes or less, or of unknown length).
    ///
    /// Episodes still without dialogue text get their summary from the episode list as a
    /// [`TextKind::Summary`] text, when the list has one. Embedded subtitle streams are added by
    /// `mi-core` through [`crate::embedded`].
    ///
    /// A source that fails (network down, rate limited, key rejected) is skipped for the rest of
    /// the call and the next source is tried, so one failing service never fails the job; its
    /// failure shows in [`Sources::status`]. Everything downloaded is cached first, so a repeated
    /// call makes no requests. Episodes are processed season by season, and
    /// `on_progress(done, total)` is called at the start and after each season, counting
    /// episodes. Returns `Cancelled` when `cancel` is set.
    pub async fn reference_texts(
        &self,
        show: &Show,
        episodes: &[Episode],
        language: &str,
        on_progress: &(dyn Fn(u32, u32) + Send + Sync),
        cancel: &CancelFlag,
    ) -> crate::Result<Vec<ReferenceText>> {
        if cancel.is_cancelled() {
            return Err(SourceError::Cancelled);
        }
        let total = episodes.len() as u32;
        on_progress(0, total);
        let mut out: Vec<ReferenceText> = Vec::new();
        let mut covered: HashSet<(u32, u32)> = HashSet::new();
        for e in episodes {
            let cached = self.cache.texts(&e.show_ref, e.ordering, e.key, None)?;
            for text in cached {
                if is_dialogue(text.kind) {
                    covered.insert((e.key.season, e.key.number));
                    out.push(text);
                }
            }
        }

        let lacking_any = episodes
            .iter()
            .any(|e| !covered.contains(&(e.key.season, e.key.number)));
        let ids = if lacking_any {
            self.show_ids(&show.show_ref).await.unwrap_or_else(|e| {
                tracing::info!(error = %e, "show ids unavailable");
                ShowIds::default()
            })
        } else {
            ShowIds::default()
        };
        let aired = if lacking_any {
            self.aired_keys(episodes).await
        } else {
            HashMap::new()
        };

        let mut providers: Vec<Arc<dyn ReferenceProvider>> = self.extra.clone();
        if let Some(key) = self.key(ProviderId::Subdl) {
            providers.push(Arc::new(Subdl::new(
                self.http.clone(),
                Arc::clone(&self.cache),
                key,
            )));
        }
        providers.push(Arc::new(Lrclib::new(
            self.http.clone(),
            Arc::clone(&self.cache),
        )));
        let mut disabled: HashSet<usize> = HashSet::new();

        let mut seasons: Vec<u32> = episodes.iter().map(|e| e.key.season).collect();
        seasons.sort_unstable();
        seasons.dedup();
        let mut done = 0u32;
        for season in seasons {
            let in_season: Vec<&Episode> =
                episodes.iter().filter(|e| e.key.season == season).collect();
            for (index, provider) in providers.iter().enumerate() {
                if cancel.is_cancelled() {
                    return Err(SourceError::Cancelled);
                }
                if disabled.contains(&index) {
                    continue;
                }
                let lacking: Vec<Episode> = in_season
                    .iter()
                    .filter(|e| !covered.contains(&(e.key.season, e.key.number)))
                    .map(|e| (*e).clone())
                    .collect();
                if lacking.is_empty() {
                    break;
                }
                let request = ReferenceRequest {
                    show,
                    ids: &ids,
                    episodes: &lacking,
                    aired: &aired,
                    language,
                    cancel,
                };
                match provider.reference_texts(&request).await {
                    Ok(texts) => {
                        if provider.id() != ProviderId::Local {
                            self.cache.put_texts(&texts)?;
                        }
                        for t in &texts {
                            covered.insert((t.episode.season, t.episode.number));
                        }
                        out.extend(texts);
                    }
                    Err(SourceError::Cancelled) => return Err(SourceError::Cancelled),
                    Err(e) => {
                        tracing::warn!(
                            provider = provider_name(provider.id()),
                            error = %e,
                            "reference source failed; skipping it"
                        );
                        disabled.insert(index);
                    }
                }
            }
            done += in_season.len() as u32;
            on_progress(done, total);
        }

        for e in episodes {
            if covered.contains(&(e.key.season, e.key.number)) {
                continue;
            }
            if let Some(summary) = e.summary.as_ref().filter(|s| !s.trim().is_empty()) {
                out.push(reference_text(
                    e,
                    TextKind::Summary,
                    e.show_ref.provider,
                    e.provider_episode_id.clone(),
                    summary.clone(),
                    language,
                ));
            }
        }
        out.sort_by(|a, b| {
            (a.episode, kind_rank(a.kind), a.provider, &a.provider_ref).cmp(&(
                b.episode,
                kind_rank(b.kind),
                b.provider,
                &b.provider_ref,
            ))
        });
        Ok(out)
    }

    /// Aired keys by episode id, for episodes in DVD order (empty when none are, or when the
    /// aired list is unavailable).
    async fn aired_keys(&self, episodes: &[Episode]) -> HashMap<String, EpisodeKey> {
        let Some(first) = episodes
            .iter()
            .find(|e| e.ordering != EpisodeOrdering::Aired)
        else {
            return HashMap::new();
        };
        let list = match first.show_ref.provider {
            ProviderId::Tvmaze => {
                self.tvmaze()
                    .episodes(&first.show_ref, EpisodeOrdering::Aired)
                    .await
            }
            ProviderId::Tmdb => match self.tmdb() {
                Some(tmdb) => tmdb.episodes(&first.show_ref, EpisodeOrdering::Aired).await,
                None => Err(SourceError::KeyMissing(ProviderId::Tmdb)),
            },
            _ => Ok(Vec::new()),
        };
        match list {
            Ok(list) => list
                .into_iter()
                .map(|e| (e.provider_episode_id, e.key))
                .collect(),
            Err(e) => {
                tracing::info!(error = %e, "aired numbering unavailable");
                HashMap::new()
            }
        }
    }

    /// Checks a key the user entered, without storing it. `KeyRejected` when the provider
    /// refuses it.
    pub async fn validate_key(&self, provider: ApiKeyProvider, key: &str) -> crate::Result<()> {
        match provider {
            ApiKeyProvider::Subdl => crate::subdl::validate_key(&self.http, key).await,
            ApiKeyProvider::Tmdb => crate::tmdb::validate_key(&self.http, key).await,
        }
    }

    /// Status of each source for the Settings screen, in its order (TVmaze, LRCLIB, SubDL,
    /// TMDb): `NeedsKey` when a key-only source has no key, otherwise from the outcome of the
    /// most recent request (`Ready` before any request).
    pub fn status(&self) -> Vec<SourceStatus> {
        [
            ProviderId::Tvmaze,
            ProviderId::Lrclib,
            ProviderId::Subdl,
            ProviderId::Tmdb,
        ]
        .into_iter()
        .map(|provider| {
            let needs_key = matches!(provider, ProviderId::Subdl | ProviderId::Tmdb);
            let has_key = self.key(provider).is_some();
            let state = if needs_key && !has_key {
                SourceState::NeedsKey
            } else {
                match self.http.last_outcome(provider) {
                    None | Some(Outcome::Ok) => SourceState::Ready,
                    Some(Outcome::KeyRejected) => SourceState::KeyRejected,
                    Some(Outcome::RateLimited) => SourceState::Unavailable {
                        message: format!(
                            "{} is limiting requests. Try again later.",
                            provider_name(provider)
                        ),
                    },
                    Some(Outcome::Failed(_)) => SourceState::Unavailable {
                        message: format!(
                            "Couldn't reach {}. Check your internet connection.",
                            provider_name(provider)
                        ),
                    },
                }
            };
            SourceStatus {
                provider,
                state,
                has_key: needs_key && has_key,
            }
        })
        .collect()
    }
}

fn is_dialogue(kind: TextKind) -> bool {
    matches!(kind, TextKind::Subtitles | TextKind::Lyrics)
}

fn kind_rank(kind: TextKind) -> u8 {
    match kind {
        TextKind::Subtitles => 0,
        TextKind::Lyrics => 1,
        TextKind::Summary => 2,
    }
}
