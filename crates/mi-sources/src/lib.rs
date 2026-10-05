//! Online sources and the local cache.
//!
//! - Episode lists: [`tvmaze`] (default, no key) and [`tmdb`] (optional, user key).
//! - Reference text: [`subdl`] subtitles (user key), [`lrclib`] lyrics (no key), [`embedded`]
//!   text subtitle streams in the files themselves, and episode summaries as a last resort.
//! - [`cache`]: one SQLite database so nothing is downloaded twice.
//! - [`http`]: the shared client (User-Agent, rate-limit headers, back-off on HTTP 429).
//! - [`text`]: turning subtitle files into plain dialogue lines.
//!
//! Only show names, provider ids and episode numbers are sent online; audio and video never
//! leave the computer. Provider terms, limits and attribution are in `docs/sources.md`.
//!
//! Owner: sources module (see `docs/architecture.md`).

pub mod cache;
pub mod embedded;
pub mod http;
pub mod lrclib;
pub mod subdl;
pub mod text;
pub mod tmdb;
pub mod tvmaze;

mod sources;

pub use cache::Cache;
pub use http::{HttpClient, USER_AGENT};
pub use sources::{ApiKeys, Sources};

/// Errors from this crate.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// The provider could not be reached or answered with an unexpected status.
    #[error("{provider:?} request failed: {message}")]
    Network {
        /// Which provider.
        provider: mi_types::ProviderId,
        /// What happened.
        message: String,
    },
    /// The HTTP client could not be built (TLS setup).
    #[error("could not create the HTTP client: {0}")]
    Client(String),
    /// The provider is rate limiting and the retry budget is spent.
    #[error("{0:?} is rate limiting requests")]
    RateLimited(mi_types::ProviderId),
    /// The provider needs a key that is not set.
    #[error("{0:?} needs an API key")]
    KeyMissing(mi_types::ProviderId),
    /// The provider rejected the key.
    #[error("{0:?} rejected the API key")]
    KeyRejected(mi_types::ProviderId),
    /// The response could not be parsed.
    #[error("{provider:?} sent an unexpected response: {message}")]
    BadResponse {
        /// Which provider.
        provider: mi_types::ProviderId,
        /// What was wrong.
        message: String,
    },
    /// The cache database failed.
    #[error("cache error: {0}")]
    Cache(#[from] rusqlite::Error),
    /// A file system error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The operation was cancelled.
    #[error("cancelled")]
    Cancelled,
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, SourceError>;
