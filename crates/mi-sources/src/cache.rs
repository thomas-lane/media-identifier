//! The local SQLite cache.

use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use mi_types::{EpisodeKey, EpisodeOrdering, ProviderId, ReferenceText, ShowRef, TextKind};
use rusqlite::{OptionalExtension, params};

/// Schema version stored in `PRAGMA user_version`.
const SCHEMA_VERSION: i64 = 1;

/// The current time as Unix milliseconds.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// One SQLite database (`<app data>/cache.sqlite`) holding provider responses and normalised
/// reference text.
///
/// Tables (created by [`Cache::open`], versioned with `PRAGMA user_version`):
/// - `responses(provider, key, fetched_at_ms, body)`: raw response bodies (JSON, subtitle files,
///   subtitle archives) keyed by provider and public URL. Secrets never enter the key.
/// - `texts(provider, provider_ref, show_provider, show_id, ordering, season, episode, kind,
///   language, text, fetched_at_ms)`: normalised reference text, unique per
///   `(provider, provider_ref, show_provider, show_id, ordering, season, episode, kind)`.
///
/// Opening the cache deletes TMDb rows older than [`crate::tmdb::MAX_CACHE_AGE_MS`], because
/// TMDb's terms allow caching for at most six months.
///
/// The connection is behind a mutex because `rusqlite::Connection` is not `Sync`; every method
/// holds it for one statement or transaction. Statements are small and local, so the async code
/// calls them directly instead of moving them to a blocking thread.
#[derive(Debug)]
pub struct Cache {
    conn: Mutex<rusqlite::Connection>,
}

impl Cache {
    /// Opens (creating and migrating) the database at `path`, creating its folder.
    ///
    /// A file that is not a SQLite database (damaged, or written by something else) is renamed to
    /// `<name>.damaged-<unix ms>` and a new cache is created, because everything in the cache can
    /// be downloaded again.
    pub fn open(path: &Path) -> crate::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match Self::open_existing(path) {
            Ok(cache) => Ok(cache),
            Err(crate::SourceError::Cache(e)) if is_not_a_database(&e) => {
                let mut aside = path.as_os_str().to_owned();
                aside.push(format!(".damaged-{}", now_ms()));
                tracing::warn!(?path, "cache database is damaged; setting it aside");
                std::fs::rename(path, &aside)?;
                Self::open_existing(path)
            }
            Err(e) => Err(e),
        }
    }

    fn open_existing(path: &Path) -> crate::Result<Self> {
        let conn = rusqlite::Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::from_connection(conn)
    }

    /// Opens an in-memory database (tests).
    pub fn in_memory() -> crate::Result<Self> {
        Self::from_connection(rusqlite::Connection::open_in_memory()?)
    }

    fn from_connection(conn: rusqlite::Connection) -> crate::Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < SCHEMA_VERSION {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS responses (
                     provider TEXT NOT NULL,
                     key TEXT NOT NULL,
                     fetched_at_ms INTEGER NOT NULL,
                     body BLOB NOT NULL,
                     PRIMARY KEY (provider, key)
                 );
                 CREATE TABLE IF NOT EXISTS texts (
                     provider TEXT NOT NULL,
                     provider_ref TEXT NOT NULL,
                     show_provider TEXT NOT NULL,
                     show_id TEXT NOT NULL,
                     ordering TEXT NOT NULL,
                     season INTEGER NOT NULL,
                     episode INTEGER NOT NULL,
                     kind TEXT NOT NULL,
                     language TEXT NOT NULL,
                     text TEXT NOT NULL,
                     fetched_at_ms INTEGER NOT NULL,
                     PRIMARY KEY (provider, provider_ref, show_provider, show_id, ordering,
                                  season, episode, kind)
                 );
                 CREATE INDEX IF NOT EXISTS texts_by_episode
                     ON texts (show_provider, show_id, ordering, season, episode);
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }
        let cache = Self {
            conn: Mutex::new(conn),
        };
        cache.purge_expired(now_ms())?;
        Ok(cache)
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, rusqlite::Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Deletes rows that may no longer be kept: TMDb responses and texts fetched more than
    /// [`crate::tmdb::MAX_CACHE_AGE_MS`] before `now_ms`. Returns the number of rows deleted.
    pub fn purge_expired(&self, now_ms: i64) -> crate::Result<usize> {
        let cutoff = now_ms - crate::tmdb::MAX_CACHE_AGE_MS;
        let tmdb = provider_str(ProviderId::Tmdb);
        let conn = self.conn();
        let a = conn.execute(
            "DELETE FROM responses WHERE provider = ?1 AND fetched_at_ms < ?2",
            params![tmdb, cutoff],
        )?;
        let b = conn.execute(
            "DELETE FROM texts WHERE provider = ?1 AND fetched_at_ms < ?2",
            params![tmdb, cutoff],
        )?;
        Ok(a + b)
    }

    /// A cached response body, if younger than `max_age_ms`.
    pub fn response(
        &self,
        provider: ProviderId,
        key: &str,
        max_age_ms: i64,
    ) -> crate::Result<Option<Vec<u8>>> {
        Ok(self
            .response_with_time(provider, key)?
            .filter(|(_, fetched)| now_ms() - fetched <= max_age_ms)
            .map(|(body, _)| body))
    }

    /// A cached response body of any age, with the time it was fetched (Unix ms).
    pub fn response_with_time(
        &self,
        provider: ProviderId,
        key: &str,
    ) -> crate::Result<Option<(Vec<u8>, i64)>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT body, fetched_at_ms FROM responses WHERE provider = ?1 AND key = ?2",
                params![provider_str(provider), key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// Stores a response body, fetched now.
    pub fn put_response(&self, provider: ProviderId, key: &str, body: &[u8]) -> crate::Result<()> {
        self.put_response_at(provider, key, body, now_ms())
    }

    /// Stores a response body with an explicit fetch time (tests and imports).
    pub fn put_response_at(
        &self,
        provider: ProviderId,
        key: &str,
        body: &[u8],
        fetched_at_ms: i64,
    ) -> crate::Result<()> {
        self.conn().execute(
            "INSERT OR REPLACE INTO responses (provider, key, fetched_at_ms, body)
             VALUES (?1, ?2, ?3, ?4)",
            params![provider_str(provider), key, fetched_at_ms, body],
        )?;
        Ok(())
    }

    /// Cached reference texts for one episode, any provider, filtered by `kind` when given.
    /// Ordered by provider and provider reference, so results are stable.
    pub fn texts(
        &self,
        show: &ShowRef,
        ordering: EpisodeOrdering,
        episode: EpisodeKey,
        kind: Option<TextKind>,
    ) -> crate::Result<Vec<ReferenceText>> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT provider, provider_ref, kind, language, text, fetched_at_ms FROM texts
             WHERE show_provider = ?1 AND show_id = ?2 AND ordering = ?3
               AND season = ?4 AND episode = ?5
             ORDER BY provider, provider_ref, kind",
        )?;
        let rows = stmt.query_map(
            params![
                provider_str(show.provider),
                show.id,
                ordering_str(ordering),
                episode.season,
                episode.number
            ],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            let (provider, provider_ref, row_kind, language, text, fetched_at_ms) = row?;
            let (Some(provider), Some(row_kind)) =
                (parse_provider(&provider), parse_kind(&row_kind))
            else {
                continue;
            };
            if kind.is_some_and(|k| k != row_kind) {
                continue;
            }
            out.push(ReferenceText {
                show_ref: show.clone(),
                ordering,
                episode,
                kind: row_kind,
                provider,
                provider_ref,
                text,
                language,
                fetched_at_ms,
            });
        }
        Ok(out)
    }

    /// Stores reference texts (replacing rows with the same unique key) in one transaction.
    pub fn put_texts(&self, texts: &[ReferenceText]) -> crate::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO texts (provider, provider_ref, show_provider, show_id,
                     ordering, season, episode, kind, language, text, fetched_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            for t in texts {
                stmt.execute(params![
                    provider_str(t.provider),
                    t.provider_ref,
                    provider_str(t.show_ref.provider),
                    t.show_ref.id,
                    ordering_str(t.ordering),
                    t.episode.season,
                    t.episode.number,
                    kind_str(t.kind),
                    t.language,
                    t.text,
                    t.fetched_at_ms
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

fn is_not_a_database(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt)
    )
}

/// The stable name stored for a provider (the same as its JSON name).
pub fn provider_str(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Tvmaze => "tvmaze",
        ProviderId::Tmdb => "tmdb",
        ProviderId::Subdl => "subdl",
        ProviderId::Lrclib => "lrclib",
        ProviderId::Embedded => "embedded",
        ProviderId::Local => "local",
    }
}

fn parse_provider(s: &str) -> Option<ProviderId> {
    [
        ProviderId::Tvmaze,
        ProviderId::Tmdb,
        ProviderId::Subdl,
        ProviderId::Lrclib,
        ProviderId::Embedded,
        ProviderId::Local,
    ]
    .into_iter()
    .find(|p| provider_str(*p) == s)
}

fn ordering_str(ordering: EpisodeOrdering) -> &'static str {
    match ordering {
        EpisodeOrdering::Aired => "aired",
        EpisodeOrdering::Dvd => "dvd",
    }
}

fn kind_str(kind: TextKind) -> &'static str {
    match kind {
        TextKind::Subtitles => "subtitles",
        TextKind::Lyrics => "lyrics",
        TextKind::Summary => "summary",
    }
}

fn parse_kind(s: &str) -> Option<TextKind> {
    [TextKind::Subtitles, TextKind::Lyrics, TextKind::Summary]
        .into_iter()
        .find(|k| kind_str(*k) == s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show() -> ShowRef {
        ShowRef {
            provider: ProviderId::Tvmaze,
            id: "15448".into(),
        }
    }

    fn text(provider: ProviderId, r: &str, kind: TextKind, body: &str) -> ReferenceText {
        ReferenceText {
            show_ref: show(),
            ordering: EpisodeOrdering::Aired,
            episode: EpisodeKey {
                season: 1,
                number: 2,
            },
            kind,
            provider,
            provider_ref: r.into(),
            text: body.into(),
            language: "en".into(),
            fetched_at_ms: 1,
        }
    }

    #[test]
    fn responses_respect_max_age() {
        let cache = Cache::in_memory().unwrap();
        cache
            .put_response_at(ProviderId::Tvmaze, "k", b"old", now_ms() - 10_000)
            .unwrap();
        assert_eq!(
            cache.response(ProviderId::Tvmaze, "k", 60_000).unwrap(),
            Some(b"old".to_vec())
        );
        assert_eq!(
            cache.response(ProviderId::Tvmaze, "k", 5_000).unwrap(),
            None
        );
        assert_eq!(
            cache.response(ProviderId::Lrclib, "k", 60_000).unwrap(),
            None
        );
        cache.put_response(ProviderId::Tvmaze, "k", b"new").unwrap();
        assert_eq!(
            cache.response(ProviderId::Tvmaze, "k", 5_000).unwrap(),
            Some(b"new".to_vec())
        );
    }

    #[test]
    fn texts_round_trip_replace_and_filter_by_kind() {
        let cache = Cache::in_memory().unwrap();
        cache
            .put_texts(&[
                text(ProviderId::Subdl, "a", TextKind::Subtitles, "hello"),
                text(ProviderId::Lrclib, "9", TextKind::Lyrics, "la la"),
            ])
            .unwrap();
        cache
            .put_texts(&[text(
                ProviderId::Subdl,
                "a",
                TextKind::Subtitles,
                "hello again",
            )])
            .unwrap();
        let key = EpisodeKey {
            season: 1,
            number: 2,
        };
        let all = cache
            .texts(&show(), EpisodeOrdering::Aired, key, None)
            .unwrap();
        assert_eq!(all.len(), 2);
        let subs = cache
            .texts(
                &show(),
                EpisodeOrdering::Aired,
                key,
                Some(TextKind::Subtitles),
            )
            .unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].text, "hello again");
        assert_eq!(
            subs[0],
            text(ProviderId::Subdl, "a", TextKind::Subtitles, "hello again")
        );
        assert!(
            cache
                .texts(&show(), EpisodeOrdering::Dvd, key, None)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn tmdb_rows_older_than_six_months_are_purged_and_others_kept() {
        let cache = Cache::in_memory().unwrap();
        let now = now_ms();
        let old = now - crate::tmdb::MAX_CACHE_AGE_MS - 1;
        cache
            .put_response_at(ProviderId::Tmdb, "old", b"x", old)
            .unwrap();
        cache
            .put_response_at(ProviderId::Tmdb, "recent", b"x", now - 1000)
            .unwrap();
        cache
            .put_response_at(ProviderId::Tvmaze, "old", b"x", old)
            .unwrap();
        let mut t = text(ProviderId::Tmdb, "1", TextKind::Summary, "s");
        t.fetched_at_ms = old;
        cache.put_texts(&[t]).unwrap();
        assert_eq!(cache.purge_expired(now).unwrap(), 2);
        assert!(
            cache
                .response_with_time(ProviderId::Tmdb, "old")
                .unwrap()
                .is_none()
        );
        assert!(
            cache
                .response_with_time(ProviderId::Tmdb, "recent")
                .unwrap()
                .is_some()
        );
        assert!(
            cache
                .response_with_time(ProviderId::Tvmaze, "old")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn open_persists_and_sets_a_damaged_file_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("cache.sqlite");
        {
            let cache = Cache::open(&path).unwrap();
            cache.put_response(ProviderId::Tvmaze, "k", b"v").unwrap();
        }
        let cache = Cache::open(&path).unwrap();
        assert_eq!(
            cache.response(ProviderId::Tvmaze, "k", 60_000).unwrap(),
            Some(b"v".to_vec())
        );
        drop(cache);

        let bad = dir.path().join("bad.sqlite");
        std::fs::write(
            &bad,
            b"this is not a database at all, just some text bytes....",
        )
        .unwrap();
        let cache = Cache::open(&bad).unwrap();
        cache.put_response(ProviderId::Tvmaze, "k", b"v").unwrap();
        let aside: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("bad.sqlite.damaged-")
            })
            .collect();
        assert_eq!(aside.len(), 1);
    }
}
