//! TMDb numbering through the Sources facade, from hand-written responses in TMDb's documented
//! shapes (TMDb needs a personal key, so nothing was recorded).

mod common;

use common::{firefly, fixture, sources, url};
use mi_sources::http::Response;
use mi_sources::testing::FixtureTransport;
use mi_sources::{ApiKeys, Cache};
use mi_types::{EpisodeKey, EpisodeOrdering, ProviderId, ShowRef, SourceState};

const TMDB: &str = "https://api.themoviedb.org/3";
const V3_KEY: &str = "0123456789abcdef0123456789abcdef";

fn keys() -> ApiKeys {
    ApiKeys {
        tmdb: Some(V3_KEY.into()),
        ..ApiKeys::default()
    }
}

fn tmdb_url(path: &str, params: &[(&str, &str)]) -> String {
    url(ProviderId::Tmdb, &format!("{TMDB}{path}"), params)
}

fn firefly_on_tmdb(t: &FixtureTransport) {
    t.on_json(
        &tmdb_url("/find/78874", &[("external_source", "tvdb_id")]),
        &fixture("tmdb/find-tvdb-78874.json"),
    );
    t.on_json(&tmdb_url("/tv/1437", &[]), &fixture("tmdb/tv-1437.json"));
    t.on_json(
        &tmdb_url("/tv/1437/season/0", &[]),
        &fixture("tmdb/tv-1437-season-0.json"),
    );
    t.on_json(
        &tmdb_url("/tv/1437/season/1", &[]),
        &fixture("tmdb/tv-1437-season-1.json"),
    );
    t.on_json(
        &tmdb_url("/tv/1437/episode_groups", &[]),
        &fixture("tmdb/episode-groups.json"),
    );
    t.on_json(
        &tmdb_url("/tv/episode_group/5b11c5520e0a265f9b000001", &[]),
        &fixture("tmdb/episode-group-dvd.json"),
    );
}

fn firefly_ref() -> ShowRef {
    ShowRef {
        provider: ProviderId::Tvmaze,
        id: "180".into(),
    }
}

#[tokio::test(start_paused = true)]
async fn a_tmdb_key_switches_a_tvmaze_show_to_tmdb_numbering() {
    let t = FixtureTransport::new();
    firefly(&t);
    firefly_on_tmdb(&t);
    let s = sources(&t, keys());
    let list = s
        .episodes(&firefly_ref(), EpisodeOrdering::Aired)
        .await
        .unwrap();
    let keys: Vec<(u32, u32, &str)> = list
        .iter()
        .map(|e| (e.key.season, e.key.number, e.title.as_str()))
        .collect();
    assert_eq!(
        keys,
        vec![
            (0, 1, "Here's How It All Happened"),
            (1, 1, "The Train Job"),
            (1, 2, "Bushwhacked"),
            (1, 11, "Serenity"),
        ]
    );
    assert!(
        list.iter()
            .all(|e| e.show_ref.provider == ProviderId::Tmdb && e.show_ref.id == "1437")
    );
    for request in t
        .requests()
        .iter()
        .filter(|r| r.provider == ProviderId::Tmdb)
    {
        assert!(request.had_secret);
        assert!(
            !request.url.contains(V3_KEY),
            "key leaked into {}",
            request.url
        );
    }
}

#[tokio::test(start_paused = true)]
async fn tmdb_dvd_order_comes_from_the_dvd_episode_group() {
    let t = FixtureTransport::new();
    firefly(&t);
    firefly_on_tmdb(&t);
    let s = sources(&t, keys());
    let dvd = s
        .episodes(&firefly_ref(), EpisodeOrdering::Dvd)
        .await
        .unwrap();
    assert_eq!(dvd[1].title, "Serenity");
    assert_eq!(
        dvd[1].key,
        EpisodeKey {
            season: 1,
            number: 1
        }
    );
    assert_eq!(dvd[1].ordering, EpisodeOrdering::Dvd);
}

#[tokio::test(start_paused = true)]
async fn a_rejected_tmdb_key_falls_back_to_tvmaze_and_shows_in_status() {
    let t = FixtureTransport::new();
    firefly(&t);
    t.on(
        &tmdb_url("/find/78874", &[("external_source", "tvdb_id")]),
        Response::new(
            401,
            r#"{"status_code":7,"status_message":"Invalid API key"}"#,
        ),
    );
    let s = sources(&t, keys());
    let list = s
        .episodes(&firefly_ref(), EpisodeOrdering::Aired)
        .await
        .unwrap();
    assert!(
        list.iter()
            .all(|e| e.show_ref.provider == ProviderId::Tvmaze)
    );
    assert_eq!(list.len(), 14);
    let tmdb = s
        .status()
        .into_iter()
        .find(|st| st.provider == ProviderId::Tmdb)
        .unwrap();
    assert_eq!(tmdb.state, SourceState::KeyRejected);
    assert!(tmdb.has_key);
}

#[tokio::test(start_paused = true)]
async fn tmdb_data_older_than_six_months_is_never_served() {
    let cache = Cache::in_memory().unwrap();
    let details = tmdb_url("/tv/1437", &[]);
    let seven_months_ago = mi_sources::cache::now_ms() - 213 * mi_sources::fetch::DAY_MS;
    cache
        .put_response_at(
            ProviderId::Tmdb,
            &details,
            b"{\"seasons\":[]}",
            seven_months_ago,
        )
        .unwrap();
    // The row is still stored (opening the cache purges such rows; this one arrived later), and
    // the lookup must refuse it even though TMDb is down.
    let t = FixtureTransport::new();
    t.on(&details, Response::new(503, ""));
    let tmdb = mi_sources::tmdb::Tmdb::new(
        mi_sources::HttpClient::with_transport(t.clone()),
        std::sync::Arc::new(cache),
        V3_KEY.into(),
    );
    let tmdb_ref = ShowRef {
        provider: ProviderId::Tmdb,
        id: "1437".into(),
    };
    use mi_sources::EpisodeProvider;
    assert!(
        tmdb.episodes(&tmdb_ref, EpisodeOrdering::Aired)
            .await
            .is_err()
    );
}

#[tokio::test(start_paused = true)]
async fn stale_tmdb_data_within_six_months_is_served_offline() {
    let cache = Cache::in_memory().unwrap();
    let two_months_ago = mi_sources::cache::now_ms() - 60 * mi_sources::fetch::DAY_MS;
    for (path, file) in [
        ("/tv/1437", "tmdb/tv-1437.json"),
        ("/tv/1437/season/0", "tmdb/tv-1437-season-0.json"),
        ("/tv/1437/season/1", "tmdb/tv-1437-season-1.json"),
    ] {
        cache
            .put_response_at(
                ProviderId::Tmdb,
                &tmdb_url(path, &[]),
                fixture(file).as_bytes(),
                two_months_ago,
            )
            .unwrap();
    }
    let t = FixtureTransport::new(); // unreachable: no fixtures at all
    let tmdb = mi_sources::tmdb::Tmdb::new(
        mi_sources::HttpClient::with_transport(t.clone()),
        std::sync::Arc::new(cache),
        V3_KEY.into(),
    );
    let tmdb_ref = ShowRef {
        provider: ProviderId::Tmdb,
        id: "1437".into(),
    };
    use mi_sources::EpisodeProvider;
    let list = tmdb
        .episodes(&tmdb_ref, EpisodeOrdering::Aired)
        .await
        .unwrap();
    assert_eq!(list.len(), 4);
}
