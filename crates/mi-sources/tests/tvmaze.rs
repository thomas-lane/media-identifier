//! TVmaze through the Sources facade, from recorded responses.

mod common;

use common::{TVMAZE, firefly, fixture, schoolhouse_rock, sources, url};
use mi_sources::http::Response;
use mi_sources::testing::FixtureTransport;
use mi_sources::{ApiKeys, Cache, HttpClient, SourceError, Sources};
use mi_types::{EpisodeKey, EpisodeOrdering, ProviderId, ShowRef};

fn tvmaze(id: &str) -> ShowRef {
    ShowRef {
        provider: ProviderId::Tvmaze,
        id: id.into(),
    }
}

#[tokio::test(start_paused = true)]
async fn search_returns_the_show_with_year_counts_and_score() {
    let t = FixtureTransport::new();
    schoolhouse_rock(&t);
    let s = sources(&t, ApiKeys::default());
    let results = s.search_shows("  Schoolhouse Rock ").await.unwrap();
    assert_eq!(results.len(), 1);
    let first = &results[0];
    assert_eq!(first.show.name, "Schoolhouse Rock!");
    assert_eq!(first.show.year, Some(1973));
    assert_eq!(first.show.kind.as_deref(), Some("Animation"));
    assert_eq!(first.show.season_count, Some(7));
    assert_eq!(first.show.episode_count, Some(65));
    assert_eq!(
        first.show.url.as_deref(),
        Some("https://www.tvmaze.com/shows/15448/schoolhouse-rock")
    );
    assert!((first.score - 1.0).abs() < f32::EPSILON);
    assert!(!first.guessed_from_folder);
    assert!(s.search_shows("   ").await.unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn aired_episode_list_and_the_cache_prevents_a_second_download() {
    let t = FixtureTransport::new();
    schoolhouse_rock(&t);
    let s = sources(&t, ApiKeys::default());
    let list = s
        .episodes(&tvmaze("15448"), EpisodeOrdering::Aired)
        .await
        .unwrap();
    assert_eq!(list.len(), 65);
    assert_eq!(list[0].title, "Three is a Magic Number");
    assert_eq!(
        list[0].key,
        EpisodeKey {
            season: 1,
            number: 1
        }
    );
    assert_eq!(list[0].provider_episode_id, "701194");
    assert_eq!(list[0].runtime_s, None);
    let last = list.last().unwrap();
    assert_eq!((last.key.season, last.key.number), (7, 12));
    assert_eq!(last.runtime_s, Some(180.0));
    let requests = t.requests().len();
    let again = s
        .episodes(&tvmaze("15448"), EpisodeOrdering::Aired)
        .await
        .unwrap();
    assert_eq!(again, list);
    assert_eq!(
        t.requests().len(),
        requests,
        "second call must be served from the cache"
    );
}

#[tokio::test(start_paused = true)]
async fn dvd_order_uses_the_dvd_alternate_list() {
    let t = FixtureTransport::new();
    firefly(&t);
    let s = sources(&t, ApiKeys::default());
    let dvd = s
        .episodes(&tvmaze("180"), EpisodeOrdering::Dvd)
        .await
        .unwrap();
    let aired = s
        .episodes(&tvmaze("180"), EpisodeOrdering::Aired)
        .await
        .unwrap();
    assert_eq!(dvd[0].title, "Serenity");
    assert_eq!(
        dvd[0].key,
        EpisodeKey {
            season: 1,
            number: 1
        }
    );
    let serenity_aired = aired.iter().find(|e| e.title == "Serenity").unwrap();
    assert_eq!(
        serenity_aired.key,
        EpisodeKey {
            season: 1,
            number: 11
        }
    );
    assert_eq!(
        dvd[0].provider_episode_id,
        serenity_aired.provider_episode_id
    );
}

#[tokio::test(start_paused = true)]
async fn dvd_order_fails_clearly_when_tvmaze_has_none() {
    let t = FixtureTransport::new();
    schoolhouse_rock(&t);
    let s = sources(&t, ApiKeys::default());
    let err = s
        .episodes(&tvmaze("15448"), EpisodeOrdering::Dvd)
        .await
        .unwrap_err();
    assert!(matches!(err, SourceError::BadResponse { .. }), "{err:?}");
}

#[tokio::test(start_paused = true)]
async fn a_stale_cached_list_is_used_when_tvmaze_is_unreachable() {
    let cache = Cache::in_memory().unwrap();
    let list_url = url(
        ProviderId::Tvmaze,
        &format!("{TVMAZE}/shows/15448/episodes"),
        &[("specials", "1")],
    );
    let month_ago = mi_sources::cache::now_ms() - 30 * mi_sources::fetch::DAY_MS;
    cache
        .put_response_at(
            ProviderId::Tvmaze,
            &list_url,
            fixture("tvmaze/schoolhouse-rock-episodes.json").as_bytes(),
            month_ago,
        )
        .unwrap();
    let t = FixtureTransport::new();
    t.on(&list_url, Response::new(503, ""));
    let s = Sources::new(
        HttpClient::with_transport(t.clone()),
        cache,
        ApiKeys::default(),
    );
    let list = s
        .episodes(&tvmaze("15448"), EpisodeOrdering::Aired)
        .await
        .unwrap();
    assert_eq!(list.len(), 65);
    assert!(
        t.count(&list_url) >= 1,
        "a refresh must have been attempted"
    );
}
