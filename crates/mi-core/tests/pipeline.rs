//! Whole jobs through the engine with scripted media, catalog and speech (see `common`).
//! These check the pipeline's plumbing and events; they say nothing about real-world accuracy.

mod common;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use common::*;
use mi_types::{
    EpisodeKey, FileDecision, FileId, FileRole, FileStatus, JobEvent, NamingScheme, PlanConflict,
    RenamePlanRequest, ReviewDecision, SaveMode, Settings, Stage, StageState, Suggestion, Verdict,
};

fn key(n: u32) -> EpisodeKey {
    EpisodeKey {
        season: 1,
        number: n,
    }
}

/// Four two-minute files, each telling one story; `order[i]` is the story of `title_t0{i+1}`.
fn four_files(order: [usize; 4]) -> (FakeMedia, Script, std::path::PathBuf) {
    let folder = std::path::PathBuf::from("/disc");
    let files = (0..4)
        .map(|i| {
            media_file(
                &folder,
                &format!("title_t0{}.mkv", i + 1),
                120.0,
                FileRole::Candidate,
            )
        })
        .collect();
    let script: Script = (0..4)
        .map(|i| {
            (
                FileId(format!("title_t0{}.mkv", i + 1)),
                story_lines(order[i], 10.0),
            )
        })
        .collect();
    (FakeMedia::new(scan(&folder, files)), script, folder)
}

fn final_matches(events: &[JobEvent]) -> HashMap<FileId, mi_types::FileMatch> {
    let mut out = HashMap::new();
    for e in events {
        if let JobEvent::Matched { result, .. } = e {
            out.insert(result.file_id.clone(), result.clone());
        }
    }
    out
}

fn suggested(m: &mi_types::FileMatch) -> Option<EpisodeKey> {
    match m.suggestion {
        Suggestion::Episode { episode } => Some(episode),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identifies_every_file_and_reports_progress_in_order() {
    let (media, script, folder) = four_files([2, 0, 3, 1]);
    let h = harness(
        media,
        FakeCatalog::with_subtitles(),
        FakeSpeech::new(script),
    );
    let job = h.engine.start_job(request(&folder)).unwrap();
    assert!(matches!(
        h.recorder.wait_for_end(&job).await,
        JobEvent::Finished { .. }
    ));
    let events = h.recorder.events();

    assert!(
        matches!(&events[0], JobEvent::Started { file_ids, accelerator: mi_types::Accelerator::AppleGpu, .. } if file_ids.len() == 4)
    );
    let episodes_at = events
        .iter()
        .position(|e| matches!(e, JobEvent::Episodes { episodes, .. } if episodes.len() == 4))
        .expect("episodes event");
    let first_listen = events
        .iter()
        .position(|e| {
            matches!(
                e,
                JobEvent::File {
                    status: FileStatus::Listening,
                    ..
                }
            )
        })
        .unwrap();
    assert!(
        episodes_at < first_listen,
        "the episode list comes before listening"
    );
    for stage in [
        Stage::EpisodeList,
        Stage::Subtitles,
        Stage::Listening,
        Stage::Matching,
    ] {
        assert!(
            events.iter().any(|e| matches!(e, JobEvent::Stage { stage: s, state: StageState::Done, .. } if *s == stage)),
            "{stage:?} finishes"
        );
    }
    assert!(events.iter().any(|e| matches!(
        e,
        JobEvent::Stage {
            stage: Stage::DiscOrder,
            state: StageState::Skipped,
            ..
        }
    )));
    assert!(events.iter().any(|e| matches!(e, JobEvent::Eta { .. })));

    let matches = final_matches(&events);
    let expected = [3, 1, 4, 2];
    for (i, n) in expected.iter().enumerate() {
        let m = &matches[&FileId(format!("title_t0{}.mkv", i + 1))];
        assert_eq!(suggested(m), Some(key(*n)), "title_t0{}", i + 1);
        assert_eq!(m.confidence.verdict, Verdict::Confident);
    }

    let results = h.engine.job_results(&job).unwrap();
    assert!(results.complete);
    assert_eq!(results.episodes.len(), 4);
    assert_eq!(results.matches.len(), 4);
    assert!(!h.engine.is_job_running());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_file_is_matched_as_soon_as_it_is_heard() {
    let (media, script, folder) = four_files([0, 1, 2, 3]);
    let h = harness(
        media,
        FakeCatalog::with_subtitles(),
        FakeSpeech::new(script),
    );
    let job = h.engine.start_job(request(&folder)).unwrap();
    h.recorder.wait_for_end(&job).await;
    let events = h.recorder.events();
    let first_matched = events
        .iter()
        .position(|e| matches!(e, JobEvent::Matched { result, .. } if result.file_id.0 == "title_t01.mkv"))
        .unwrap();
    let second_listening = events
        .iter()
        .position(|e| matches!(e, JobEvent::File { file_id, status: FileStatus::Listening, .. } if file_id.0 == "title_t02.mkv"))
        .unwrap();
    assert!(
        first_matched < second_listening,
        "the first file can be reviewed before the second is heard"
    );
    let done_with_best = events.iter().any(|e| {
        matches!(e,
        JobEvent::File { file_id, status: FileStatus::Done, best_so_far: Some(t), .. }
            if file_id.0 == "title_t01.mkv" && t == TITLES[0])
    });
    assert!(done_with_best);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_play_all_gives_the_disc_order_and_is_never_matched() {
    let folder = std::path::PathBuf::from("/disc");
    // Disc order: t01=ep2, t02=ep1, t03=ep3. Each file's audio is 20 s of its own noise; the
    // play-all is the three in disc order, with a chapter at each start.
    let order = [1usize, 0, 2];
    let mut files: Vec<_> = (0..3)
        .map(|i| {
            media_file(
                &folder,
                &format!("title_t0{}.mkv", i + 1),
                20.0,
                FileRole::Candidate,
            )
        })
        .collect();
    let play_all = with_chapters(
        media_file(&folder, "title_t00.mkv", 60.0, FileRole::PlayAll),
        &[0.0, 20.0, 40.0],
    );
    files.insert(0, play_all);
    let mut media = FakeMedia::new(scan(&folder, files));
    let mut play_all_audio = Vec::new();
    for i in 0..3 {
        let audio = noise(i as u64 + 7, 20.0);
        play_all_audio.extend_from_slice(&audio);
        media
            .audio
            .insert(FileId(format!("title_t0{}.mkv", i + 1)), audio);
    }
    media
        .audio
        .insert(FileId("title_t00.mkv".into()), play_all_audio);
    let script: Script = (0..3)
        .map(|i| {
            (
                FileId(format!("title_t0{}.mkv", i + 1)),
                story_lines(order[i], 1.0),
            )
        })
        .collect();
    let h = harness(
        media,
        FakeCatalog::with_subtitles(),
        FakeSpeech::new(script),
    );
    let job = h.engine.start_job(request(&folder)).unwrap();
    assert!(matches!(
        h.recorder.wait_for_end(&job).await,
        JobEvent::Finished { .. }
    ));
    let events = h.recorder.events();

    assert!(
        matches!(&events[0], JobEvent::Started { file_ids, .. } if file_ids.len() == 4 && file_ids[3].0 == "title_t00.mkv")
    );
    assert!(events.iter().any(|e| matches!(
        e,
        JobEvent::Stage {
            stage: Stage::DiscOrder,
            state: StageState::Done,
            ..
        }
    )));
    let matches = final_matches(&events);
    assert_eq!(
        matches[&FileId("title_t00.mkv".into())].confidence.verdict,
        Verdict::PlayAll
    );
    assert_eq!(
        matches[&FileId("title_t00.mkv".into())].suggestion,
        Suggestion::PlayAll
    );
    for i in 0..3 {
        let m = &matches[&FileId(format!("title_t0{}.mkv", i + 1))];
        assert_eq!(suggested(m), Some(key(order[i] as u32 + 1)));
        let position = m.candidates[0]
            .evidence
            .play_all_position
            .as_ref()
            .expect("located in the play-all");
        assert_eq!(position.order_index, i as u32);
        assert_eq!(position.chapter, Some(i as u32));
        assert!(
            (position.start_s - 20.0 * i as f64).abs() < 0.5,
            "{position:?}"
        );
    }
    // The play-all itself is only fingerprinted, never transcribed.
    assert!(h.media.decoded_windows("title_t00.mkv").is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uncertain_long_files_are_listened_to_further() {
    let folder = std::path::PathBuf::from("/disc");
    let file = media_file(&folder, "title_t01.mkv", 1200.0, FileRole::Candidate);
    let media = FakeMedia::new(scan(&folder, vec![file]));
    // The four sampled windows (around 180, 480, 780 and 1020 s) hear only the harbour's
    // greeting, which every episode shares; the story is told at 300-330 s, between the first
    // two windows.
    let shared =
        "Welcome back to the harbour where the gulls are singing and the boats are bobbing";
    let mut catalog = FakeCatalog::with_subtitles();
    if let Ok(texts) = catalog.texts.as_mut() {
        for t in texts {
            t.text = format!("{shared}\n{}", t.text);
        }
    }
    let mut lines: Vec<(f64, String)> = [180.0, 480.0, 780.0, 1020.0]
        .iter()
        .map(|t| (*t, shared.to_owned()))
        .collect();
    lines.extend(story_lines(1, 300.0));
    let script: Script = [(FileId("title_t01.mkv".into()), lines)].into();
    let h = harness(media, catalog, FakeSpeech::new(script));
    let job = h.engine.start_job(request(&folder)).unwrap();
    h.recorder.wait_for_end(&job).await;

    let windows = h.media.decoded_windows("title_t01.mkv");
    assert!(
        windows.len() > 4,
        "more windows were transcribed: {windows:?}"
    );
    assert!(
        windows
            .iter()
            .any(|w| w.start_s <= 300.0 && w.end_s >= 320.0)
    );
    let m = &final_matches(&h.recorder.events())[&FileId("title_t01.mkv".into())];
    assert_eq!(suggested(m), Some(key(2)));
    assert_eq!(m.confidence.verdict, Verdict::Confident);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_found_to_be_mostly_music_is_heard_further_without_voice_detection() {
    let folder = std::path::PathBuf::from("/disc");
    // Two rips of one musical episode of a spoken show: the duplicate left without an episode is
    // uncertain, so it is heard further.
    let files = vec![
        media_file(&folder, "title_t01.mkv", 1200.0, FileRole::Candidate),
        media_file(&folder, "title_t02.mkv", 1200.0, FileRole::Candidate),
    ];
    let media = FakeMedia::new(scan(&folder, files));
    let mut lines: Vec<(f64, String)> = Vec::new();
    for t in [180.0, 480.0, 780.0, 1020.0] {
        for k in 0..3 {
            lines.push((t + 5.0 * f64::from(k), "♪♪".to_owned()));
        }
    }
    lines.extend(story_lines(1, 200.0));
    let script: Script = [
        (FileId("title_t01.mkv".into()), lines.clone()),
        (FileId("title_t02.mkv".into()), lines),
    ]
    .into();
    let speech = FakeSpeech::new(script);
    let heard = Arc::clone(&speech.heard);
    let h = harness(media, FakeCatalog::with_subtitles(), speech);
    let job = h.engine.start_job(request(&folder)).unwrap();
    h.recorder.wait_for_end(&job).await;

    let heard = heard.lock().unwrap().clone();
    let planned =
        mi_transcribe::plan_windows(1200.0, &mi_transcribe::SamplingPolicy::default(), true);
    let (first, further) = heard.split_at(2 * planned.len());
    assert!(!further.is_empty(), "{heard:?}");
    assert!(first.iter().all(|(_, _, vad)| *vad), "{first:?}");
    assert!(further.iter().all(|(_, _, vad)| !*vad), "{further:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_read_from_its_own_subtitles_is_not_heard_further() {
    let folder = std::path::PathBuf::from("/disc");
    // Two rips of one episode, both with text subtitles: the duplicate left without an episode is
    // uncertain, but hearing more of it cannot change dialogue read from its subtitles.
    let names = ["title_t01.mkv", "title_t02.mkv"];
    let files = names
        .iter()
        .map(|n| with_text_subtitles(media_file(&folder, n, 1200.0, FileRole::Candidate)))
        .collect();
    let mut media = FakeMedia::new(scan(&folder, files));
    let srt: String = STORIES[3]
        .split(". ")
        .enumerate()
        .map(|(i, line)| {
            format!(
                "{}\n00:00:{:02},000 --> 00:00:{:02},500\n{line}\n\n",
                i + 1,
                i * 3,
                i * 3 + 2
            )
        })
        .collect();
    for n in names {
        media.subtitles.insert(FileId(n.into()), srt.clone());
    }
    let h = harness(
        media,
        FakeCatalog::with_subtitles(),
        FakeSpeech::new(Script::new()),
    );
    let job = h.engine.start_job(request(&folder)).unwrap();
    h.recorder.wait_for_end(&job).await;
    let matches = final_matches(&h.recorder.events());
    assert!(
        matches
            .values()
            .any(|m| m.confidence.verdict == Verdict::Check),
        "{matches:?}"
    );
    let planned =
        mi_transcribe::plan_windows(1200.0, &mi_transcribe::SamplingPolicy::default(), true);
    for n in names {
        assert_eq!(h.media.decoded_windows(n).len(), planned.len(), "{n}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_while_hearing_more_keeps_the_global_assignment() {
    let folder = std::path::PathBuf::from("/disc");
    // Two rips of one episode: matched alone, each claims the episode; the global assignment
    // gives it to one of them.
    let files = vec![
        media_file(&folder, "title_t01.mkv", 1200.0, FileRole::Candidate),
        media_file(&folder, "title_t02.mkv", 1200.0, FileRole::Candidate),
    ];
    let media = FakeMedia::new(scan(&folder, files));
    let lines = story_lines(1, 200.0);
    let script: Script = [
        (FileId("title_t01.mkv".into()), lines.clone()),
        (FileId("title_t02.mkv".into()), lines),
    ]
    .into();
    let mut speech = FakeSpeech::new(script);
    speech.delay = Duration::from_millis(40);
    let h = harness(media, FakeCatalog::with_subtitles(), speech);
    let job = h.engine.start_job(request(&folder)).unwrap();
    // Cancel once further listening has started.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let events = h.recorder.events();
        let matching = events.iter().position(|e| {
            matches!(
                e,
                JobEvent::Stage {
                    stage: Stage::Matching,
                    ..
                }
            )
        });
        let listening_again = matching.is_some_and(|i| {
            events[i..].iter().any(|e| {
                matches!(
                    e,
                    JobEvent::File {
                        status: FileStatus::Listening,
                        ..
                    }
                )
            })
        });
        if listening_again {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{events:?}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    h.engine.cancel_job(&job).unwrap();
    assert!(matches!(
        h.recorder.wait_for_end(&job).await,
        JobEvent::Cancelled { .. }
    ));
    let results = h.engine.job_results(&job).unwrap();
    let suggestions: Vec<_> = results.matches.iter().map(suggested).collect();
    assert_eq!(suggestions.len(), 2);
    assert!(
        suggestions.contains(&None),
        "only one file keeps the episode: {suggestions:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn long_files_are_heard_whole_when_sampling_is_off() {
    let folder = std::path::PathBuf::from("/disc");
    let file = media_file(&folder, "title_t01.mkv", 1200.0, FileRole::Candidate);
    let media = FakeMedia::new(scan(&folder, vec![file]));
    let script: Script = [(FileId("title_t01.mkv".into()), story_lines(0, 600.0))].into();
    let settings = Settings {
        sample_long_files: false,
        ..Settings::default()
    };
    let h = harness_with(
        media,
        FakeCatalog::with_subtitles(),
        FakeSpeech::new(script),
        settings,
    );
    let job = h.engine.start_job(request(&folder)).unwrap();
    h.recorder.wait_for_end(&job).await;
    let windows = h.media.decoded_windows("title_t01.mkv");
    assert_eq!(windows.len(), 1);
    assert_eq!((windows[0].start_s, windows[0].end_s), (0.0, 1200.0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedded_text_subtitles_identify_a_file_with_nothing_heard() {
    let folder = std::path::PathBuf::from("/disc");
    let file = with_text_subtitles(media_file(
        &folder,
        "title_t01.mkv",
        120.0,
        FileRole::Candidate,
    ));
    let mut media = FakeMedia::new(scan(&folder, vec![file]));
    let srt: String = STORIES[3]
        .split(". ")
        .enumerate()
        .map(|(i, line)| {
            format!(
                "{}\n00:00:{:02},000 --> 00:00:{:02},500\n{line}\n\n",
                i + 1,
                i * 3,
                i * 3 + 2
            )
        })
        .collect();
    media.subtitles.insert(FileId("title_t01.mkv".into()), srt);
    let h = harness(
        media,
        FakeCatalog::with_subtitles(),
        FakeSpeech::new(Script::new()),
    );
    let job = h.engine.start_job(request(&folder)).unwrap();
    h.recorder.wait_for_end(&job).await;
    let m = &final_matches(&h.recorder.events())[&FileId("title_t01.mkv".into())];
    assert_eq!(suggested(m), Some(key(4)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_that_cannot_be_decoded_fails_alone() {
    let (mut media, script, folder) = four_files([0, 1, 2, 3]);
    media.broken.insert(FileId("title_t02.mkv".into()));
    let h = harness(
        media,
        FakeCatalog::with_subtitles(),
        FakeSpeech::new(script),
    );
    let job = h.engine.start_job(request(&folder)).unwrap();
    assert!(matches!(
        h.recorder.wait_for_end(&job).await,
        JobEvent::Finished { .. }
    ));
    let events = h.recorder.events();
    assert!(events.iter().any(|e| matches!(e,
        JobEvent::File { file_id, status: FileStatus::Failed, .. } if file_id.0 == "title_t02.mkv")));
    let matches = final_matches(&events);
    assert_ne!(
        matches[&FileId("title_t02.mkv".into())].confidence.verdict,
        Verdict::Confident
    );
    for (i, n) in [(1, 1), (3, 3), (4, 4)] {
        assert_eq!(
            suggested(&matches[&FileId(format!("title_t0{i}.mkv"))]),
            Some(key(n))
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_reference_text_the_job_continues_with_titles_and_summaries() {
    let (media, mut script, folder) = four_files([0, 1, 2, 3]);
    // Each file now also says its title, as many shows do.
    for (i, title) in TITLES.iter().enumerate() {
        script
            .get_mut(&FileId(format!("title_t0{}.mkv", i + 1)))
            .unwrap()
            .push((100.0, format!("Today's story is {title}.")));
    }
    let catalog = FakeCatalog {
        episodes: Ok(episodes()),
        texts: Err("SubDL is down".into()),
        panics: false,
    };
    let h = harness(media, catalog, FakeSpeech::new(script));
    let job = h.engine.start_job(request(&folder)).unwrap();
    assert!(matches!(
        h.recorder.wait_for_end(&job).await,
        JobEvent::Finished { .. }
    ));
    let events = h.recorder.events();
    assert!(events.iter().any(|e| matches!(
        e,
        JobEvent::Stage {
            stage: Stage::Subtitles,
            state: StageState::Failed { .. },
            ..
        }
    )));
    let matches = final_matches(&events);
    for i in 0..4 {
        assert_eq!(
            suggested(&matches[&FileId(format!("title_t0{}.mkv", i + 1))]),
            Some(key(i as u32 + 1))
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_episode_list_fails_the_job_with_a_reason() {
    let (media, script, folder) = four_files([0, 1, 2, 3]);
    let catalog = FakeCatalog {
        episodes: Err("connection refused".into()),
        texts: Ok(Vec::new()),
        panics: false,
    };
    let h = harness(media, catalog, FakeSpeech::new(script));
    let job = h.engine.start_job(request(&folder)).unwrap();
    let end = h.recorder.wait_for_end(&job).await;
    let JobEvent::Failed { message, .. } = end else {
        panic!("expected Failed, got {end:?}")
    };
    assert!(
        message.starts_with("Couldn't get the episode list"),
        "{message}"
    );
    assert!(h.recorder.events().iter().any(|e| matches!(
        e,
        JobEvent::Stage {
            stage: Stage::EpisodeList,
            state: StageState::Failed { .. },
            ..
        }
    )));
    assert!(!h.engine.is_job_running());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_panic_inside_a_job_fails_it_and_frees_the_engine() {
    let (media, script, folder) = four_files([0, 1, 2, 3]);
    let mut catalog = FakeCatalog::with_subtitles();
    catalog.panics = true;
    let h = harness(media, catalog, FakeSpeech::new(script));
    let job = h.engine.start_job(request(&folder)).unwrap();
    let end = h.recorder.wait_for_end(&job).await;
    assert!(
        matches!(&end, JobEvent::Failed { message, .. } if message.contains("internal error")),
        "{end:?}"
    );
    assert!(!h.engine.is_job_running());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_speech_model_fails_the_job_before_it_starts() {
    let (media, script, folder) = four_files([0, 1, 2, 3]);
    let mut speech = FakeSpeech::new(script);
    speech.missing_model = true;
    let h = harness(media, FakeCatalog::with_subtitles(), speech);
    let job = h.engine.start_job(request(&folder)).unwrap();
    let end = h.recorder.wait_for_end(&job).await;
    assert!(
        matches!(&end, JobEvent::Failed { message, .. } if message.contains("speech model is not downloaded"))
    );
    assert!(
        !h.recorder
            .events()
            .iter()
            .any(|e| matches!(e, JobEvent::Started { .. }))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn seasons_limit_the_candidate_episodes() {
    let (media, script, folder) = four_files([0, 1, 2, 3]);
    let mut catalog = FakeCatalog::with_subtitles();
    let mut list = episodes();
    let mut special = episode(0);
    special.key = EpisodeKey {
        season: 0,
        number: 1,
    };
    special.provider_episode_id = "special".into();
    list.insert(0, special);
    catalog.episodes = Ok(list);
    let h = harness(media, catalog, FakeSpeech::new(script));
    let mut req = request(&folder);
    req.seasons = Some(vec![1]);
    let job = h.engine.start_job(req).unwrap();
    h.recorder.wait_for_end(&job).await;
    let results = h.engine.job_results(&job).unwrap();
    assert_eq!(results.episodes.len(), 4);
    assert!(results.episodes.iter().all(|e| e.key.season == 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_stops_the_job_and_frees_the_engine() {
    let (media, script, folder) = four_files([0, 1, 2, 3]);
    let mut speech = FakeSpeech::new(script);
    speech.delay = Duration::from_secs(5);
    let h = harness(media, FakeCatalog::with_subtitles(), speech);
    let job = h.engine.start_job(request(&folder)).unwrap();
    assert!(h.engine.is_job_running());
    assert!(matches!(
        h.engine.start_job(request(&folder)),
        Err(mi_core::CoreError::Busy)
    ));
    // Wait until it is listening, then cancel.
    for _ in 0..500 {
        if h.recorder.events().iter().any(|e| {
            matches!(
                e,
                JobEvent::File {
                    status: FileStatus::Listening,
                    ..
                }
            )
        }) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let started = std::time::Instant::now();
    h.engine.cancel_job(&job).unwrap();
    assert!(matches!(
        h.recorder.wait_for_end(&job).await,
        JobEvent::Cancelled { .. }
    ));
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "cancelled within one window"
    );
    assert!(!h.engine.is_job_running());
    assert!(!h.engine.job_results(&job).unwrap().complete);
    // Cancelling again, or a finished job, is harmless; an unknown job is not found.
    h.engine.cancel_job(&job).unwrap();
    assert!(
        h.engine
            .cancel_job(&mi_types::JobId("job-unknown".into()))
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn finished_jobs_are_saved_and_listed_newest_first() {
    let (media, script, folder) = four_files([0, 1, 2, 3]);
    let h = harness(
        media,
        FakeCatalog::with_subtitles(),
        FakeSpeech::new(script.clone()),
    );
    let first = h.engine.start_job(request(&folder)).unwrap();
    h.recorder.wait_for_end(&first).await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    let second = h.engine.start_job(request(&folder)).unwrap();
    h.recorder.wait_for_end(&second).await;

    let recent = h.engine.recent_jobs().unwrap();
    assert_eq!(
        recent.iter().map(|r| r.job_id.clone()).collect::<Vec<_>>(),
        vec![second.clone(), first.clone()]
    );
    assert_eq!(recent[0].show_name, "Harbour Tales");
    assert_eq!(recent[0].file_count, 4);
    assert!(!recent[0].saved);

    // A new engine over the same data folder (the app relaunched) still has them.
    let (media2, _, _) = four_files([0, 1, 2, 3]);
    let services = mi_core::Services {
        media: std::sync::Arc::new(media2),
        catalog: std::sync::Arc::new(FakeCatalog::with_subtitles()),
        speech: std::sync::Arc::new(FakeSpeech::new(script)),
    };
    let reopened = mi_core::Engine::with_services(
        h.data.path(),
        Settings::default(),
        tokio::runtime::Handle::current(),
        services,
        std::sync::Arc::new(Recorder::default()),
    );
    assert_eq!(reopened.recent_jobs().unwrap().len(), 2);
    assert_eq!(
        reopened.job_results(&first).unwrap(),
        h.engine.job_results(&first).unwrap()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn approved_files_are_renamed_in_place_and_undone_from_history() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("SCHOOL_D1");
    std::fs::create_dir(&folder).unwrap();
    let mut files = Vec::new();
    for i in 1..=3 {
        let name = format!("title_t0{i}.mkv");
        std::fs::write(folder.join(&name), format!("video {i}")).unwrap();
        let mut file = media_file(&folder, &name, 120.0, FileRole::Candidate);
        file.size_bytes = 7;
        files.push(file);
    }
    let media = FakeMedia::new(scan(&folder, files));
    let script: Script = (0..3)
        .map(|i| {
            (
                FileId(format!("title_t0{}.mkv", i + 1)),
                story_lines(2 - i, 10.0),
            )
        })
        .collect();
    let h = harness(
        media,
        FakeCatalog::with_subtitles(),
        FakeSpeech::new(script),
    );
    let job = h.engine.start_job(request(&folder)).unwrap();
    h.recorder.wait_for_end(&job).await;

    let decisions = vec![
        FileDecision {
            file_id: FileId("title_t01.mkv".into()),
            decision: ReviewDecision::Approved { episode: key(3) },
        },
        FileDecision {
            file_id: FileId("title_t02.mkv".into()),
            decision: ReviewDecision::Approved { episode: key(2) },
        },
        FileDecision {
            file_id: FileId("title_t03.mkv".into()),
            decision: ReviewDecision::Skip,
        },
    ];
    let request = RenamePlanRequest {
        job_id: job.clone(),
        decisions,
        mode: SaveMode::RenameInPlace {
            root: folder.clone(),
        },
        naming: NamingScheme::JellyfinPlex,
        save_heard_subtitles: true,
    };
    let plan = h.engine.plan_rename(&request).unwrap();
    assert!(plan.conflicts.is_empty());
    assert_eq!(plan.items.len(), 2);

    // A plan that differs from what the job's record gives is refused: a changed source or
    // target, a conflict removed, or a plan for an earlier version of the request.
    let mut tampered = plan.clone();
    tampered.items[0].from = dir.path().join("elsewhere.mkv");
    let mut retargeted = plan.clone();
    retargeted.items[0].to = dir.path().join("anywhere.mkv");
    let mut stale = plan.clone();
    stale.request.naming = NamingScheme::Kodi;
    let mut play_all = plan.clone();
    play_all.items.push(play_all.items[0].clone());
    for bad in [tampered, retargeted, stale, play_all] {
        assert!(matches!(
            h.engine.apply_rename(bad).await,
            Err(mi_core::CoreError::Invalid(_))
        ));
    }
    assert!(folder.join("title_t01.mkv").exists(), "nothing was renamed");

    let outcome = h.engine.apply_rename(plan).await.unwrap();
    assert_eq!(outcome.completed, 2);
    assert!(outcome.failed.is_empty());
    let target = folder
        .join("Harbour Tales (1999)")
        .join("Season 01")
        .join("Harbour Tales (1999) - S01E03 - The Storm at Midnight.mkv");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "video 1");
    let heard = std::fs::read_to_string(target.with_extension("srt")).unwrap();
    assert!(
        heard.contains("Thunder rolled across the harbour"),
        "{heard}"
    );
    assert!(folder.join("title_t03.mkv").exists(), "skipped files stay");
    assert!(h.engine.recent_jobs().unwrap()[0].saved);

    let history = h.engine.history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].show_name, "Harbour Tales");
    let undone = h.engine.undo(&history[0].id).await.unwrap();
    assert!(undone.failed.is_empty(), "{undone:?}");
    assert_eq!(
        std::fs::read_to_string(folder.join("title_t01.mkv")).unwrap(),
        "video 1"
    );
    assert!(!target.exists());

    // Once undone, a second plan sees no conflicts with the old targets.
    let again = h.engine.plan_rename(&request).unwrap();
    assert!(
        !again
            .conflicts
            .iter()
            .any(|c| matches!(c, PlanConflict::TargetExists { .. }))
    );

    // A new rip with the same file name is not the file that was identified.
    std::fs::write(folder.join("title_t01.mkv"), "another disc").unwrap();
    let changed = h.engine.plan_rename(&request).unwrap();
    assert!(changed.conflicts.iter().any(|c| matches!(
        c,
        PlanConflict::SourceChanged { file_id, .. } if file_id.0 == "title_t01.mkv"
    )));
    assert!(matches!(
        h.engine.apply_rename(again).await,
        Err(mi_core::CoreError::Invalid(_)) | Err(mi_core::CoreError::Rename(_))
    ));
    assert_eq!(
        std::fs::read_to_string(folder.join("title_t01.mkv")).unwrap(),
        "another disc"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn searching_the_folder_guess_marks_the_results() {
    let (media, script, folder) = four_files([0, 1, 2, 3]);
    let h = harness(
        media,
        FakeCatalog::with_subtitles(),
        FakeSpeech::new(script),
    );
    let scanned = h.engine.scan(&folder).await.unwrap();
    assert_eq!(scanned.show_guess.as_deref(), Some("Harbour Tales"));
    assert!(h.engine.search_shows("harbour tales").await.unwrap()[0].guessed_from_folder);
    assert!(!h.engine.search_shows("Harbour").await.unwrap()[0].guessed_from_folder);
}
