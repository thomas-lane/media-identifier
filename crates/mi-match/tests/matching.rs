//! End-to-end matching on a fictional show with simulated speech-recognition errors.
//!
//! These tests check that the method behaves as designed on controlled data: misheard
//! transcripts still find their episode, bonus features become extras, and the play-all order is
//! used only when it agrees with the dialogue. They do not measure accuracy on real recordings.

mod common;

use common::*;
use mi_match::{
    DiscOrder, DiscOrderProblem, DiscOrderUse, EpisodeInput, FileInput, MatchConfig, MatchError,
    MatchInput, match_files, match_with_outcome, needs_more_listening, score_all,
};
use mi_types::{
    CancelFlag, EpisodeKey, EvidenceNote, FileId, FileMatch, PlayAllPosition, Suggestion, TextKind,
    Verdict,
};

fn key(i: usize) -> EpisodeKey {
    EpisodeKey {
        season: 1,
        number: i as u32 + 1,
    }
}

fn suggested(m: &FileMatch) -> Option<EpisodeKey> {
    match m.suggestion {
        Suggestion::Episode { episode } => Some(episode),
        _ => None,
    }
}

/// Files named so that name order says nothing: file `t0N` holds episode `ORDER[N]`.
const ORDER: [usize; 10] = [6, 2, 9, 0, 4, 7, 1, 8, 3, 5];

fn misheard_files(errors: Errors, seed: u64) -> Vec<FileInput> {
    ORDER
        .iter()
        .enumerate()
        .map(|(n, &ep)| {
            file(
                &format!("t{n:02}.mkv"),
                mishear(&episode_text(ep), errors, seed + n as u64),
                RUNTIME_S - 10.0,
            )
        })
        .collect()
}

fn run(input: &MatchInput) -> Vec<FileMatch> {
    match_files(input, &MatchConfig::default(), &CancelFlag::new()).expect("matched")
}

#[test]
fn the_simulator_really_mishears() {
    // Guards the fixture: the tests below would be meaningless with clean transcripts.
    for ep in 0..10 {
        let clean = episode_text(ep);
        let moderate = word_error_rate(&clean, &mishear(&clean, Errors::MODERATE, 7));
        let heavy = word_error_rate(&clean, &mishear(&clean, Errors::HEAVY, 7));
        assert!((0.15..0.45).contains(&moderate), "episode {ep}: {moderate}");
        assert!(heavy > 0.3, "episode {ep}: {heavy}");
        assert!(heavy > moderate);
    }
}

#[test]
fn misheard_transcripts_match_their_episodes_confidently() {
    for seed in [1, 100, 2000] {
        let input = MatchInput {
            files: misheard_files(Errors::MODERATE, seed),
            episodes: episodes_with_subtitles(),
            disc_order: None,
        };
        let matches = run(&input);
        for (n, m) in matches.iter().enumerate() {
            assert_eq!(
                suggested(m),
                Some(key(ORDER[n])),
                "seed {seed} file {n}: {m:?}"
            );
            assert_eq!(
                m.confidence.verdict,
                Verdict::Confident,
                "seed {seed}: {m:?}"
            );
        }
    }
}

#[test]
fn heavily_misheard_transcripts_still_match() {
    for seed in [3, 300] {
        let input = MatchInput {
            files: misheard_files(Errors::HEAVY, seed),
            episodes: episodes_with_subtitles(),
            disc_order: None,
        };
        for (n, m) in run(&input).iter().enumerate() {
            assert_eq!(
                suggested(m),
                Some(key(ORDER[n])),
                "seed {seed} file {n}: {m:?}"
            );
            assert_ne!(m.confidence.verdict, Verdict::Extra);
        }
    }
}

#[test]
fn sampled_windows_of_an_episode_still_match() {
    // A quarter of each episode, from the middle, as one sample window would give.
    let files: Vec<FileInput> = ORDER
        .iter()
        .enumerate()
        .map(|(n, &ep)| {
            let mut f = file(
                &format!("t{n:02}.mkv"),
                mishear(
                    &excerpt(&episode_text(ep), 0.4, 0.25),
                    Errors::MODERATE,
                    n as u64,
                ),
                RUNTIME_S,
            );
            f.sampled_windows = Some(1);
            f
        })
        .collect();
    let input = MatchInput {
        files,
        episodes: episodes_with_subtitles(),
        disc_order: None,
    };
    for (n, m) in run(&input).iter().enumerate() {
        assert_eq!(suggested(m), Some(key(ORDER[n])), "file {n}: {m:?}");
        assert!(
            m.candidates[0]
                .evidence
                .notes
                .contains(&EvidenceNote::Sampled { windows: 1 })
        );
    }
}

#[test]
fn a_bonus_feature_becomes_an_extra() {
    let mut files = misheard_files(Errors::MODERATE, 5);
    files.push(file(
        "t10.mkv",
        mishear(FEATURETTE, Errors::MODERATE, 11),
        RUNTIME_S - 20.0,
    ));
    let input = MatchInput {
        files,
        episodes: episodes_with_subtitles(),
        disc_order: None,
    };
    let matches = run(&input);
    let extra = &matches[10];
    assert_eq!(extra.suggestion, Suggestion::NotAnEpisode, "{extra:?}");
    assert_eq!(extra.confidence.verdict, Verdict::Extra);
    // The episodes are unaffected.
    for (n, m) in matches[..10].iter().enumerate() {
        assert_eq!(suggested(m), Some(key(ORDER[n])));
    }
}

#[test]
fn the_shared_theme_alone_is_not_an_episode() {
    // A menu loop or trailer that only plays the theme song.
    let theme = format!("{THEME_OPEN} {THEME_OPEN} {THEME_CLOSE}");
    let input = MatchInput {
        files: vec![file("menu.mkv", mishear(&theme, Errors::MODERATE, 2), 45.0)],
        episodes: episodes_with_subtitles(),
        disc_order: None,
    };
    let m = &run(&input)[0];
    assert_eq!(m.confidence.verdict, Verdict::Extra, "{m:?}");
}

#[test]
fn a_duplicate_file_does_not_take_a_second_episode() {
    let mut files = misheard_files(Errors::MODERATE, 8);
    files.push(file(
        "t10.mkv",
        mishear(&episode_text(ORDER[0]), Errors::MODERATE, 999),
        RUNTIME_S,
    ));
    let input = MatchInput {
        files,
        episodes: episodes_with_subtitles(),
        disc_order: None,
    };
    let matches = run(&input);
    let first = &matches[0];
    let copy = &matches[10];
    let episodes = [suggested(first), suggested(copy)];
    // One of the two gets the episode; the other is left for the user to check, not an extra.
    assert!(episodes.contains(&Some(key(ORDER[0]))), "{episodes:?}");
    assert!(episodes.contains(&None), "{episodes:?}");
    let loser = if suggested(first).is_none() {
        first
    } else {
        copy
    };
    assert_eq!(loser.confidence.verdict, Verdict::Check);
    assert!(
        needs_more_listening(&matches, &MatchConfig::default()).contains(&loser.file_id),
        "an unresolved duplicate is worth hearing more of"
    );
}

#[test]
fn embedded_subtitles_replace_the_transcript_for_dialogue() {
    let mut files = misheard_files(Errors::HEAVY, 21);
    // File 0's transcript is useless, but it carries its own subtitles.
    files[0].transcript = "music music music".into();
    files[0].embedded_text = Some(episode_text(ORDER[0]));
    let input = MatchInput {
        files,
        episodes: episodes_with_subtitles(),
        disc_order: None,
    };
    let m = &run(&input)[0];
    assert_eq!(suggested(m), Some(key(ORDER[0])));
    assert_eq!(m.confidence.verdict, Verdict::Confident);
    assert!(m.candidates[0].evidence.signals.dialogue.unwrap() > 0.9);
}

#[test]
fn songs_without_reference_text_match_by_their_title() {
    // A disc of short songs that sing their titles; no lyrics or subtitles are available.
    let songs = [
        "Conjunction Junction",
        "Three Is a Magic Number",
        "Interplanet Janet",
        "Lolly, Lolly, Lolly, Get Your Adverbs Here",
    ];
    let lyrics = [
        "conjunction junction whats your function hooking up words and phrases and clauses",
        "3 is a magic number yes it is its a magic number somewhere in that ancient mystic trinity",
        "inter planet janet she's a galaxy girl a solar system miss from a future world",
        "lolly lolly lolly get your adverbs here father and son and lolly too",
    ];
    let episodes: Vec<EpisodeInput> = songs
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut e = episode(i);
            e.title = (*t).into();
            e.runtime_s = Some(180.0);
            EpisodeInput {
                episode: e,
                texts: vec![],
            }
        })
        .collect();
    let order = [2, 0, 3, 1];
    let files: Vec<FileInput> = order
        .iter()
        .enumerate()
        .map(|(n, &s)| {
            let mut f = file(
                &format!("song{n}.mkv"),
                mishear(lyrics[s], Errors::MODERATE, n as u64 + 40),
                175.0,
            );
            f.mostly_music = true;
            f
        })
        .collect();
    let matches = run(&MatchInput {
        files,
        episodes,
        disc_order: None,
    });
    for (n, m) in matches.iter().enumerate() {
        assert_eq!(suggested(m), Some(key(order[n])), "song {n}: {m:?}");
        let ev = &m.candidates[0].evidence;
        assert!(ev.notes.contains(&EvidenceNote::NoReferenceText));
        assert!(ev.notes.contains(&EvidenceNote::MostlyMusic));
        assert!(ev.signals.dialogue.is_none());
    }
    // The title was heard in each song, so each lists that as evidence.
    let heard: usize = matches
        .iter()
        .filter(|m| {
            m.candidates[0]
                .evidence
                .notes
                .contains(&EvidenceNote::TitleHeard)
        })
        .count();
    assert!(heard >= 3, "{heard}");
}

#[test]
fn summaries_are_used_when_no_dialogue_text_exists() {
    let summaries = [
        "Grandpa Owl tells how a boastful hare naps during a race against a patient tortoise.",
        "A shepherd boy tricks the villagers with false alarms about a wolf.",
        "A thirsty crow drops pebbles into a pitcher to raise the water.",
    ];
    let episodes: Vec<EpisodeInput> = [0usize, 1, 8]
        .iter()
        .zip(summaries)
        .map(|(&i, s)| {
            let mut e = episode(i);
            e.summary = Some(s.into());
            EpisodeInput {
                episode: e,
                texts: vec![],
            }
        })
        .collect();
    let files = vec![
        file(
            "a.mkv",
            mishear(&episode_text(8), Errors::MODERATE, 1),
            RUNTIME_S,
        ),
        file(
            "b.mkv",
            mishear(&episode_text(0), Errors::MODERATE, 2),
            RUNTIME_S,
        ),
        file(
            "c.mkv",
            mishear(&episode_text(1), Errors::MODERATE, 3),
            RUNTIME_S,
        ),
    ];
    let matches = run(&MatchInput {
        files,
        episodes: episodes.clone(),
        disc_order: None,
    });
    let expected = [
        episodes[2].episode.key,
        episodes[0].episode.key,
        episodes[1].episode.key,
    ];
    for (m, want) in matches.iter().zip(expected) {
        assert_eq!(suggested(m), Some(want), "{m:?}");
        // The quotes highlight summary words that were heard.
        let ev = &m.candidates[0].evidence;
        assert!(ev.reference.iter().any(|p| p.matched), "{ev:?}");
    }
}

#[test]
fn evidence_quotes_highlight_the_overlap() {
    let input = MatchInput {
        files: misheard_files(Errors::MODERATE, 77),
        episodes: episodes_with_subtitles(),
        disc_order: None,
    };
    for m in run(&input) {
        let best = &m.candidates[0];
        let ev = &best.evidence;
        assert!(ev.heard.iter().any(|p| p.matched), "{ev:?}");
        assert!(ev.reference.iter().any(|p| p.matched), "{ev:?}");
        let heard: String = ev.heard.iter().map(|p| p.text.as_str()).collect();
        assert!(heard.split_whitespace().count() <= 30, "{heard}");
        // Candidates are sorted, capped, and include the suggestion.
        assert!(m.candidates.len() <= 5);
        assert!(m.candidates.windows(2).all(|w| w[0].score >= w[1].score));
        assert_eq!(Some(best.episode), suggested(&m));
    }
}

fn position(order_index: u32, start_s: f64) -> PlayAllPosition {
    PlayAllPosition {
        chapter: Some(order_index),
        start_s,
        end_s: start_s + RUNTIME_S,
        order_index,
        alignment_score: 0.9,
    }
}

/// A disc holding episodes 3 to 8 in order, in files whose names are shuffled.
fn disc_files(errors: Errors) -> (Vec<FileInput>, DiscOrder, Vec<usize>) {
    let on_disc = [2usize, 3, 4, 5, 6, 7];
    let names = [4usize, 0, 5, 2, 1, 3]; // file name index for each disc position
    let mut files: Vec<FileInput> = (0..6)
        .map(|n| file(&format!("t{n:02}.mkv"), String::new(), RUNTIME_S))
        .collect();
    let mut episode_of_file = vec![0; 6];
    let mut positions = Vec::new();
    for (rank, (&ep, &name)) in on_disc.iter().zip(&names).enumerate() {
        files[name].transcript = mishear(&episode_text(ep), errors, 50 + rank as u64);
        episode_of_file[name] = ep;
        positions.push((
            files[name].file_id.clone(),
            position(rank as u32, rank as f64 * RUNTIME_S),
        ));
    }
    (
        files,
        DiscOrder {
            positions,
            trustworthy: true,
            problem: None,
        },
        episode_of_file,
    )
}

#[test]
fn disc_order_fills_in_a_file_the_dialogue_cannot_identify() {
    let (mut files, order, expected) = disc_files(Errors::MODERATE);
    // One file in the middle of the disc was heard as almost nothing useful.
    let weak = files
        .iter()
        .position(|f| f.file_id == order.positions[3].0)
        .unwrap();
    files[weak].transcript = "la la la mm hmm oh yeah la la uh huh oh".into();
    let input = MatchInput {
        files,
        episodes: episodes_with_subtitles(),
        disc_order: Some(order),
    };
    let outcome = match_with_outcome(&input, &MatchConfig::default(), &CancelFlag::new()).unwrap();
    assert_eq!(outcome.disc_order, DiscOrderUse::Used);
    for (f, m) in outcome.matches.iter().enumerate() {
        assert_eq!(suggested(m), Some(key(expected[f])), "file {f}: {m:?}");
    }
    let m = &outcome.matches[weak];
    let ev = &m.candidates[0].evidence;
    assert!(
        ev.notes
            .contains(&EvidenceNote::DiscOrderAgrees { chapter: Some(3) }),
        "{ev:?}"
    );
    assert_eq!(ev.play_all_position.as_ref().unwrap().order_index, 3);
    // Order alone is not proof: the user is asked to check.
    assert_ne!(m.confidence.verdict, Verdict::Confident);
}

#[test]
fn a_shuffled_play_all_is_ignored() {
    let (files, mut order, expected) = disc_files(Errors::MODERATE);
    // Reverse the play-all: its order now contradicts every confident match.
    let n = order.positions.len() as u32;
    for (_, p) in &mut order.positions {
        p.order_index = n - 1 - p.order_index;
    }
    let input = MatchInput {
        files,
        episodes: episodes_with_subtitles(),
        disc_order: Some(order),
    };
    let outcome = match_with_outcome(&input, &MatchConfig::default(), &CancelFlag::new()).unwrap();
    assert_eq!(
        outcome.disc_order,
        DiscOrderUse::Ignored(DiscOrderProblem::ShuffledAgainstContent)
    );
    for (f, m) in outcome.matches.iter().enumerate() {
        assert_eq!(suggested(m), Some(key(expected[f])), "file {f}: {m:?}");
        let ev = &m.candidates[0].evidence;
        assert!(ev.notes.contains(&EvidenceNote::PlayAllIgnored));
        assert!(ev.signals.disc_order.is_none());
    }
}

#[test]
fn an_untrustworthy_disc_order_is_ignored() {
    let (files, mut order, expected) = disc_files(Errors::MODERATE);
    order.trustworthy = false;
    order.problem = Some(DiscOrderProblem::Overlapping);
    let input = MatchInput {
        files,
        episodes: episodes_with_subtitles(),
        disc_order: Some(order),
    };
    let outcome = match_with_outcome(&input, &MatchConfig::default(), &CancelFlag::new()).unwrap();
    assert_eq!(
        outcome.disc_order,
        DiscOrderUse::Ignored(DiscOrderProblem::Overlapping)
    );
    for (f, m) in outcome.matches.iter().enumerate() {
        assert_eq!(suggested(m), Some(key(expected[f])));
    }
}

#[test]
fn score_matrix_covers_every_pair() {
    let input = MatchInput {
        files: misheard_files(Errors::MODERATE, 4),
        episodes: episodes_with_subtitles(),
        disc_order: None,
    };
    let matrix = score_all(&input, &MatchConfig::default(), &CancelFlag::new()).unwrap();
    assert_eq!(matrix.files.len(), 10);
    assert_eq!(matrix.episodes.len(), 10);
    for (f, row) in matrix.cells.iter().enumerate() {
        assert_eq!(row.len(), 10);
        let best = row
            .iter()
            .max_by(|a, b| a.score.total_cmp(&b.score))
            .unwrap();
        assert_eq!(best.episode, key(ORDER[f]));
        for c in row {
            assert!((0.0..=1.0).contains(&c.score));
            let s = c.evidence.signals;
            assert!(s.dialogue.is_some() && s.title_hook.is_some() && s.duration.is_some());
            assert!(s.disc_order.is_none());
        }
    }
}

#[test]
fn files_with_no_speech_are_matched_by_what_remains() {
    let input = MatchInput {
        files: vec![file("silent.mkv", String::new(), RUNTIME_S)],
        episodes: episodes_with_subtitles(),
        disc_order: None,
    };
    let m = &run(&input)[0];
    let ev = &m.candidates[0].evidence;
    assert!(ev.notes.contains(&EvidenceNote::NoSpeech));
    assert!(ev.signals.dialogue.is_none() && ev.signals.title_hook.is_none());
    // Length alone fits all ten episodes equally, so nothing is confident.
    assert_ne!(m.confidence.verdict, Verdict::Confident);
}

#[test]
fn no_candidate_episodes_makes_every_file_an_extra() {
    let input = MatchInput {
        files: misheard_files(Errors::MODERATE, 1),
        episodes: vec![],
        disc_order: None,
    };
    for m in run(&input) {
        assert_eq!(m.suggestion, Suggestion::NotAnEpisode);
        assert_eq!(m.confidence.verdict, Verdict::Extra);
        assert!(m.candidates.is_empty());
    }
}

#[test]
fn lyrics_count_as_dialogue() {
    let mut episodes = episodes_with_subtitles();
    episodes[0].texts = vec![reference(0, TextKind::Lyrics, episode_text(0))];
    let input = MatchInput {
        files: vec![file(
            "a.mkv",
            mishear(&episode_text(0), Errors::MODERATE, 6),
            RUNTIME_S,
        )],
        episodes,
        disc_order: None,
    };
    let m = &run(&input)[0];
    assert_eq!(suggested(m), Some(key(0)));
}

#[test]
fn cancellation_stops_matching() {
    let input = MatchInput {
        files: misheard_files(Errors::MODERATE, 1),
        episodes: episodes_with_subtitles(),
        disc_order: None,
    };
    let cancel = CancelFlag::new();
    cancel.cancel();
    assert!(matches!(
        match_files(&input, &MatchConfig::default(), &cancel),
        Err(MatchError::Cancelled)
    ));
}

#[test]
fn inconsistent_input_is_rejected() {
    let mut files = misheard_files(Errors::MODERATE, 1);
    files[1].file_id = files[0].file_id.clone();
    let input = MatchInput {
        files,
        episodes: episodes_with_subtitles(),
        disc_order: None,
    };
    assert!(matches!(
        match_files(&input, &MatchConfig::default(), &CancelFlag::new()),
        Err(MatchError::InvalidInput(_))
    ));

    let input = MatchInput {
        files: misheard_files(Errors::MODERATE, 1),
        episodes: episodes_with_subtitles(),
        disc_order: Some(DiscOrder {
            positions: vec![(FileId("missing.mkv".into()), position(0, 0.0))],
            trustworthy: true,
            problem: None,
        }),
    };
    assert!(matches!(
        match_files(&input, &MatchConfig::default(), &CancelFlag::new()),
        Err(MatchError::InvalidInput(_))
    ));
}

/// A disc order with the given files at consecutive chapters (`(file index, chapter)`), each a
/// full episode long.
fn order_at(files: &[FileInput], at: &[(usize, u32)]) -> DiscOrder {
    DiscOrder {
        positions: at
            .iter()
            .enumerate()
            .map(|(rank, &(f, chapter))| {
                let start_s = f64::from(chapter) * RUNTIME_S;
                (
                    files[f].file_id.clone(),
                    PlayAllPosition {
                        chapter: Some(chapter),
                        start_s,
                        end_s: start_s + RUNTIME_S,
                        order_index: rank as u32,
                        alignment_score: 0.9,
                    },
                )
            })
            .collect(),
        trustworthy: true,
        problem: None,
    }
}

const FEW_WORDS: &str = "la la la mm hmm oh yeah la la uh huh oh";

#[test]
fn a_heard_title_raises_its_episode_when_too_few_words_were_heard_for_dialogue() {
    // A sung short: fewer content words than dialogue needs, but its title is clear.
    let input = MatchInput {
        files: vec![file(
            "song.mkv",
            "la la the lion and the mouse la la oh".into(),
            RUNTIME_S,
        )],
        episodes: episodes_with_subtitles(),
        disc_order: None,
    };
    let m = &run(&input)[0];
    assert_eq!(suggested(m), Some(key(3)), "{m:?}");
    assert_eq!(m.candidates[0].episode, key(3));
    let other = m.candidates.iter().find(|c| c.episode != key(3)).unwrap();
    assert!(m.candidates[0].score > other.score, "{m:?}");
    assert_eq!(other.evidence.signals.title_hook, Some(0.0));
}

#[test]
fn shared_theme_lines_alone_are_not_dialogue() {
    // Opening credits or a theme-song video: only lines every episode shares, heard cleanly.
    let theme = format!("{THEME_OPEN} {THEME_CLOSE}");
    for duration_s in [95.0, RUNTIME_S] {
        let input = MatchInput {
            files: vec![file("credits.mkv", theme.clone(), duration_s)],
            episodes: episodes_with_subtitles(),
            disc_order: None,
        };
        let m = &run(&input)[0];
        let floor = MatchConfig::default().identity_floor;
        for c in &m.candidates {
            let d = c.evidence.signals.dialogue.unwrap();
            assert!(
                d < floor,
                "{duration_s} s: dialogue {d} for {:?}",
                c.episode
            );
        }
        assert_eq!(
            m.suggestion,
            Suggestion::NotAnEpisode,
            "{duration_s} s: {m:?}"
        );
    }
}

#[test]
fn a_located_file_out_of_order_still_gets_its_free_episode() {
    // Play-all order: episodes 1, 2, 6, 3, 4. Four of five anchors are in order, so the order is
    // used, and it cannot place the file holding episode 6.
    let episodes_on_disc = [0usize, 1, 5, 2, 3];
    let files: Vec<FileInput> = episodes_on_disc
        .iter()
        .enumerate()
        .map(|(n, &ep)| {
            file(
                &format!("t{n:02}.mkv"),
                mishear(&episode_text(ep), Errors::MODERATE, 70 + n as u64),
                RUNTIME_S - 10.0,
            )
        })
        .collect();
    let order = order_at(&files, &[(0, 0), (1, 1), (2, 2), (3, 3), (4, 4)]);
    let input = MatchInput {
        files,
        episodes: episodes_with_subtitles(),
        disc_order: Some(order),
    };
    let outcome = match_with_outcome(&input, &MatchConfig::default(), &CancelFlag::new()).unwrap();
    assert_eq!(outcome.disc_order, DiscOrderUse::Used);
    for (f, m) in outcome.matches.iter().enumerate() {
        assert_eq!(
            suggested(m),
            Some(key(episodes_on_disc[f])),
            "file {f}: {m:?}"
        );
    }
}

#[test]
fn a_special_at_the_end_of_the_disc_keeps_its_own_episode() {
    // Episode lists sort specials (season 0) first, whatever their place on the disc.
    let mut episodes = episodes_with_subtitles();
    let mut special = episodes.remove(9);
    special.episode.key = EpisodeKey {
        season: 0,
        number: 1,
    };
    for t in &mut special.texts {
        t.episode = special.episode.key;
    }
    episodes.insert(0, special);
    let on_disc = [0usize, 1, 2, 9];
    let files: Vec<FileInput> = on_disc
        .iter()
        .enumerate()
        .map(|(n, &ep)| {
            file(
                &format!("t{n:02}.mkv"),
                mishear(&episode_text(ep), Errors::MODERATE, 80 + n as u64),
                RUNTIME_S - 10.0,
            )
        })
        .collect();
    let order = order_at(&files, &[(0, 0), (1, 1), (2, 2), (3, 3)]);
    let input = MatchInput {
        files,
        episodes,
        disc_order: Some(order),
    };
    let matches = run(&input);
    for (f, m) in matches.iter().take(3).enumerate() {
        assert_eq!(suggested(m), Some(key(on_disc[f])), "file {f}: {m:?}");
    }
    let special_key = EpisodeKey {
        season: 0,
        number: 1,
    };
    assert_eq!(
        suggested(&matches[3]),
        Some(special_key),
        "{:?}",
        matches[3]
    );
}

#[test]
fn a_title_missing_from_the_rip_shifts_the_order_by_its_chapter() {
    // Chapters 0-5 hold episodes 1-6; chapter 2's title (episode 3) was not ripped. The files at
    // chapters 3 and 4 were heard as almost nothing.
    let files = vec![
        file(
            "a.mkv",
            mishear(&episode_text(0), Errors::MODERATE, 90),
            RUNTIME_S,
        ),
        file(
            "b.mkv",
            mishear(&episode_text(1), Errors::MODERATE, 91),
            RUNTIME_S,
        ),
        file("c.mkv", FEW_WORDS.into(), RUNTIME_S),
        file("d.mkv", FEW_WORDS.into(), RUNTIME_S),
    ];
    let order = order_at(&files, &[(0, 0), (1, 1), (2, 3), (3, 4)]);
    let input = MatchInput {
        files,
        episodes: episodes_with_subtitles(),
        disc_order: Some(order),
    };
    let matches = run(&input);
    let got: Vec<_> = matches.iter().map(suggested).collect();
    assert_eq!(
        got,
        [Some(key(0)), Some(key(1)), Some(key(3)), Some(key(4))],
        "{matches:?}"
    );
}

#[test]
fn a_silent_bonus_feature_never_takes_an_episode_from_a_file_that_matches_it() {
    let mut files = misheard_files(Errors::MODERATE, 5);
    files.push(file("silent.mkv", String::new(), RUNTIME_S));
    files.push(file(
        "filler.mkv",
        "thank you thanks hey wow".into(),
        RUNTIME_S + 10.0,
    ));
    let input = MatchInput {
        files,
        episodes: episodes_with_subtitles(),
        disc_order: None,
    };
    let matches = run(&input);
    for (n, m) in matches[..10].iter().enumerate() {
        assert_eq!(suggested(m), Some(key(ORDER[n])), "file {n}: {m:?}");
    }
    for m in &matches[10..] {
        assert_eq!(m.suggestion, Suggestion::NotAnEpisode, "{m:?}");
        assert_ne!(m.confidence.verdict, Verdict::Confident);
    }
}
