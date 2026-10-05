//! SubDL (<https://subdl.com/api-doc>): subtitles, preferably whole-season packs.
//!
//! The search API needs a key: its documentation lists `api_key` as required, and requests
//! without one are answered HTTP 403 `not_authorized`. Users enter their own free key in
//! Settings (SubDL's terms allow apps where each user brings their own key). A free key allows
//! 2,000 searches a day.
//!
//! Downloads come from `dl.subdl.com` without the key: SubDL counts anonymous downloads per IP
//! address (300 a day), and authenticated downloads only exist on paid plans.
//!
//! For each season the app searches once with `full_season=1&unpack=1`. `unpack=1` makes SubDL
//! list the files inside each archive with their season and episode numbers, so the app can pick
//! the pack that covers the most episodes and download it once as a ZIP. Episodes the pack does
//! not cover are filled from a second, per-episode search of the same season. SubDL numbers
//! episodes by broadcast order, so DVD-ordered episodes are looked up by their aired numbers
//! ([`ReferenceRequest::aired_key`]) and reported under their DVD numbers.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::sync::Arc;

use async_trait::async_trait;
use mi_types::{Episode, ProviderId, ReferenceText, Show, TextKind};
use serde::{Deserialize, Deserializer};

use crate::fetch::{self, Freshness};
use crate::http::{Request, Secret};
use crate::provider::{ReferenceProvider, ReferenceRequest, ShowIds, reference_text};
use crate::{Cache, HttpClient, SourceError, names};

/// Search endpoint (API v1).
pub const SEARCH_URL: &str = "https://api.subdl.com/api/v1/subtitles";
/// Account endpoint, used to check a key without spending a search.
pub const ACCOUNT_URL: &str = "https://api.subdl.com/api/v1/me";
/// Download host; subtitle `url` paths from search results are appended to it.
pub const DOWNLOAD_BASE: &str = "https://dl.subdl.com";

/// Search results are reused for 30 days.
pub const SEARCH_FRESHNESS: Freshness = Freshness::days(30);

/// Archive limits, so a damaged or hostile archive cannot exhaust memory: at most this many
/// entries are read, each at most [`MAX_ENTRY_BYTES`], [`MAX_ARCHIVE_BYTES`] in all.
pub const MAX_ENTRIES: usize = 500;
/// Largest subtitle file read from an archive.
pub const MAX_ENTRY_BYTES: u64 = 5 * 1024 * 1024;
/// Largest total of subtitle bytes read from one archive.
pub const MAX_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024;

/// Subtitle file extensions read from archives, best first.
const EXTENSIONS: &[&str] = &["srt", "ass", "ssa", "vtt"];

/// SubDL's language code for an ISO 639-1 tag (`en` → `EN`).
pub fn language_code(language: &str) -> String {
    match language.to_ascii_lowercase().as_str() {
        "pt-br" => "BR_PT".to_owned(),
        other => other
            .split(['-', '_'])
            .next()
            .unwrap_or("en")
            .to_ascii_uppercase(),
    }
}

/// Checks that a key works (`KeyRejected` otherwise). Never cached.
pub async fn validate_key(http: &HttpClient, key: &str) -> crate::Result<()> {
    let request = Request::get(ProviderId::Subdl, ACCOUNT_URL, &[])?.with_secret(Secret::Query {
        name: "api_key",
        value: key.trim().to_owned(),
    });
    let body = http.send(&request).await?;
    let response: StatusOnly = fetch::parse(ProviderId::Subdl, &body)?;
    if response.status == Some(false) {
        return Err(SourceError::KeyRejected(ProviderId::Subdl));
    }
    Ok(())
}

/// The SubDL reference-text provider.
#[derive(Debug, Clone)]
pub struct Subdl {
    http: HttpClient,
    cache: Arc<Cache>,
    key: String,
}

impl Subdl {
    /// Creates the provider with the user's key.
    pub fn new(http: HttpClient, cache: Arc<Cache>, key: String) -> Self {
        Self { http, cache, key }
    }

    /// Searches one season (cached). `packs` asks for full-season packs only.
    pub async fn search_season(
        &self,
        show: &Show,
        ids: &ShowIds,
        season: u32,
        language: &str,
        packs: bool,
    ) -> crate::Result<SearchResponse> {
        let season_s = season.to_string();
        let languages = language_code(language);
        let year = show.year.map(|y| y.to_string());
        let tmdb = ids.tmdb.map(|t| t.to_string());
        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(imdb) = &ids.imdb {
            params.push(("imdb_id", imdb));
        } else if let Some(tmdb) = &tmdb {
            params.push(("tmdb_id", tmdb));
        } else {
            params.push(("film_name", &show.name));
            if let Some(year) = &year {
                params.push(("year", year));
            }
        }
        params.extend([
            ("type", "tv"),
            ("season_number", season_s.as_str()),
            ("languages", languages.as_str()),
            ("subs_per_page", "30"),
            ("unpack", "1"),
            ("client", "custom_integration"),
        ]);
        if packs {
            params.push(("full_season", "1"));
        }
        let request =
            Request::get(ProviderId::Subdl, SEARCH_URL, &params)?.with_secret(Secret::Query {
                name: "api_key",
                value: self.key.trim().to_owned(),
            });
        let response: SearchResponse =
            fetch::json(&self.http, &self.cache, &request, SEARCH_FRESHNESS).await?;
        if response.status == Some(false) {
            let error = response.error.clone().unwrap_or_default().to_lowercase();
            if error.contains("key") || error.contains("auth") {
                return Err(SourceError::KeyRejected(ProviderId::Subdl));
            }
            // "Not found" answers carry no subtitles; they are cached like any other answer.
            return Ok(SearchResponse::default());
        }
        if ids.imdb.is_none() && tmdb.is_none() && !response.is_show(&show.name, show.year) {
            tracing::info!(show = %show.name, "SubDL's first result is a different show; ignored");
            return Ok(SearchResponse::default());
        }
        Ok(response)
    }

    /// Reference text for `wanted` episodes of one aired `season`; `wanted` maps aired episode
    /// numbers to the episodes to label the text with.
    async fn season_texts(
        &self,
        request: &ReferenceRequest<'_>,
        season: u32,
        wanted: &BTreeMap<u32, &Episode>,
    ) -> crate::Result<Vec<ReferenceText>> {
        let mut found: BTreeMap<u32, (String, String)> = BTreeMap::new();

        let packs = self
            .search_season(request.show, request.ids, season, request.language, true)
            .await?;
        if let Some(pack) = best_pack(&packs.subtitles, season, wanted) {
            request.check_cancel()?;
            let archive = self.download(&pack.url).await?;
            let unpacked = pack.unpack_names();
            for (name, bytes) in extract_subtitles(&archive)? {
                let listed = unpacked.get(&basename(&name).to_lowercase()).copied();
                let Some((s, e)) = listed.or_else(|| names::episode_marker(&name)) else {
                    continue;
                };
                if s == season && wanted.contains_key(&e) && !found.contains_key(&e) {
                    let text =
                        crate::text::subtitle_to_dialogue(&crate::text::decode_bytes(&bytes));
                    if !text.is_empty() {
                        found.insert(e, (format!("{}#{}", pack.url, name), text));
                    }
                }
            }
        }

        let missing: Vec<u32> = wanted
            .keys()
            .copied()
            .filter(|e| !found.contains_key(e))
            .collect();
        if !missing.is_empty() {
            let singles = self
                .search_season(request.show, request.ids, season, request.language, false)
                .await?;
            for episode in missing {
                request.check_cancel()?;
                let Some(choice) = single_choice(&singles.subtitles, season, episode) else {
                    continue;
                };
                let (provider_ref, bytes) = match choice {
                    Single::Raw(url) => (url.clone(), self.download(&url).await?),
                    Single::Archive(url) => {
                        let archive = self.download(&url).await?;
                        let entries = extract_subtitles(&archive)?;
                        let pick = entries
                            .iter()
                            .find(|(n, _)| names::episode_marker(n) == Some((season, episode)))
                            .or_else(|| (entries.len() == 1).then(|| &entries[0]));
                        match pick {
                            Some((name, bytes)) => (format!("{url}#{name}"), bytes.clone()),
                            None => continue,
                        }
                    }
                };
                let text = crate::text::subtitle_to_dialogue(&crate::text::decode_bytes(&bytes));
                if !text.is_empty() {
                    found.insert(episode, (provider_ref, text));
                }
            }
        }

        Ok(found
            .into_iter()
            .filter_map(|(number, (provider_ref, text))| {
                wanted.get(&number).map(|episode| {
                    reference_text(
                        episode,
                        TextKind::Subtitles,
                        ProviderId::Subdl,
                        provider_ref,
                        text,
                        request.language,
                    )
                })
            })
            .collect())
    }

    /// Downloads `path` from [`DOWNLOAD_BASE`] (cached forever: subtitle files do not change).
    async fn download(&self, path: &str) -> crate::Result<Vec<u8>> {
        let url = if path.starts_with("http") {
            path.to_owned()
        } else {
            format!("{DOWNLOAD_BASE}{path}")
        };
        let request = Request::get(ProviderId::Subdl, &url, &[])?;
        fetch::bytes(&self.http, &self.cache, &request, Freshness::FOREVER).await
    }
}

#[async_trait]
impl ReferenceProvider for Subdl {
    fn id(&self) -> ProviderId {
        ProviderId::Subdl
    }

    async fn reference_texts(
        &self,
        request: &ReferenceRequest<'_>,
    ) -> crate::Result<Vec<ReferenceText>> {
        let mut seasons: BTreeMap<u32, BTreeMap<u32, &Episode>> = BTreeMap::new();
        for episode in request.episodes {
            let key = request.aired_key(episode);
            seasons
                .entry(key.season)
                .or_default()
                .insert(key.number, episode);
        }
        let mut out = Vec::new();
        for (season, wanted) in seasons {
            request.check_cancel()?;
            out.extend(self.season_texts(request, season, &wanted).await?);
        }
        Ok(out)
    }
}

/// A SubDL search response (fields the app uses).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchResponse {
    /// `true` on success.
    #[serde(default)]
    pub status: Option<bool>,
    /// Failure reason when `status` is false.
    #[serde(default)]
    pub error: Option<String>,
    /// Matching titles; subtitles belong to the first.
    #[serde(default)]
    pub results: Vec<SearchTitle>,
    /// Subtitles of the first title.
    #[serde(default)]
    pub subtitles: Vec<Subtitle>,
}

impl SearchResponse {
    /// Whether the first result is the show (same name, first-air year within one year).
    fn is_show(&self, name: &str, year: Option<u16>) -> bool {
        let Some(first) = self.results.first() else {
            return true;
        };
        let name_ok = names::same_title(&first.name, name);
        let year_ok = match (year, first.year) {
            (Some(a), Some(b)) => (i64::from(a) - i64::from(b)).abs() <= 1,
            _ => true,
        };
        name_ok && year_ok
    }
}

/// A title in SubDL's results.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchTitle {
    /// Title.
    #[serde(default)]
    pub name: String,
    /// Release or first-air year.
    #[serde(default, deserialize_with = "lenient_u32")]
    pub year: Option<u32>,
}

/// One subtitle (single file or pack) in SubDL's results.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Subtitle {
    /// Download path, for example `/subtitle/3197651-3213944.zip`.
    #[serde(default)]
    pub url: String,
    /// Release name.
    #[serde(default)]
    pub release_name: Option<String>,
    /// Season.
    #[serde(default, deserialize_with = "lenient_u32")]
    pub season: Option<u32>,
    /// Episode (single-episode subtitles).
    #[serde(default, deserialize_with = "lenient_u32")]
    pub episode: Option<u32>,
    /// First episode covered by a pack.
    #[serde(default, deserialize_with = "lenient_u32")]
    pub episode_from: Option<u32>,
    /// Last episode covered by a pack.
    #[serde(default, deserialize_with = "lenient_u32")]
    pub episode_end: Option<u32>,
    /// Whether this is a full-season pack.
    #[serde(default)]
    pub full_season: Option<bool>,
    /// Hearing-impaired subtitles.
    #[serde(default)]
    pub hi: Option<bool>,
    /// Files inside the archive (present with `unpack=1`).
    #[serde(default)]
    pub unpack_files: Vec<UnpackFile>,
}

/// One file inside a SubDL archive.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnpackFile {
    /// File name inside the archive.
    #[serde(default)]
    pub name: String,
    /// Season.
    #[serde(default, deserialize_with = "lenient_u32")]
    pub season: Option<u32>,
    /// Episode.
    #[serde(default, deserialize_with = "lenient_u32")]
    pub episode: Option<u32>,
    /// Hearing-impaired.
    #[serde(default)]
    pub hi: Option<bool>,
    /// `srt`, `ass`, ...
    #[serde(default)]
    pub format: Option<String>,
    /// Raw-file download path, `/subtitle/{n_id}/{file_n_id}`.
    #[serde(default)]
    pub url: String,
}

impl Subtitle {
    fn is_pack(&self) -> bool {
        self.full_season == Some(true) || self.unpack_files.len() > 1
    }

    /// Lower-case base names of unpacked files → (season, episode).
    fn unpack_names(&self) -> HashMap<String, (u32, u32)> {
        self.unpack_files
            .iter()
            .filter_map(|f| {
                let season = f.season.or(self.season)?;
                Some((basename(&f.name).to_lowercase(), (season, f.episode?)))
            })
            .collect()
    }

    /// The episodes of `season` this subtitle covers.
    fn covers(&self, season: u32) -> Vec<u32> {
        if !self.unpack_files.is_empty() {
            return self
                .unpack_files
                .iter()
                .filter(|f| f.season.or(self.season) == Some(season))
                .filter_map(|f| f.episode)
                .collect();
        }
        if self.season != Some(season) {
            return Vec::new();
        }
        match (self.episode_from, self.episode_end, self.episode) {
            (Some(a), Some(b), _) if a <= b && b - a < 500 => (a..=b).collect(),
            (_, _, Some(e)) => vec![e],
            _ => Vec::new(),
        }
    }
}

/// The pack covering the most wanted episodes (ties: not hearing-impaired, then first listed).
fn best_pack<'a>(
    subtitles: &'a [Subtitle],
    season: u32,
    wanted: &BTreeMap<u32, &Episode>,
) -> Option<&'a Subtitle> {
    subtitles
        .iter()
        .filter(|s| s.is_pack() && !s.url.is_empty())
        .map(|s| {
            let covered = s
                .covers(season)
                .into_iter()
                .filter(|e| wanted.contains_key(e))
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            (s, covered)
        })
        .filter(|(_, covered)| *covered > 0)
        .enumerate()
        .max_by(|(ia, (a, ca)), (ib, (b, cb))| {
            ca.cmp(cb)
                .then((b.hi == Some(true)).cmp(&(a.hi == Some(true))))
                .then(ib.cmp(ia))
        })
        .map(|(_, (s, _))| s)
}

enum Single {
    /// A raw subtitle file (`unpack_files[].url`).
    Raw(String),
    /// An archive holding the episode.
    Archive(String),
}

/// How to get one episode from a per-episode search: a raw unpacked file when SubDL lists one,
/// else the archive of a subtitle for exactly that episode. Not-hearing-impaired first.
fn single_choice(subtitles: &[Subtitle], season: u32, episode: u32) -> Option<Single> {
    let mut candidates: Vec<(bool, Single)> = Vec::new();
    for s in subtitles {
        for f in &s.unpack_files {
            if f.season.or(s.season) == Some(season)
                && f.episode == Some(episode)
                && !f.url.is_empty()
            {
                let hi = f.hi.or(s.hi) == Some(true);
                candidates.push((hi, Single::Raw(f.url.clone())));
            }
        }
        if !s.is_pack()
            && s.season == Some(season)
            && s.episode == Some(episode)
            && !s.url.is_empty()
        {
            candidates.push((s.hi == Some(true), Single::Archive(s.url.clone())));
        }
    }
    let index = candidates
        .iter()
        .position(|(hi, _)| !hi)
        .or((!candidates.is_empty()).then_some(0))?;
    Some(candidates.swap_remove(index).1)
}

/// The subtitle files in a ZIP archive as `(name, bytes)`, best format first for equal names.
/// A payload that is not a ZIP is returned as one unnamed file (raw downloads).
pub fn extract_subtitles(payload: &[u8]) -> crate::Result<Vec<(String, Vec<u8>)>> {
    if !payload.starts_with(b"PK") {
        return Ok(vec![(String::new(), payload.to_vec())]);
    }
    let bad = |message: String| SourceError::BadResponse {
        provider: ProviderId::Subdl,
        message,
    };
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(payload))
        .map_err(|e| bad(format!("unreadable subtitle archive: {e}")))?;
    let mut out = Vec::new();
    let mut total = 0u64;
    for i in 0..archive.len().min(MAX_ENTRIES) {
        let mut file = archive
            .by_index(i)
            .map_err(|e| bad(format!("unreadable archive entry: {e}")))?;
        let name = file.name().to_owned();
        if file.is_dir() || name.starts_with("__MACOSX/") || basename(&name).starts_with("._") {
            continue;
        }
        let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        if !EXTENSIONS.contains(&ext.as_str()) || file.size() > MAX_ENTRY_BYTES {
            continue;
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_ENTRY_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|e| bad(format!("unreadable archive entry: {e}")))?;
        total += bytes.len() as u64;
        if total > MAX_ARCHIVE_BYTES {
            break;
        }
        out.push((name, bytes));
    }
    out.sort_by_key(|(name, _)| {
        let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        EXTENSIONS
            .iter()
            .position(|e| *e == ext)
            .unwrap_or(EXTENSIONS.len())
    });
    Ok(out)
}

fn basename(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

#[derive(Debug, Deserialize)]
struct StatusOnly {
    #[serde(default)]
    status: Option<bool>,
}

/// Accepts a number, a numeric string, or null/empty.
fn lenient_u32<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(d)?;
    Ok(match value {
        Some(serde_json::Value::Number(n)) => n.as_u64().and_then(|v| u32::try_from(v).ok()),
        Some(serde_json::Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    pub(crate) fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, content) in files {
                w.start_file(*name, options).unwrap();
                w.write_all(content.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn language_codes() {
        assert_eq!(language_code("en"), "EN");
        assert_eq!(language_code("fr-CA"), "FR");
        assert_eq!(language_code("pt-BR"), "BR_PT");
    }

    #[test]
    fn archives_yield_subtitle_files_only() {
        let zip = zip_of(&[
            ("Show.S01E01.srt", "1\n00:00:01,000 --> 00:00:02,000\nHi\n"),
            ("__MACOSX/._Show.S01E01.srt", "junk"),
            ("readme.nfo", "release notes"),
            ("subs/Show.S01E02.ass", "[Events]\n"),
        ]);
        let files = extract_subtitles(&zip).unwrap();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["Show.S01E01.srt", "subs/Show.S01E02.ass"]);
        let raw = extract_subtitles(b"1\n00:00:01,000 --> 00:00:02,000\nHi\n").unwrap();
        assert_eq!(raw.len(), 1);
    }

    #[test]
    fn numbers_may_arrive_as_strings() {
        let s: Subtitle = serde_json::from_str(
            r#"{"url":"/x.zip","season":"2","episode":null,"episode_from":1,"episode_end":"3"}"#,
        )
        .unwrap();
        assert_eq!(s.season, Some(2));
        assert_eq!(s.covers(2), vec![1, 2, 3]);
        assert!(s.covers(1).is_empty());
    }
}
