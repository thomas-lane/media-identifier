//! Live smoke tests against the real services. Ignored by default (tests never touch the
//! network); run with `cargo test -p mi-sources --test live -- --ignored --nocapture`.
//! SubDL and TMDb run only when `MI_SUBDL_KEY` / `MI_TMDB_KEY` are set.

use mi_sources::{ApiKeys, Cache, HttpClient, Sources};
use mi_types::{CancelFlag, EpisodeOrdering, ProviderId, TextKind};

fn live(keys: ApiKeys) -> Sources {
    Sources::new(
        HttpClient::new().unwrap(),
        Cache::in_memory().unwrap(),
        keys,
    )
}

#[tokio::test]
#[ignore = "uses the network"]
async fn tvmaze_schoolhouse_rock() {
    let s = live(ApiKeys::default());
    let results = s.search_shows("Schoolhouse Rock").await.unwrap();
    let show = &results[0].show;
    println!(
        "{} ({:?}) seasons={:?} episodes={:?}",
        show.name, show.year, show.season_count, show.episode_count
    );
    assert_eq!(show.name, "Schoolhouse Rock!");
    assert_eq!(show.year, Some(1973));
    let list = s
        .episodes(&show.show_ref, EpisodeOrdering::Aired)
        .await
        .unwrap();
    println!("{} episodes; first: {}", list.len(), list[0].title);
    assert!(list.len() >= 60);
    assert!(list.iter().any(|e| e.title == "Conjunction Junction"));
}

#[tokio::test]
#[ignore = "uses the network"]
async fn lrclib_conjunction_junction() {
    let s = live(ApiKeys::default());
    let show = s.search_shows("Schoolhouse Rock").await.unwrap()[0]
        .show
        .clone();
    let list = s
        .episodes(&show.show_ref, EpisodeOrdering::Aired)
        .await
        .unwrap();
    let episode = list
        .iter()
        .find(|e| e.title == "Conjunction Junction")
        .unwrap()
        .clone();
    let texts = s
        .reference_texts(
            &show,
            std::slice::from_ref(&episode),
            "en",
            &|_, _| {},
            &CancelFlag::new(),
        )
        .await
        .unwrap();
    let lyrics = texts
        .iter()
        .find(|t| t.kind == TextKind::Lyrics)
        .expect("lyrics");
    println!(
        "LRCLIB record {}: {} lines",
        lyrics.provider_ref,
        lyrics.text.lines().count()
    );
    assert_eq!(lyrics.provider, ProviderId::Lrclib);
    assert!(
        lyrics
            .text
            .to_lowercase()
            .contains("conjunction junction, what's your function")
    );
}

#[tokio::test]
#[ignore = "uses the network and MI_SUBDL_KEY"]
async fn subdl_firefly_season_1() {
    let Ok(key) = std::env::var("MI_SUBDL_KEY") else {
        println!("MI_SUBDL_KEY not set; skipped");
        return;
    };
    let s = live(ApiKeys {
        subdl: Some(key),
        tmdb: None,
    });
    let show = s.search_shows("Firefly").await.unwrap()[0].show.clone();
    let list = s
        .episodes(&show.show_ref, EpisodeOrdering::Aired)
        .await
        .unwrap();
    let texts = s
        .reference_texts(&show, &list[..3], "en", &|_, _| {}, &CancelFlag::new())
        .await
        .unwrap();
    for t in &texts {
        println!(
            "{:?} {:?} {} chars from {}",
            t.episode,
            t.kind,
            t.text.len(),
            t.provider_ref
        );
    }
    assert!(texts.iter().any(|t| t.provider == ProviderId::Subdl));
}

#[tokio::test]
#[ignore = "uses the network and MI_TMDB_KEY"]
async fn tmdb_firefly_numbering() {
    let Ok(key) = std::env::var("MI_TMDB_KEY") else {
        println!("MI_TMDB_KEY not set; skipped");
        return;
    };
    let s = live(ApiKeys {
        subdl: None,
        tmdb: Some(key),
    });
    let show = s.search_shows("Firefly").await.unwrap()[0].show.clone();
    let list = s
        .episodes(&show.show_ref, EpisodeOrdering::Aired)
        .await
        .unwrap();
    println!("{} episodes from {:?}", list.len(), list[0].show_ref);
    assert_eq!(list[0].show_ref.provider, ProviderId::Tmdb);
}

#[tokio::test]
#[ignore = "uses the network"]
async fn lrclib_schoolhouse_rock_season_1_coverage() {
    let s = live(ApiKeys::default());
    let show = s.search_shows("Schoolhouse Rock").await.unwrap()[0]
        .show
        .clone();
    let list = s
        .episodes(&show.show_ref, EpisodeOrdering::Aired)
        .await
        .unwrap();
    let season: Vec<_> = list.into_iter().filter(|e| e.key.season == 1).collect();
    let texts = s
        .reference_texts(&show, &season, "en", &|_, _| {}, &CancelFlag::new())
        .await
        .unwrap();
    for e in &season {
        let found = texts.iter().find(|t| t.episode == e.key);
        println!(
            "S{:02}E{:02} {:<32} {}",
            e.key.season,
            e.key.number,
            e.title,
            found.map_or("none".to_owned(), |t| format!(
                "{:?} {}",
                t.kind, t.provider_ref
            ))
        );
    }
    assert!(texts.iter().any(|t| t.kind == TextKind::Lyrics));
}
