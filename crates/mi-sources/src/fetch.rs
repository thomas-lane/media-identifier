//! Cache-first fetching shared by the providers.

use mi_types::ProviderId;
use serde::de::DeserializeOwned;

use crate::http::Request;
use crate::{Cache, HttpClient, SourceError};

/// One day in milliseconds.
pub const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// How long a cached response is used without asking the provider again.
#[derive(Debug, Clone, Copy)]
pub struct Freshness {
    /// Younger responses are used without a request.
    pub ttl_ms: i64,
    /// When the provider cannot be reached, older responses are still used up to this age
    /// (`None`: any age). TMDb sets this to six months, as its terms require.
    pub stale_limit_ms: Option<i64>,
}

impl Freshness {
    /// Fresh for `days`, usable at any age when offline.
    pub const fn days(days: i64) -> Self {
        Self {
            ttl_ms: days * DAY_MS,
            stale_limit_ms: None,
        }
    }

    /// Never refreshed (immutable downloads such as subtitle files).
    pub const FOREVER: Self = Self {
        ttl_ms: i64::MAX,
        stale_limit_ms: None,
    };
}

/// The body for `request`: from the cache when fresh, else from the provider (then cached).
/// When the provider cannot be reached or is rate limiting, a stale cached body within
/// `freshness.stale_limit_ms` is returned instead, so a known show still works offline.
pub async fn bytes(
    http: &HttpClient,
    cache: &Cache,
    request: &Request,
    freshness: Freshness,
) -> crate::Result<Vec<u8>> {
    Ok(bytes_from(http, cache, request, freshness).await?.0)
}

/// [`bytes`], also saying whether the body came from the cache.
async fn bytes_from(
    http: &HttpClient,
    cache: &Cache,
    request: &Request,
    freshness: Freshness,
) -> crate::Result<(Vec<u8>, bool)> {
    let provider = request.provider;
    let key = request.public_url();
    let cached = cache.response_with_time(provider, key)?;
    let now = crate::cache::now_ms();
    if let Some((body, fetched)) = &cached
        && now.saturating_sub(*fetched) <= freshness.ttl_ms
    {
        return Ok((body.clone(), true));
    }
    match http.send(request).await {
        Ok(body) => {
            cache.put_response(provider, key, &body)?;
            Ok((body, false))
        }
        Err(e @ (SourceError::Network { .. } | SourceError::RateLimited(_))) => match cached {
            Some((body, fetched))
                if freshness
                    .stale_limit_ms
                    .is_none_or(|limit| now.saturating_sub(fetched) <= limit) =>
            {
                tracing::info!(
                    ?provider,
                    url = key,
                    "provider unavailable; using cached copy"
                );
                Ok((body, true))
            }
            _ => Err(e),
        },
        Err(e) => Err(e),
    }
}

/// Like [`bytes`], parsed as JSON into `T`. A cached body that no longer parses (the provider
/// changed its format) is fetched again once.
pub async fn json<T: DeserializeOwned>(
    http: &HttpClient,
    cache: &Cache,
    request: &Request,
    freshness: Freshness,
) -> crate::Result<T> {
    let (body, from_cache) = bytes_from(http, cache, request, freshness).await?;
    match parse(request.provider, &body) {
        Ok(v) => Ok(v),
        Err(first) if !from_cache => Err(first),
        Err(first) => {
            let body = match http.send(request).await {
                Ok(b) => b,
                Err(_) => return Err(first),
            };
            let value = parse(request.provider, &body)?;
            cache.put_response(request.provider, request.public_url(), &body)?;
            Ok(value)
        }
    }
}

/// Parses a JSON body, mapping failures to `BadResponse`.
pub fn parse<T: DeserializeOwned>(provider: ProviderId, body: &[u8]) -> crate::Result<T> {
    serde_json::from_slice(body).map_err(|e| SourceError::BadResponse {
        provider,
        message: e.to_string(),
    })
}
