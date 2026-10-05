//! A recorded-response transport for tests, so no test touches the network.
//!
//! Other crates use it too (for example `mi-core`'s pipeline tests), which is why it is public
//! rather than `#[cfg(test)]`.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use mi_types::ProviderId;

use crate::http::{Request, Response, Transport};

/// One request the fixture transport received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRequest {
    /// The provider.
    pub provider: ProviderId,
    /// The public URL (no secret).
    pub url: String,
    /// Whether a secret (API key) was attached.
    pub had_secret: bool,
}

/// Answers requests from responses registered per public URL.
///
/// Responses registered for one URL are served in order; the last one is repeated. A request for
/// an unregistered URL fails like a dropped connection, with a message naming the URL, so a test
/// that makes an unexpected request fails visibly.
#[derive(Debug, Default)]
pub struct FixtureTransport {
    routes: Mutex<HashMap<String, VecDeque<Response>>>,
    log: Mutex<Vec<RecordedRequest>>,
}

impl FixtureTransport {
    /// An empty transport.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Registers `response` for the exact public URL `url` (query parameters included, in the
    /// order the code adds them).
    pub fn on(&self, url: &str, response: Response) {
        self.routes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(url.to_owned())
            .or_default()
            .push_back(response);
    }

    /// Registers a 200 response with a JSON (or any text) body.
    pub fn on_json(&self, url: &str, body: &str) {
        self.on(url, Response::new(200, body.as_bytes().to_vec()));
    }

    /// Every request received so far, in order.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.log.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// How many requests went to URLs starting with `prefix`.
    pub fn count(&self, prefix: &str) -> usize {
        self.requests()
            .iter()
            .filter(|r| r.url.starts_with(prefix))
            .count()
    }
}

#[async_trait]
impl Transport for FixtureTransport {
    async fn send(&self, request: &Request) -> Result<Response, String> {
        let url = request.public_url().to_owned();
        self.log
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(RecordedRequest {
                provider: request.provider,
                url: url.clone(),
                had_secret: request.has_secret(),
            });
        let mut routes = self.routes.lock().unwrap_or_else(|p| p.into_inner());
        let queue = routes
            .get_mut(&url)
            .ok_or_else(|| format!("no fixture for {url}"))?;
        let response = if queue.len() > 1 {
            queue.pop_front()
        } else {
            queue.front().cloned()
        };
        response.ok_or_else(|| format!("no fixture for {url}"))
    }
}
