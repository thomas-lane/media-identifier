//! The shared HTTP client.

use std::time::Duration;

use mi_types::ProviderId;

/// `User-Agent` sent with every request.
pub const USER_AGENT: &str = concat!("MediaIdentifier/", env!("CARGO_PKG_VERSION"));

/// HTTP client with per-provider rate limiting.
///
/// Every provider request goes through [`HttpClient::get`], which waits when the provider's last
/// response said the remaining allowance is spent (`X-RateLimit-Remaining`/`-Reset`,
/// `RateLimit-*`, or provider-specific headers), and retries HTTP 429 after `Retry-After`
/// (or exponential back-off from 1 s, at most 4 tries).
#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
}

impl HttpClient {
    /// Builds the client with [`USER_AGENT`] and a 30 s timeout.
    pub fn new() -> crate::Result<Self> {
        let inner = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| crate::SourceError::Client(e.to_string()))?;
        Ok(Self { inner })
    }

    /// GETs `url` for `provider` with rate limiting and retries; returns the body on 2xx.
    /// 401/403 map to `KeyRejected` for providers with keys.
    pub async fn get(&self, provider: ProviderId, url: &str) -> crate::Result<Vec<u8>> {
        let _ = (&self.inner, provider, url);
        todo!("sources module: rate-limited GET")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_names_the_app_and_version() {
        assert_eq!(
            USER_AGENT,
            format!("MediaIdentifier/{}", env!("CARGO_PKG_VERSION"))
        );
    }
}
