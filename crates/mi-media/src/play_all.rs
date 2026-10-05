//! Detecting the disc's "play all" title.
//!
//! A DVD or Blu-ray of short episodes usually has one long title that plays every episode back to
//! back. MakeMKV rips it as an ordinary file next to the episodes. It is recognised here from
//! durations and chapters alone; whether its order can be trusted is decided later by audio
//! alignment in `mi-match`.

use mi_types::{FileRole, MediaFile, PlayAllInfo};

/// Tolerances for recognising a play-all title.
#[derive(Debug, Clone)]
pub struct PlayAllThresholds {
    /// The play-all's duration may differ from the sum of the other files by at most this
    /// fraction of that sum (rips trim a few seconds per title, and some titles are missing).
    pub max_relative_difference: f64,
    /// The play-all must be at least this many times longer than the longest other file. This
    /// keeps a double-length episode among normal ones from being taken for a play-all.
    pub min_ratio_to_longest: f64,
    /// With chapter evidence as well as duration evidence, a lower ratio is enough: a disc of two
    /// or three episodes has a play-all only two or three times longer than each.
    pub min_ratio_with_chapters: f64,
    /// A candidate file "matches" a chapter when their lengths differ by at most this many
    /// seconds or [`Self::chapter_relative_tolerance`] of the file's length, whichever is larger.
    pub chapter_tolerance_s: f64,
    /// Relative part of the chapter tolerance.
    pub chapter_relative_tolerance: f64,
    /// Chapter evidence counts when at least this fraction of the other files match a chapter.
    pub min_chapter_fraction: f64,
    /// ...and at least this many files match.
    pub min_chapters_matched: u32,
}

impl Default for PlayAllThresholds {
    fn default() -> Self {
        Self {
            max_relative_difference: 0.25,
            min_ratio_to_longest: 3.0,
            min_ratio_with_chapters: 1.5,
            chapter_tolerance_s: 2.0,
            chapter_relative_tolerance: 0.01,
            min_chapter_fraction: 0.5,
            min_chapters_matched: 2,
        }
    }
}

/// Picks the play-all title among the [`FileRole::Candidate`] files that have a probe, if one
/// exists. Other roles are ignored, so unreadable files and menus do not count towards the sum.
///
/// The longest file is compared with all the others:
/// - **duration fit**: its duration is within `max_relative_difference` of their sum;
/// - **chapter fit**: at least `min_chapter_fraction` of the others (and at least
///   `min_chapters_matched`) have the length of one of its chapters, each chapter used once;
/// - **ratio**: its duration divided by the next longest file's.
///
/// It is the play-all when the ratio is at least `min_ratio_to_longest` and either fit holds, or
/// when the ratio is at least `min_ratio_with_chapters` and both fits hold. A play-all whose
/// chapters fit but whose duration does not is accepted because MakeMKV drops titles shorter than
/// its minimum length, which can leave the play-all much longer than the files found.
///
/// `confidence` is `0.6 * max(d, c) + 0.4 * min(d, c)` where `d = 1 - difference / allowed
/// difference` (clamped to 0..1) and `c` is the fraction of files matching a chapter; a play-all
/// without chapters gets `0.7 * d`, because duration alone is weaker evidence.
///
/// Returns `None` for fewer than three such files (a play-all needs at least two titles to play).
pub fn detect_play_all(files: &[MediaFile], thresholds: &PlayAllThresholds) -> Option<PlayAllInfo> {
    let mut probed: Vec<(&MediaFile, f64)> = files
        .iter()
        .filter(|f| f.role == FileRole::Candidate)
        .filter_map(|f| f.probe.as_ref().map(|p| (f, p.duration_s)))
        .collect();
    if probed.len() < 3 {
        return None;
    }
    probed.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.id.cmp(&b.0.id)));
    let (play_all, duration_s) = probed[0];
    let others: Vec<f64> = probed[1..].iter().map(|(_, d)| *d).collect();
    let total: f64 = others.iter().sum();
    let longest_other = others[0];
    if total <= 0.0 || longest_other <= 0.0 {
        return None;
    }

    let ratio = duration_s / longest_other;
    let relative_difference = (duration_s - total).abs() / total;
    let duration_fit = relative_difference <= thresholds.max_relative_difference;

    let chapters = &play_all.probe.as_ref()?.chapters;
    let chapter_lengths: Vec<f64> = chapters
        .iter()
        .map(|c| c.end_s - c.start_s)
        .filter(|d| *d > 0.0)
        .collect();
    let chapters_matched = count_chapter_matches(&chapter_lengths, &others, thresholds);
    let chapter_fraction = chapters_matched as f64 / others.len() as f64;
    let has_chapters = chapter_lengths.len() >= 2;
    let chapter_fit = has_chapters
        && chapter_fraction >= thresholds.min_chapter_fraction
        && chapters_matched >= thresholds.min_chapters_matched;

    let accepted = (ratio >= thresholds.min_ratio_to_longest && (duration_fit || chapter_fit))
        || (ratio >= thresholds.min_ratio_with_chapters && duration_fit && chapter_fit);
    if !accepted {
        return None;
    }

    let d = (1.0 - relative_difference / thresholds.max_relative_difference).clamp(0.0, 1.0);
    let confidence = if has_chapters {
        let c = chapter_fraction.clamp(0.0, 1.0);
        0.6 * d.max(c) + 0.4 * d.min(c)
    } else {
        0.7 * d
    };

    let reason = describe(
        &play_all.file_name,
        duration_s,
        total,
        others.len(),
        relative_difference,
        has_chapters.then_some((chapters_matched, chapter_lengths.len())),
    );
    Some(PlayAllInfo {
        file_id: play_all.id.clone(),
        duration_s,
        chapter_count: chapters.len() as u32,
        candidates_total_s: total,
        chapters_matched,
        confidence: confidence as f32,
        reason,
    })
}

/// Counts files whose length matches a distinct chapter. Pairs are taken closest first, so a
/// chapter is given to the file it fits best.
fn count_chapter_matches(chapters: &[f64], files: &[f64], t: &PlayAllThresholds) -> u32 {
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for (fi, &file) in files.iter().enumerate() {
        let tolerance = t
            .chapter_tolerance_s
            .max(t.chapter_relative_tolerance * file);
        for (ci, &chapter) in chapters.iter().enumerate() {
            let diff = (chapter - file).abs();
            if diff <= tolerance {
                pairs.push((diff, fi, ci));
            }
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut file_used = vec![false; files.len()];
    let mut chapter_used = vec![false; chapters.len()];
    let mut matched = 0;
    for (_, fi, ci) in pairs {
        if !file_used[fi] && !chapter_used[ci] {
            file_used[fi] = true;
            chapter_used[ci] = true;
            matched += 1;
        }
    }
    matched
}

fn describe(
    name: &str,
    duration_s: f64,
    total_s: f64,
    others: usize,
    relative_difference: f64,
    chapters: Option<(u32, usize)>,
) -> String {
    let percent = (relative_difference * 100.0).round();
    let comparison = if percent < 1.0 {
        "about the same as".to_owned()
    } else if duration_s >= total_s {
        format!("{percent}% longer than")
    } else {
        format!("{percent}% shorter than")
    };
    let mut text = format!(
        "{name} is {} long, {comparison} the other {others} files together ({}).",
        clock(duration_s),
        clock(total_s)
    );
    if let Some((matched, count)) = chapters {
        text.push_str(&format!(
            " {matched} of the {others} files have the length of one of its {count} chapters."
        ));
    }
    text
}

/// Formats seconds as `h:mm:ss` or `m:ss`.
fn clock(seconds: f64) -> String {
    let s = seconds.round() as u64;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use mi_types::{Chapter, FileId, Probe};

    /// A probed candidate file with the given duration and chapter lengths.
    pub(crate) fn file(name: &str, duration_s: f64, chapter_lengths: &[f64]) -> MediaFile {
        let mut start = 0.0;
        let chapters = chapter_lengths
            .iter()
            .enumerate()
            .map(|(i, len)| {
                let c = Chapter {
                    index: i as u32,
                    start_s: start,
                    end_s: start + len,
                    title: None,
                };
                start += len;
                c
            })
            .collect();
        MediaFile {
            id: FileId(name.to_owned()),
            path: format!("/disc/{name}").into(),
            file_name: name.to_owned(),
            size_bytes: 1,
            probe: Some(Probe {
                duration_s,
                container: "matroska,webm".into(),
                video: None,
                audio_streams: Vec::new(),
                subtitle_streams: Vec::new(),
                chapters,
            }),
            role: FileRole::Candidate,
        }
    }

    fn detect(files: &[MediaFile]) -> Option<PlayAllInfo> {
        detect_play_all(files, &PlayAllThresholds::default())
    }

    #[test]
    fn finds_play_all_whose_chapters_are_the_titles() {
        let shorts = [192.0, 185.0, 185.0, 178.0, 201.0];
        let mut files: Vec<MediaFile> = shorts
            .iter()
            .enumerate()
            .map(|(i, d)| file(&format!("title_t{:02}.mkv", i + 1), *d, &[]))
            .collect();
        files.push(file("title_t00.mkv", 941.5, &shorts));
        let info = detect(&files).expect("play-all");
        assert_eq!(info.file_id, FileId("title_t00.mkv".into()));
        assert_eq!(info.chapter_count, 5);
        assert_eq!(info.chapters_matched, 5);
        assert_eq!(info.candidates_total_s, 941.0);
        assert!(info.confidence > 0.95, "{}", info.confidence);
        assert!(
            info.reason.contains("5 of the 5 files"),
            "reason: {}",
            info.reason
        );
    }

    #[test]
    fn duration_alone_is_enough_with_a_large_ratio_but_scores_lower() {
        let mut files = vec![
            file("a.mkv", 600.0, &[]),
            file("b.mkv", 610.0, &[]),
            file("c.mkv", 590.0, &[]),
            file("d.mkv", 600.0, &[]),
        ];
        files.push(file("all.mkv", 2390.0, &[]));
        let info = detect(&files).expect("play-all");
        assert_eq!(info.file_id, FileId("all.mkv".into()));
        assert_eq!(info.chapters_matched, 0);
        assert!(
            info.confidence <= 0.7 && info.confidence > 0.6,
            "{}",
            info.confidence
        );
    }

    #[test]
    fn chapters_rescue_a_play_all_when_short_titles_were_not_ripped() {
        // Ten 3-minute titles on the disc; MakeMKV only kept six of them.
        let all: Vec<f64> = (0..10).map(|i| 170.0 + i as f64 * 3.0).collect();
        let mut files: Vec<MediaFile> = all[..6]
            .iter()
            .enumerate()
            .map(|(i, d)| file(&format!("t{i}.mkv"), *d, &[]))
            .collect();
        files.push(file("all.mkv", all.iter().sum(), &all));
        let info = detect(&files).expect("play-all");
        assert!(
            (info.duration_s - info.candidates_total_s) / info.candidates_total_s > 0.25,
            "duration alone would not fit"
        );
        assert_eq!(info.chapters_matched, 6);
        assert_eq!(info.chapter_count, 10);
    }

    #[test]
    fn a_double_episode_is_not_a_play_all() {
        let files = vec![
            file("e1.mkv", 1320.0, &[300.0, 400.0, 620.0]),
            file("e2.mkv", 1330.0, &[]),
            file("double.mkv", 2640.0, &[600.0, 700.0, 640.0, 700.0]),
        ];
        assert_eq!(detect(&files), None);
    }

    #[test]
    fn a_small_disc_needs_both_duration_and_chapters() {
        let with_chapters = vec![
            file("e1.mkv", 1320.0, &[]),
            file("e2.mkv", 1330.0, &[]),
            file("all.mkv", 2650.0, &[1320.0, 1330.0]),
        ];
        let info = detect(&with_chapters).expect("two-episode play-all");
        assert_eq!(info.chapters_matched, 2);

        let without = vec![
            file("e1.mkv", 1320.0, &[]),
            file("e2.mkv", 1330.0, &[]),
            file("all.mkv", 2650.0, &[]),
        ];
        assert_eq!(detect(&without), None);
    }

    #[test]
    fn a_movie_with_featurettes_is_not_a_play_all() {
        let files = vec![
            file("movie.mkv", 5400.0, &[600.0; 9]),
            file("bonus1.mkv", 300.0, &[]),
            file("bonus2.mkv", 420.0, &[]),
        ];
        assert_eq!(detect(&files), None);
    }

    #[test]
    fn ignores_files_that_are_not_candidates_and_needs_three() {
        let mut ignored = file("menu.mkv", 30.0, &[]);
        ignored.role = FileRole::Ignored;
        let files = vec![
            file("a.mkv", 600.0, &[]),
            file("all.mkv", 630.0, &[]),
            ignored,
        ];
        assert_eq!(detect(&files), None);
    }

    #[test]
    fn each_chapter_matches_one_file() {
        let t = PlayAllThresholds::default();
        // Three files of the same length but only one chapter of that length.
        assert_eq!(
            count_chapter_matches(&[180.0, 400.0], &[180.0, 181.0, 179.5], &t),
            1
        );
        // Tolerance is 2 s or 1%, whichever is larger.
        assert_eq!(count_chapter_matches(&[1312.0], &[1300.0], &t), 1);
        assert_eq!(count_chapter_matches(&[183.0], &[180.0], &t), 0);
    }

    #[test]
    fn clock_formats() {
        assert_eq!(clock(59.6), "1:00");
        assert_eq!(clock(8091.0), "2:14:51");
    }
}
