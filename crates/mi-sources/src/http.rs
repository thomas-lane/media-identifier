//! The shared HTTP client: User-Agent, per-provider spacing, rate-limit headers and retries.
//!
//! Every online request of the app goes through [`HttpClient::send`]. Requests are described by
//! [`Request`], which keeps secrets (API keys) apart from the public URL so that the public URL
//! can be logged and used as a cache key without exposing a key. The bytes travel through a
//! [`Transport`]: [`ReqwestTransport`] in the app, [`crate::testing::FixtureTransport`] in tests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use mi_types::ProviderId;
use reqwest::Url;
use tokio::time::Instant;

/// `User-Agent` sent with every request: the app name and version, and the project page.
///
/// TVmaze asks for a User-Agent that identifies the application, and LRCLIB requires the
/// application's name, version and a link to its homepage.
pub const USER_AGENT: &str = concat!(
    "MediaIdentifier/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/thomas-lane/media-identifier)"
);

/// A secret attached to a request. It is added only when the request is sent; it never appears
/// in [`Request::public_url`], logs, cache keys or `Debug` output.
#[derive(Clone)]
pub enum Secret {
    /// A query parameter, such as SubDL's or TMDb v3's `api_key`.
    Query {
        /// Parameter name.
        name: &'static str,
        /// Parameter value.
        value: String,
    },
    /// An `Authorization: Bearer` header (TMDb v4 read access token).
    Bearer(String),
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Secret::Query { name, .. } => write!(f, "Query({name}=<hidden>)"),
            Secret::Bearer(_) => write!(f, "Bearer(<hidden>)"),
        }
    }
}

/// Largest response body accepted by default: 16 MiB, after decompression. Episode lists and
/// search results are far smaller; the limit keeps a broken or hostile server (or a compressed
/// "bomb") from exhausting memory, since bodies are held in memory and cached.
pub const MAX_BODY_BYTES: u64 = 16 * 1024 * 1024;

/// One GET request.
#[derive(Debug, Clone)]
pub struct Request {
    /// The provider the request is for (selects rate limits and error mapping).
    pub provider: ProviderId,
    url: Url,
    secret: Option<Secret>,
    max_body_bytes: u64,
}

impl Request {
    /// A GET of `base` with query `params` (percent-encoded in order).
    pub fn get(provider: ProviderId, base: &str, params: &[(&str, &str)]) -> crate::Result<Self> {
        let url = if params.is_empty() {
            Url::parse(base)
        } else {
            Url::parse_with_params(base, params)
        }
        .map_err(|e| crate::SourceError::BadResponse {
            provider,
            message: format!("invalid URL {base}: {e}"),
        })?;
        Ok(Self {
            provider,
            url,
            secret: None,
            max_body_bytes: MAX_BODY_BYTES,
        })
    }

    /// Sets the largest body accepted, after decompression (default [`MAX_BODY_BYTES`]).
    pub fn with_max_body(mut self, bytes: u64) -> Self {
        self.max_body_bytes = bytes;
        self
    }

    /// The largest body accepted, after decompression.
    pub fn max_body_bytes(&self) -> u64 {
        self.max_body_bytes
    }

    /// Attaches a secret.
    pub fn with_secret(mut self, secret: Secret) -> Self {
        self.secret = Some(secret);
        self
    }

    /// Whether a secret is attached.
    pub fn has_secret(&self) -> bool {
        self.secret.is_some()
    }

    /// The URL without the secret: safe to log and used as the cache key.
    pub fn public_url(&self) -> &str {
        self.url.as_str()
    }

    /// The URL actually requested (with a query secret appended). Only transports call this.
    pub fn url_with_secret(&self) -> Url {
        let mut url = self.url.clone();
        if let Some(Secret::Query { name, value }) = &self.secret {
            url.query_pairs_mut().append_pair(name, value);
        }
        url
    }

    /// The bearer token, when the secret is a header. Only transports call this.
    pub fn bearer(&self) -> Option<&str> {
        match &self.secret {
            Some(Secret::Bearer(token)) => Some(token),
            _ => None,
        }
    }
}

/// A response as the transport received it. Header names are lower case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// HTTP status code.
    pub status: u16,
    /// Headers as `(lower-case name, value)` pairs.
    pub headers: Vec<(String, String)>,
    /// Body bytes.
    pub body: Vec<u8>,
}

impl Response {
    /// A response with `status` and `body` and no headers.
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    /// Adds a header (the name is stored lower case).
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers
            .push((name.to_ascii_lowercase(), value.to_owned()));
        self
    }

    /// The first value of header `name` (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Why a transport returned no usable response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// No response arrived (connection, TLS or timeout failure), in plain words. Retried.
    Failed(String),
    /// The body was larger than [`Request::max_body_bytes`]; reading stopped there. Not retried.
    TooLarge {
        /// The limit, bytes.
        limit: u64,
    },
}

impl From<String> for TransportError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// Moves bytes. Implementations do no retries, caching or rate limiting; [`HttpClient`] does.
#[async_trait]
pub trait Transport: Send + Sync + std::fmt::Debug {
    /// Sends `request` and returns the response, whatever its status, with at most
    /// [`Request::max_body_bytes`] of body.
    async fn send(&self, request: &Request) -> Result<Response, TransportError>;
}

/// The real transport, built on `reqwest` with [`USER_AGENT`] and a 30 s timeout. Redirects are
/// followed only to `https` URLs (at most five), so a redirect can never downgrade a request,
/// which may carry a key, to plain HTTP.
#[derive(Debug, Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    /// Builds the transport.
    pub fn new() -> crate::Result<Self> {
        let redirects = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() != "https" {
                attempt.error("refused a redirect to a non-HTTPS address")
            } else if attempt.previous().len() >= 5 {
                attempt.error("too many redirects")
            } else {
                attempt.follow()
            }
        });
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .redirect(redirects)
            .build()
            .map_err(|e| crate::SourceError::Client(e.to_string()))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl Transport for ReqwestTransport {
    async fn send(&self, request: &Request) -> Result<Response, TransportError> {
        let mut builder = self.client.get(request.url_with_secret());
        if let Some(token) = request.bearer() {
            builder = builder.bearer_auth(token);
        }
        // reqwest's error text can include the URL, which may carry a query secret, so errors
        // are described without it.
        let response = builder
            .send()
            .await
            .map_err(|e| describe_reqwest_error(&e))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|v| (name.as_str().to_ascii_lowercase(), v.to_owned()))
            })
            .collect();
        // The body is read chunk by chunk (after gzip decoding) and abandoned at the limit, so a
        // huge or endlessly decompressing body is never held in memory.
        let limit = request.max_body_bytes();
        let mut response = response;
        if response.content_length().is_some_and(|n| n > limit) {
            return Err(TransportError::TooLarge { limit });
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| describe_reqwest_error(&e))?
        {
            if body.len() as u64 + chunk.len() as u64 > limit {
                return Err(TransportError::TooLarge { limit });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}

fn too_large(provider: ProviderId, limit: u64) -> crate::SourceError {
    crate::SourceError::BadResponse {
        provider,
        message: format!(
            "the response was larger than {} MB",
            limit.div_ceil(1024 * 1024)
        ),
    }
}

fn describe_reqwest_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "the request timed out".to_owned()
    } else if error.is_connect() {
        "could not connect".to_owned()
    } else if error.is_body() || error.is_decode() {
        "the response was cut off".to_owned()
    } else {
        "the request failed".to_owned()
    }
}

/// How the most recent request to a provider ended, for the Settings screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A 2xx response, or a 404 (a valid "not found" answer).
    Ok,
    /// The provider rejected the key (HTTP 401 or 403 on a request with a key).
    KeyRejected,
    /// The provider kept answering HTTP 429 or said its allowance is spent.
    RateLimited,
    /// Any other failure, in plain words.
    Failed(String),
}

/// Retry and waiting limits.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Total tries per request (the first try included).
    pub max_attempts: u32,
    /// First back-off when the provider gives no wait time; doubled per retry.
    pub initial_backoff: Duration,
    /// Longest single wait. A provider that asks for a longer wait (for example a daily quota
    /// that resets in hours) fails fast with `RateLimited` until that time instead, so a job is
    /// never stuck waiting for hours.
    pub max_wait: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            initial_backoff: Duration::from_secs(1),
            max_wait: Duration::from_secs(60),
        }
    }
}

/// Minimum spacing between two requests to one provider, from each provider's published
/// guidance: TVmaze allows at least 20 calls per 10 s; LRCLIB asks for sequential requests with
/// a 200-500 ms pause; TMDb's upper limit is about 40 requests per second; SubDL publishes only
/// daily quotas, so it gets TVmaze's conservative spacing.
pub fn min_interval(provider: ProviderId) -> Duration {
    match provider {
        ProviderId::Tvmaze | ProviderId::Subdl => Duration::from_millis(500),
        ProviderId::Lrclib => Duration::from_millis(250),
        ProviderId::Tmdb => Duration::from_millis(50),
        ProviderId::Embedded | ProviderId::Local => Duration::ZERO,
    }
}

#[derive(Debug, Default)]
struct ProviderState {
    /// Earliest time the next request may start (spacing and spent allowances).
    next_slot: Option<Instant>,
    /// The provider said its allowance is spent until this wall-clock time, further away than
    /// [`RetryPolicy::max_wait`].
    exhausted_until: Option<SystemTime>,
    last_outcome: Option<Outcome>,
}

/// HTTP client with per-provider spacing, rate-limit handling and retries.
///
/// For each request [`HttpClient::send`]:
/// 1. fails at once with `RateLimited` while the provider's allowance is known to be spent for
///    longer than [`RetryPolicy::max_wait`];
/// 2. waits for the provider's next slot ([`min_interval`] after the previous request, or later
///    when the previous response said the remaining allowance is zero);
/// 3. sends, and on HTTP 429 or 503 waits for `Retry-After` (seconds or an HTTP date), else the
///    rate-limit reset header, else exponential back-off from [`RetryPolicy::initial_backoff`];
///    other 5xx statuses and failed connections are retried with back-off, up to
///    [`RetryPolicy::max_attempts`] tries in all;
/// 4. after every response reads `X-RateLimit-Remaining`/`-Reset`, `RateLimit-Remaining`/`-Reset`,
///    `X-Rate-Limit-Remaining`/`-Reset` or the combined `RateLimit` header, and when the
///    remaining allowance is zero holds the next request until the reset.
///
/// The client is cheap to clone; clones share their state.
#[derive(Debug, Clone)]
pub struct HttpClient {
    transport: Arc<dyn Transport>,
    state: Arc<Mutex<HashMap<ProviderId, ProviderState>>>,
    policy: RetryPolicy,
}

impl HttpClient {
    /// The real client: [`ReqwestTransport`] with the default [`RetryPolicy`].
    pub fn new() -> crate::Result<Self> {
        Ok(Self::with_transport(Arc::new(ReqwestTransport::new()?)))
    }

    /// A client over any transport (tests use [`crate::testing::FixtureTransport`]).
    pub fn with_transport(transport: Arc<dyn Transport>) -> Self {
        Self {
            transport,
            state: Arc::default(),
            policy: RetryPolicy::default(),
        }
    }

    /// Replaces the retry policy.
    pub fn with_policy(mut self, policy: RetryPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// GETs a public URL (no key) and returns the body of a 2xx response.
    pub async fn get(&self, provider: ProviderId, url: &str) -> crate::Result<Vec<u8>> {
        self.send(&Request::get(provider, url, &[])?).await
    }

    /// Sends `request` (see the type documentation for waiting and retries) and returns the body
    /// of a 2xx response. Errors: 404 → `NotFound`; 401/403 on a request with a secret →
    /// `KeyRejected`; 429/503 after the retries → `RateLimited`; anything else → `Network`.
    pub async fn send(&self, request: &Request) -> crate::Result<Vec<u8>> {
        let provider = request.provider;
        let result = self.send_inner(request).await;
        let outcome = match &result {
            Ok(_) | Err(crate::SourceError::NotFound { .. }) => Outcome::Ok,
            Err(crate::SourceError::KeyRejected(_)) => Outcome::KeyRejected,
            Err(crate::SourceError::RateLimited(_)) => Outcome::RateLimited,
            Err(e) => Outcome::Failed(e.to_string()),
        };
        self.with_state(provider, |s| s.last_outcome = Some(outcome));
        result
    }

    /// How the most recent request to `provider` ended, if one was made.
    pub fn last_outcome(&self, provider: ProviderId) -> Option<Outcome> {
        self.with_state(provider, |s| s.last_outcome.clone())
    }

    /// Forgets the recorded outcome and spent allowance of `provider` (after its key changes).
    pub fn reset_outcome(&self, provider: ProviderId) {
        self.with_state(provider, |s| {
            s.last_outcome = None;
            s.exhausted_until = None;
        });
    }

    fn with_state<T>(&self, provider: ProviderId, f: impl FnOnce(&mut ProviderState) -> T) -> T {
        let mut map = self.state.lock().unwrap_or_else(|p| p.into_inner());
        f(map.entry(provider).or_default())
    }

    async fn send_inner(&self, request: &Request) -> crate::Result<Vec<u8>> {
        let provider = request.provider;
        let mut backoff = self.policy.initial_backoff;
        let mut last_error = String::from("the request failed");
        for attempt in 1..=self.policy.max_attempts {
            self.wait_for_slot(provider).await?;
            tracing::debug!(?provider, url = request.public_url(), attempt, "request");
            let response = match self.transport.send(request).await {
                Ok(r) if r.body.len() as u64 > request.max_body_bytes() => {
                    return Err(too_large(provider, request.max_body_bytes()));
                }
                Ok(r) => r,
                Err(TransportError::TooLarge { limit }) => return Err(too_large(provider, limit)),
                Err(TransportError::Failed(message)) => {
                    last_error = message;
                    if attempt < self.policy.max_attempts {
                        tokio::time::sleep(backoff).await;
                        backoff = backoff.saturating_mul(2);
                    }
                    continue;
                }
            };
            self.note_allowance(provider, &response);
            match response.status {
                200..=299 => return Ok(response.body),
                404 => {
                    return Err(crate::SourceError::NotFound {
                        provider,
                        what: request.public_url().to_owned(),
                    });
                }
                401 | 403 if request.has_secret() => {
                    return Err(crate::SourceError::KeyRejected(provider));
                }
                429 | 503 => {
                    let now = SystemTime::now();
                    let wait = retry_after(&response, now)
                        .or_else(|| reset_wait(&response, now))
                        .unwrap_or(backoff);
                    if wait > self.policy.max_wait {
                        self.mark_exhausted(provider, wait);
                        return Err(crate::SourceError::RateLimited(provider));
                    }
                    if attempt == self.policy.max_attempts {
                        return Err(crate::SourceError::RateLimited(provider));
                    }
                    tracing::info!(?provider, ?wait, "rate limited; waiting");
                    tokio::time::sleep(wait).await;
                    backoff = backoff.saturating_mul(2);
                }
                500..=599 => {
                    last_error = format!("the server answered HTTP {}", response.status);
                    if attempt < self.policy.max_attempts {
                        tokio::time::sleep(backoff).await;
                        backoff = backoff.saturating_mul(2);
                    }
                }
                status => {
                    return Err(crate::SourceError::Network {
                        provider,
                        message: format!("the server answered HTTP {status}"),
                    });
                }
            }
        }
        Err(crate::SourceError::Network {
            provider,
            message: last_error,
        })
    }

    /// Waits until `provider` may be called again and reserves the following slot.
    async fn wait_for_slot(&self, provider: ProviderId) -> crate::Result<()> {
        let now = Instant::now();
        let start = self.with_state(provider, |s| {
            if let Some(until) = s.exhausted_until {
                if until > SystemTime::now() {
                    return None;
                }
                s.exhausted_until = None;
            }
            let start = s.next_slot.map_or(now, |slot| slot.max(now));
            s.next_slot = Some(start + min_interval(provider));
            Some(start)
        });
        let Some(start) = start else {
            return Err(crate::SourceError::RateLimited(provider));
        };
        tokio::time::sleep_until(start).await;
        Ok(())
    }

    /// When the response says the remaining allowance is zero, delays the next slot to the reset.
    fn note_allowance(&self, provider: ProviderId, response: &Response) {
        let Some(wait) = spent_allowance_wait(response, SystemTime::now()) else {
            return;
        };
        if wait > self.policy.max_wait {
            self.mark_exhausted(provider, wait);
            return;
        }
        let until = Instant::now() + wait;
        self.with_state(provider, |s| {
            s.next_slot = Some(s.next_slot.map_or(until, |slot| slot.max(until)));
        });
    }

    fn mark_exhausted(&self, provider: ProviderId, wait: Duration) {
        tracing::warn!(
            ?provider,
            ?wait,
            "allowance spent; failing fast until it resets"
        );
        self.with_state(provider, |s| {
            s.exhausted_until = Some(SystemTime::now() + wait);
        });
    }
}

/// The wait a `Retry-After` header asks for: delta seconds or an HTTP date.
pub fn retry_after(response: &Response, now: SystemTime) -> Option<Duration> {
    let value = response.header("retry-after")?.trim();
    if let Ok(seconds) = value.parse::<f64>() {
        return seconds_to_duration(seconds);
    }
    let when = httpdate::parse_http_date(value).ok()?;
    Some(when.duration_since(now).unwrap_or(Duration::ZERO))
}

/// The wait until the rate-limit window resets, from `X-RateLimit-Reset`, `RateLimit-Reset`,
/// `X-Rate-Limit-Reset` or the `reset`/`t` member of a combined `RateLimit` header.
///
/// Providers disagree on the unit: a value above 10^12 is read as Unix milliseconds, a value
/// above 10^9 as Unix seconds, and anything smaller as seconds from now.
pub fn reset_wait(response: &Response, now: SystemTime) -> Option<Duration> {
    let raw = ["x-ratelimit-reset", "ratelimit-reset", "x-rate-limit-reset"]
        .iter()
        .find_map(|name| response.header(name))
        .map(str::to_owned)
        .or_else(|| combined_member(response, &["reset", "t"]))?;
    let value: f64 = raw.trim().parse().ok()?;
    let now_s = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_secs_f64();
    if value > 1e12 {
        seconds_to_duration(value / 1000.0 - now_s).or(Some(Duration::ZERO))
    } else if value > 1e9 {
        seconds_to_duration(value - now_s).or(Some(Duration::ZERO))
    } else {
        seconds_to_duration(value)
    }
}

/// The wait before the next request when the response says the remaining allowance is zero
/// (`None` when it is not zero or not stated). Without a reset header the wait is one second.
pub fn spent_allowance_wait(response: &Response, now: SystemTime) -> Option<Duration> {
    let remaining = [
        "x-ratelimit-remaining",
        "ratelimit-remaining",
        "x-rate-limit-remaining",
    ]
    .iter()
    .find_map(|name| response.header(name))
    .map(str::to_owned)
    .or_else(|| combined_member(response, &["remaining", "r"]))?;
    let remaining: f64 = remaining.trim().parse().ok()?;
    if remaining > 0.0 {
        return None;
    }
    Some(reset_wait(response, now).unwrap_or(Duration::from_secs(1)))
}

/// A member of the combined `RateLimit: limit=10, remaining=0, reset=5` header (also the newer
/// `"policy";r=0;t=5` form).
fn combined_member(response: &Response, names: &[&str]) -> Option<String> {
    let header = response.header("ratelimit")?;
    header
        .split([',', ';'])
        .filter_map(|part| part.split_once('='))
        .find(|(k, _)| names.contains(&k.trim()))
        .map(|(_, v)| v.trim().trim_matches('"').to_owned())
}

fn seconds_to_duration(seconds: f64) -> Option<Duration> {
    (seconds.is_finite() && seconds >= 0.0).then(|| Duration::from_secs_f64(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(unix_s: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(unix_s)
    }

    #[test]
    fn user_agent_names_the_app_version_and_project_page() {
        assert!(
            USER_AGENT.starts_with(&format!("MediaIdentifier/{} (", env!("CARGO_PKG_VERSION")))
        );
        assert!(USER_AGENT.contains("https://github.com/thomas-lane/media-identifier"));
    }

    #[test]
    fn query_secret_is_sent_but_never_in_the_public_url_or_debug() {
        let request = Request::get(
            ProviderId::Subdl,
            "https://api.example.com/x",
            &[("q", "a b")],
        )
        .unwrap()
        .with_secret(Secret::Query {
            name: "api_key",
            value: "SECRET123".into(),
        });
        assert_eq!(request.public_url(), "https://api.example.com/x?q=a+b");
        assert!(
            request
                .url_with_secret()
                .as_str()
                .ends_with("&api_key=SECRET123")
        );
        assert!(!format!("{request:?}").contains("SECRET123"));
    }

    #[test]
    fn retry_after_accepts_seconds_and_http_dates() {
        let now = at(1_800_000_000);
        let r = Response::new(429, "").with_header("Retry-After", "7");
        assert_eq!(retry_after(&r, now), Some(Duration::from_secs(7)));
        let date = httpdate::fmt_http_date(now + Duration::from_secs(30));
        let r = Response::new(429, "").with_header("Retry-After", &date);
        assert_eq!(retry_after(&r, now), Some(Duration::from_secs(30)));
        let r = Response::new(429, "").with_header("Retry-After", "soon");
        assert_eq!(retry_after(&r, now), None);
    }

    #[test]
    fn reset_header_units_are_inferred() {
        let now = at(1_800_000_000);
        let delta = Response::new(200, "").with_header("X-RateLimit-Reset", "12");
        assert_eq!(reset_wait(&delta, now), Some(Duration::from_secs(12)));
        let epoch = Response::new(200, "").with_header("RateLimit-Reset", "1800000020");
        assert_eq!(reset_wait(&epoch, now), Some(Duration::from_secs(20)));
        let epoch_ms = Response::new(200, "").with_header("x-rate-limit-reset", "1800000005000");
        assert_eq!(reset_wait(&epoch_ms, now), Some(Duration::from_secs(5)));
        let past = Response::new(200, "").with_header("X-RateLimit-Reset", "1700000000");
        assert_eq!(reset_wait(&past, now), Some(Duration::ZERO));
    }

    #[test]
    fn spent_allowance_is_read_from_separate_and_combined_headers() {
        let now = at(1_800_000_000);
        let left = Response::new(200, "")
            .with_header("X-RateLimit-Remaining", "3")
            .with_header("X-RateLimit-Reset", "9");
        assert_eq!(spent_allowance_wait(&left, now), None);
        let spent = Response::new(200, "")
            .with_header("X-RateLimit-Remaining", "0")
            .with_header("X-RateLimit-Reset", "9");
        assert_eq!(
            spent_allowance_wait(&spent, now),
            Some(Duration::from_secs(9))
        );
        let combined =
            Response::new(200, "").with_header("RateLimit", "limit=10, remaining=0, reset=4");
        assert_eq!(
            spent_allowance_wait(&combined, now),
            Some(Duration::from_secs(4))
        );
        let draft = Response::new(200, "").with_header("RateLimit", "\"default\";r=0;t=2");
        assert_eq!(
            spent_allowance_wait(&draft, now),
            Some(Duration::from_secs(2))
        );
        let no_reset = Response::new(200, "").with_header("RateLimit-Remaining", "0");
        assert_eq!(
            spent_allowance_wait(&no_reset, now),
            Some(Duration::from_secs(1))
        );
        assert_eq!(spent_allowance_wait(&Response::new(200, ""), now), None);
    }
}
