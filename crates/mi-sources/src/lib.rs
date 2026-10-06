//! Online sources and the local cache.
//!
//! - Episode lists ([`provider::EpisodeProvider`]): [`tvmaze`] (default, no key) and [`tmdb`]
//!   (optional, user key).
//! - Reference text ([`provider::ReferenceProvider`]): [`subdl`] subtitles (user key),
//!   [`lrclib`] lyrics (no key), [`local`] subtitle files in a folder, and episode summaries as
//!   a last resort; [`embedded`] turns text subtitle streams of the files themselves into text.
//! - [`Sources`]: the facade `mi-core` uses, combining them in order.
//! - [`cache`]: one SQLite database so nothing is downloaded twice.
//! - [`http`]: the shared client (User-Agent, rate-limit headers, back-off on HTTP 429);
//!   [`testing`] holds the recorded-response transport tests use instead of the network.
//! - [`text`]: turning subtitle and lyrics files into plain dialogue lines.
//! - [`mod@attribution`]: the credits the UI shows.
//!
//! Audio, video, transcripts and file paths never leave the computer; what each source receives
//! is listed in `docs/sources.md`, with provider terms, limits and attribution.

pub mod attribution;
pub mod cache;
pub mod embedded;
pub mod fetch;
pub mod http;
pub mod local;
pub mod lrclib;
pub mod names;
pub mod provider;
pub mod subdl;
pub mod testing;
pub mod text;
pub mod tmdb;
pub mod tvmaze;

mod sources;

pub use attribution::{attribution, attributions, provider_name};
pub use cache::Cache;
pub use http::{HttpClient, USER_AGENT};
pub use provider::{EpisodeProvider, ReferenceProvider, ReferenceRequest, ShowIds};
pub use sources::{ApiKeys, Sources};

/// Errors from this crate.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// The provider could not be reached or answered with an unexpected status.
    #[error("{} request failed: {message}", provider_name(*provider))]
    Network {
        /// Which provider.
        provider: mi_types::ProviderId,
        /// What happened.
        message: String,
    },
    /// The provider answered 404 for something it does not have.
    #[error("{} has no {what}", provider_name(*provider))]
    NotFound {
        /// Which provider.
        provider: mi_types::ProviderId,
        /// What was asked for (the public URL).
        what: String,
    },
    /// The HTTP client could not be built (TLS setup).
    #[error("could not create the HTTP client: {0}")]
    Client(String),
    /// The provider is rate limiting and the retry budget is spent.
    #[error("{} is rate limiting requests", provider_name(*.0))]
    RateLimited(mi_types::ProviderId),
    /// The provider needs a key that is not set.
    #[error("{} needs an API key", provider_name(*.0))]
    KeyMissing(mi_types::ProviderId),
    /// The provider rejected the key.
    #[error("{} rejected the API key", provider_name(*.0))]
    KeyRejected(mi_types::ProviderId),
    /// The response could not be parsed, or the provider lacks what was asked for.
    #[error("{} sent an unexpected response: {message}", provider_name(*provider))]
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
