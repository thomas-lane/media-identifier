//! HttpClient behaviour: spacing, rate-limit headers, retries, error mapping, and what the real
//! transport puts on the wire (checked against a local socket, not the internet).

mod common;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use mi_sources::http::{Outcome, Request, ReqwestTransport, Response, Secret};
use mi_sources::testing::FixtureTransport;
use mi_sources::{HttpClient, SourceError, USER_AGENT};
use mi_types::ProviderId;
use tokio::time::Instant;

const URL: &str = "https://api.tvmaze.com/shows/1";

fn client(t: &std::sync::Arc<FixtureTransport>) -> HttpClient {
    HttpClient::with_transport(t.clone())
}

#[tokio::test(start_paused = true)]
async fn http_429_is_retried_after_retry_after() {
    let t = FixtureTransport::new();
    t.on(
        URL,
        Response::new(429, "slow down").with_header("Retry-After", "3"),
    );
    t.on(URL, Response::new(200, "ok"));
    let start = Instant::now();
    let body = client(&t).get(ProviderId::Tvmaze, URL).await.unwrap();
    assert_eq!(body, b"ok");
    assert_eq!(t.count(URL), 2);
    assert!(start.elapsed() >= Duration::from_secs(3));
}

#[tokio::test(start_paused = true)]
async fn http_429_without_retry_after_backs_off_exponentially_then_gives_up() {
    let t = FixtureTransport::new();
    t.on(URL, Response::new(429, ""));
    let start = Instant::now();
    let err = client(&t).get(ProviderId::Tvmaze, URL).await.unwrap_err();
    assert!(matches!(err, SourceError::RateLimited(ProviderId::Tvmaze)));
    assert_eq!(t.count(URL), 4);
    // 1 s + 2 s + 4 s of back-off between the four tries.
    assert!(start.elapsed() >= Duration::from_secs(7));
}

#[tokio::test(start_paused = true)]
async fn a_spent_daily_quota_fails_fast_without_more_requests() {
    let t = FixtureTransport::new();
    t.on(
        URL,
        Response::new(429, "").with_header("Retry-After", "7200"),
    );
    let http = client(&t);
    let err = http.get(ProviderId::Tvmaze, URL).await.unwrap_err();
    assert!(matches!(err, SourceError::RateLimited(_)));
    let err = http.get(ProviderId::Tvmaze, URL).await.unwrap_err();
    assert!(matches!(err, SourceError::RateLimited(_)));
    assert_eq!(t.count(URL), 1);
    assert_eq!(
        http.last_outcome(ProviderId::Tvmaze),
        Some(Outcome::RateLimited)
    );
    // Other providers are unaffected.
    t.on("https://lrclib.net/api/search", Response::new(200, "[]"));
    assert!(
        http.get(ProviderId::Lrclib, "https://lrclib.net/api/search")
            .await
            .is_ok()
    );
}

#[tokio::test(start_paused = true)]
async fn requests_to_one_provider_are_spaced() {
    let t = FixtureTransport::new();
    t.on(URL, Response::new(200, "ok"));
    let http = client(&t);
    let start = Instant::now();
    for _ in 0..3 {
        http.get(ProviderId::Tvmaze, URL).await.unwrap();
    }
    // Three TVmaze requests need two 500 ms gaps.
    assert!(start.elapsed() >= Duration::from_millis(1000));
}

#[tokio::test(start_paused = true)]
async fn a_zero_remaining_allowance_holds_the_next_request_until_reset() {
    let t = FixtureTransport::new();
    t.on(
        URL,
        Response::new(200, "ok")
            .with_header("X-RateLimit-Remaining", "0")
            .with_header("X-RateLimit-Reset", "5"),
    );
    t.on(URL, Response::new(200, "ok"));
    let http = client(&t);
    let start = Instant::now();
    http.get(ProviderId::Tvmaze, URL).await.unwrap();
    http.get(ProviderId::Tvmaze, URL).await.unwrap();
    assert!(start.elapsed() >= Duration::from_secs(5));
}

#[tokio::test(start_paused = true)]
async fn server_errors_and_dropped_connections_are_retried() {
    let t = FixtureTransport::new();
    t.on(URL, Response::new(502, ""));
    t.on(URL, Response::new(200, "ok"));
    assert_eq!(
        client(&t).get(ProviderId::Tvmaze, URL).await.unwrap(),
        b"ok"
    );

    let t = FixtureTransport::new(); // no fixture: every try fails like a dropped connection
    let http = client(&t);
    let err = http.get(ProviderId::Tvmaze, URL).await.unwrap_err();
    assert!(matches!(err, SourceError::Network { .. }), "{err:?}");
    assert_eq!(t.count(URL), 4);
    assert!(matches!(
        http.last_outcome(ProviderId::Tvmaze),
        Some(Outcome::Failed(_))
    ));
}

#[tokio::test(start_paused = true)]
async fn status_codes_map_to_errors() {
    let t = FixtureTransport::new();
    let keyed = "https://api.themoviedb.org/3/authentication";
    t.on(keyed, Response::new(401, "{}"));
    t.on(URL, Response::new(404, "{}"));
    t.on("https://api.tvmaze.com/bad", Response::new(400, "{}"));
    let http = client(&t);
    let request = Request::get(ProviderId::Tmdb, keyed, &[])
        .unwrap()
        .with_secret(Secret::Bearer("token".into()));
    assert!(matches!(
        http.send(&request).await.unwrap_err(),
        SourceError::KeyRejected(ProviderId::Tmdb)
    ));
    assert_eq!(
        http.last_outcome(ProviderId::Tmdb),
        Some(Outcome::KeyRejected)
    );
    assert!(t.requests()[0].had_secret);
    assert!(matches!(
        http.get(ProviderId::Tvmaze, URL).await.unwrap_err(),
        SourceError::NotFound { .. }
    ));
    assert!(matches!(
        http.get(ProviderId::Tvmaze, "https://api.tvmaze.com/bad")
            .await
            .unwrap_err(),
        SourceError::Network { .. }
    ));
}

/// Serves one HTTP request on a local socket and returns the raw request text.
fn serve_once(listener: TcpListener) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nX-RateLimit-Remaining: 7\r\nConnection: close\r\n\r\nok")
            .unwrap();
        String::from_utf8_lossy(&buf).into_owned()
    })
}

#[tokio::test]
async fn the_real_transport_sends_the_user_agent_query_key_and_bearer_token() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/search", listener.local_addr().unwrap());
    let server = serve_once(listener);
    let transport = ReqwestTransport::new().unwrap();
    let http = HttpClient::with_transport(std::sync::Arc::new(transport));
    let request = Request::get(ProviderId::Subdl, &base, &[("q", "Firefly")])
        .unwrap()
        .with_secret(Secret::Query {
            name: "api_key",
            value: "k123".into(),
        });
    assert_eq!(http.send(&request).await.unwrap(), b"ok");
    let raw = server.join().unwrap().to_lowercase();
    assert!(
        raw.starts_with("get /search?q=firefly&api_key=k123 http/1.1"),
        "{raw}"
    );
    assert!(
        raw.contains(&format!("user-agent: {}", USER_AGENT.to_lowercase())),
        "{raw}"
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/authentication", listener.local_addr().unwrap());
    let server = serve_once(listener);
    let request = Request::get(ProviderId::Tmdb, &base, &[])
        .unwrap()
        .with_secret(Secret::Bearer("tok".into()));
    http.send(&request).await.unwrap();
    let raw = server.join().unwrap().to_lowercase();
    assert!(raw.contains("authorization: bearer tok"), "{raw}");
    assert!(!raw.contains("api_key"), "{raw}");
}

#[tokio::test]
async fn an_oversized_body_is_refused_without_retrying() {
    let t = FixtureTransport::new();
    t.on(URL, Response::new(200, vec![b'x'; 2048]));
    let request = Request::get(ProviderId::Tvmaze, URL, &[])
        .unwrap()
        .with_max_body(1024);
    let err = client(&t).send(&request).await.unwrap_err();
    assert!(
        matches!(&err, SourceError::BadResponse { message, .. } if message.contains("larger than")),
        "{err:?}"
    );
    assert_eq!(t.count(URL), 1, "not retried");
}

#[tokio::test]
async fn the_real_transport_stops_reading_a_body_at_the_limit() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/big", listener.local_addr().unwrap());
    // A chunked body with no length announced: 64 chunks of 1 KiB, endless as far as the client
    // knows until it has read them.
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf);
        let _ = stream.write_all(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
        );
        for _ in 0..64 {
            let chunk = vec![b'x'; 1024];
            if stream.write_all(b"400\r\n").is_err()
                || stream.write_all(&chunk).is_err()
                || stream.write_all(b"\r\n").is_err()
            {
                return;
            }
        }
        let _ = stream.write_all(b"0\r\n\r\n");
    });
    let http = HttpClient::with_transport(std::sync::Arc::new(ReqwestTransport::new().unwrap()));
    let request = Request::get(ProviderId::Tvmaze, &base, &[])
        .unwrap()
        .with_max_body(8 * 1024);
    let err = http.send(&request).await.unwrap_err();
    assert!(
        matches!(&err, SourceError::BadResponse { message, .. } if message.contains("larger than")),
        "{err:?}"
    );
    let _ = server.join();
}
