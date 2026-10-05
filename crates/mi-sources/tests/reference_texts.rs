//! Reference text through the Sources facade: SubDL season packs (hand-written responses in the
//! documented shape, archives built by the test), LRCLIB lyrics (recorded responses, lyrics cut
//! to two lines), local files, summaries, caching, failures and cancellation.

mod common;

use std::sync::{Arc, Mutex};

use common::{firefly, fixture, schoolhouse_rock, sources, srt, url, zip_of};
use mi_sources::http::Response;
use mi_sources::local::LocalReferences;
use mi_sources::testing::FixtureTransport;
use mi_sources::{ApiKeys, SourceError};
use mi_types::{CancelFlag, Episode, EpisodeOrdering, ProviderId, ShowRef, SourceState, TextKind};

const SUBDL_KEY: &str = "subdl-key-123";
const SEARCH: &str = "https://api.subdl.com/api/v1/subtitles";
const DL: &str = "https://dl.subdl.com";

fn subdl_keys() -> ApiKeys {
    ApiKeys {
        subdl: Some(SUBDL_KEY.into()),
        ..ApiKeys::default()
    }
}

fn subdl_search(packs: bool) -> String {
    let mut params = vec![
        ("imdb_id", "tt0303461"),
        ("type", "tv"),
        ("season_number", "1"),
        ("languages", "EN"),
        ("subs_per_page", "30"),
        ("unpack", "1"),
        ("client", "custom_integration"),
    ];
    if packs {
        params.push(("full_season", "1"));
    }
    url(ProviderId::Subdl, SEARCH, &params)
}

fn firefly_subdl(t: &FixtureTransport) {
    t.on_json(
        &subdl_search(true),
        &fixture("subdl/firefly-season-1-packs.json"),
    );
    t.on_json(
        &subdl_search(false),
        &fixture("subdl/firefly-season-1-singles.json"),
    );
    t.on(
        &format!("{DL}/subtitle/900001-222.zip"),
        Response::new(
            200,
            zip_of(&[
                (
                    "Firefly - 1x01 - The Train Job.srt",
                    &srt("We rob the train."),
                ),
                ("Disc 1/Episode 2.srt", &srt("Reavers took this ship.")),
                ("Firefly.S01E05.srt", &srt("Out of gas.")),
                (
                    "Firefly.S01E11.Serenity.srt",
                    &srt("Take my love, take my land."),
                ),
            ]),
        ),
    );
    t.on(
        &format!("{DL}/subtitle/900001-302/d3"),
        Response::new(200, srt("Mrs. Reynolds says hello.").into_bytes()),
    );
    t.on(
        &format!("{DL}/subtitle/900001-304.zip"),
        Response::new(200, zip_of(&[("subs.srt", &srt("Jayne is a hero."))])),
    );
}

async fn firefly_dvd(s: &mi_sources::Sources) -> Vec<Episode> {
    let show = ShowRef {
        provider: ProviderId::Tvmaze,
        id: "180".into(),
    };
    s.episodes(&show, EpisodeOrdering::Dvd).await.unwrap()
}

async fn firefly_show(s: &mi_sources::Sources) -> mi_types::Show {
    s.tvmaze().show("180").await.unwrap().0
}

fn texts_by_title<'a>(
    texts: &'a [mi_types::ReferenceText],
    episodes: &[Episode],
) -> Vec<(String, &'a str, ProviderId)> {
    texts
        .iter()
        .map(|t| {
            let title = episodes
                .iter()
                .find(|e| e.key == t.episode)
                .map(|e| e.title.clone())
                .unwrap_or_default();
            (title, t.text.as_str(), t.provider)
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn subdl_season_pack_fills_dvd_ordered_episodes_by_their_aired_numbers() {
    let t = FixtureTransport::new();
    firefly(&t);
    firefly_subdl(&t);
    let s = sources(&t, subdl_keys());
    let dvd = firefly_dvd(&s).await;
    // DVD 1-3, 6, 7 are aired 11, 1, 2, 3, 4.
    let wanted: Vec<Episode> = [0, 1, 2, 5, 6].iter().map(|&i| dvd[i].clone()).collect();
    let show = firefly_show(&s).await;
    let progress = Mutex::new(Vec::new());
    let texts = s
        .reference_texts(
            &show,
            &wanted,
            "en",
            &|done, total| progress.lock().unwrap().push((done, total)),
            &CancelFlag::new(),
        )
        .await
        .unwrap();

    assert_eq!(
        texts_by_title(&texts, &wanted),
        vec![
            (
                "Serenity".to_owned(),
                "Take my love, take my land.",
                ProviderId::Subdl
            ),
            (
                "The Train Job".to_owned(),
                "We rob the train.",
                ProviderId::Subdl
            ),
            (
                "Bushwhacked".to_owned(),
                "Reavers took this ship.",
                ProviderId::Subdl
            ),
            (
                "Our Mrs. Reynolds".to_owned(),
                "Mrs. Reynolds says hello.",
                ProviderId::Subdl
            ),
            (
                "Jaynestown".to_owned(),
                "Jayne is a hero.",
                ProviderId::Subdl
            ),
        ]
    );
    assert!(
        texts
            .iter()
            .all(|t| t.ordering == EpisodeOrdering::Dvd && t.kind == TextKind::Subtitles)
    );
    assert_eq!(
        texts[0].provider_ref,
        "/subtitle/900001-222.zip#Firefly.S01E11.Serenity.srt"
    );
    assert_eq!(texts[3].provider_ref, "/subtitle/900001-302/d3");
    assert_eq!(*progress.lock().unwrap(), vec![(0, 5), (5, 5)]);

    // One pack download for three episodes, one raw file, one single archive.
    assert_eq!(t.count(&format!("{DL}/subtitle/900001-222.zip")), 1);
    assert_eq!(t.count(&format!("{DL}/subtitle/900001-111.zip")), 0);
    assert_eq!(
        t.count(&format!("{DL}/subtitle/900001-301")),
        0,
        "hearing-impaired file avoided"
    );
    assert_eq!(t.count(SEARCH), 2);
    for r in t.requests() {
        assert!(!r.url.contains(SUBDL_KEY), "key leaked into {}", r.url);
        if r.url.starts_with(SEARCH) {
            assert!(r.had_secret);
        }
        if r.url.starts_with(DL) {
            assert!(!r.had_secret, "downloads are anonymous");
        }
    }

    // Nothing is downloaded twice: a second call is answered from the cache.
    let before = t.requests().len();
    let again = s
        .reference_texts(&show, &wanted, "en", &|_, _| {}, &CancelFlag::new())
        .await
        .unwrap();
    assert_eq!(again.len(), 5);
    assert_eq!(t.requests().len(), before);
}

#[tokio::test(start_paused = true)]
async fn a_rejected_subdl_key_falls_back_to_summaries_and_shows_in_status() {
    let t = FixtureTransport::new();
    firefly(&t);
    t.on(
        &subdl_search(true),
        Response::new(403, r#"{"status":false,"error":"not_authorized"}"#),
    );
    let s = sources(&t, subdl_keys());
    let dvd = firefly_dvd(&s).await;
    let show = firefly_show(&s).await;
    let texts = s
        .reference_texts(&show, &dvd[..2], "en", &|_, _| {}, &CancelFlag::new())
        .await
        .unwrap();
    assert_eq!(texts.len(), 2);
    assert!(
        texts
            .iter()
            .all(|t| t.kind == TextKind::Summary && t.provider == ProviderId::Tvmaze)
    );
    assert!(!texts[0].text.is_empty());
    let subdl = s
        .status()
        .into_iter()
        .find(|st| st.provider == ProviderId::Subdl)
        .unwrap();
    assert_eq!(subdl.state, SourceState::KeyRejected);
    assert_eq!(
        t.count(SEARCH),
        1,
        "a rejected source is not asked again in the same call"
    );
}

fn lrclib(t: &FixtureTransport) {
    let search = "https://lrclib.net/api/search";
    t.on_json(
        &url(
            ProviderId::Lrclib,
            search,
            &[
                ("track_name", "Three is a Magic Number"),
                ("artist_name", "Schoolhouse Rock!"),
            ],
        ),
        &fixture("lrclib/search-three-is-a-magic-number.json"),
    );
    t.on_json(
        &url(
            ProviderId::Lrclib,
            search,
            &[
                ("track_name", "My Hero, Zero"),
                ("artist_name", "Schoolhouse Rock!"),
            ],
        ),
        "[]",
    );
    t.on_json(
        &url(
            ProviderId::Lrclib,
            search,
            &[
                ("track_name", "My Hero, Zero"),
                ("album_name", "Schoolhouse Rock!"),
            ],
        ),
        "[]",
    );
}

#[tokio::test(start_paused = true)]
async fn lrclib_lyrics_fill_musical_shorts_without_a_subdl_key() {
    let t = FixtureTransport::new();
    schoolhouse_rock(&t);
    lrclib(&t);
    let s = sources(&t, ApiKeys::default());
    let show = s.search_shows("Schoolhouse Rock").await.unwrap()[0]
        .show
        .clone();
    let list = s
        .episodes(&show.show_ref, EpisodeOrdering::Aired)
        .await
        .unwrap();
    let texts = s
        .reference_texts(&show, &list[..2], "en", &|_, _| {}, &CancelFlag::new())
        .await
        .unwrap();
    assert_eq!(texts.len(), 1, "My Hero, Zero has no lyrics and no summary");
    assert_eq!(texts[0].kind, TextKind::Lyrics);
    assert_eq!(texts[0].provider, ProviderId::Lrclib);
    assert_eq!(texts[0].episode, list[0].key);
    assert!(texts[0].text.starts_with("Three is a magic number."));
    assert_eq!(
        t.count("https://api.subdl.com"),
        0,
        "SubDL is not used without a key"
    );
    let subdl = s
        .status()
        .into_iter()
        .find(|st| st.provider == ProviderId::Subdl)
        .unwrap();
    assert_eq!(subdl.state, SourceState::NeedsKey);
    assert!(!subdl.has_key);
}

#[tokio::test(start_paused = true)]
async fn local_files_come_first_and_are_not_cached() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("Season 01")).unwrap();
    std::fs::write(
        dir.path()
            .join("Season 01")
            .join("Schoolhouse Rock S01E02.srt"),
        srt("Zero is a wonderful number."),
    )
    .unwrap();
    let t = FixtureTransport::new();
    schoolhouse_rock(&t);
    lrclib(&t);
    let mut s = sources(&t, ApiKeys::default());
    s.add_reference_provider(Arc::new(LocalReferences::from_folder(dir.path()).unwrap()));
    let show = s.tvmaze().show("15448").await.unwrap().0;
    let list = s
        .episodes(&show.show_ref, EpisodeOrdering::Aired)
        .await
        .unwrap();
    let texts = s
        .reference_texts(&show, &list[..2], "en", &|_, _| {}, &CancelFlag::new())
        .await
        .unwrap();
    let local = texts
        .iter()
        .find(|t| t.provider == ProviderId::Local)
        .unwrap();
    assert_eq!(local.episode, list[1].key);
    assert_eq!(local.text, "Zero is a wonderful number.");
    assert_eq!(local.provider_ref, "Schoolhouse Rock S01E02.srt");
    let my_hero = url(
        ProviderId::Lrclib,
        "https://lrclib.net/api/search",
        &[
            ("track_name", "My Hero, Zero"),
            ("artist_name", "Schoolhouse Rock!"),
        ],
    );
    assert_eq!(
        t.count(&my_hero),
        0,
        "an episode with local text is not looked up online"
    );
    assert!(
        s.cache()
            .texts(&list[1].show_ref, list[1].ordering, list[1].key, None)
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(start_paused = true)]
async fn cancellation_stops_the_lookup() {
    let t = FixtureTransport::new();
    schoolhouse_rock(&t);
    let s = sources(&t, ApiKeys::default());
    let show = s.tvmaze().show("15448").await.unwrap().0;
    let list = s
        .episodes(&show.show_ref, EpisodeOrdering::Aired)
        .await
        .unwrap();
    let cancel = CancelFlag::new();
    cancel.cancel();
    let before = t.requests().len();
    let err = s
        .reference_texts(&show, &list, "en", &|_, _| {}, &cancel)
        .await
        .unwrap_err();
    assert!(matches!(err, SourceError::Cancelled));
    assert_eq!(t.requests().len(), before);
}
