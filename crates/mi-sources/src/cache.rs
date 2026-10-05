//! The local SQLite cache.

use std::path::Path;
use std::sync::Mutex;

use mi_types::{EpisodeKey, EpisodeOrdering, ProviderId, ReferenceText, ShowRef, TextKind};

/// One SQLite database (`<app data>/cache.sqlite`) holding provider responses and normalised
/// reference text.
///
/// Tables (created by [`Cache::open`], versioned with `PRAGMA user_version`):
/// - `responses(provider, key, fetched_at_ms, body)`: raw JSON responses keyed by request.
/// - `texts(provider, provider_ref, show_provider, show_id, ordering, season, episode, kind,
///   language, text, fetched_at_ms)`: normalised reference text, unique per
///   `(provider, provider_ref, season, episode)`.
///
/// The connection is behind a mutex because `rusqlite::Connection` is not `Sync`; every method
/// holds it only for one statement or transaction.
#[derive(Debug)]
pub struct Cache {
    conn: Mutex<rusqlite::Connection>,
}

impl Cache {
    /// Opens (creating and migrating) the database at `path`.
    pub fn open(path: &Path) -> crate::Result<Self> {
        let _ = path;
        todo!("sources module: open and migrate SQLite")
    }

    /// Opens an in-memory database (tests).
    pub fn in_memory() -> crate::Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// A cached response body, if younger than `max_age_ms`.
    pub fn response(
        &self,
        provider: ProviderId,
        key: &str,
        max_age_ms: i64,
    ) -> crate::Result<Option<Vec<u8>>> {
        let _ = (&self.conn, provider, key, max_age_ms);
        todo!("sources module: cached response lookup")
    }

    /// Stores a response body.
    pub fn put_response(&self, provider: ProviderId, key: &str, body: &[u8]) -> crate::Result<()> {
        let _ = (provider, key, body);
        todo!("sources module: store response")
    }

    /// Cached reference texts for one episode, any provider, filtered by `kind` when given.
    pub fn texts(
        &self,
        show: &ShowRef,
        ordering: EpisodeOrdering,
        episode: EpisodeKey,
        kind: Option<TextKind>,
    ) -> crate::Result<Vec<ReferenceText>> {
        let _ = (show, ordering, episode, kind);
        todo!("sources module: cached text lookup")
    }

    /// Stores reference texts (replacing rows with the same unique key).
    pub fn put_texts(&self, texts: &[ReferenceText]) -> crate::Result<()> {
        let _ = texts;
        todo!("sources module: store texts")
    }
}
