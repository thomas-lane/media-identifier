//! Choosing which parts of a file to transcribe, and when to use voice activity detection.

use mi_types::SampleWindow;

/// Gaps shorter than this (seconds) are not worth a separate transcription when the rest of a
/// file is transcribed: whisper.cpp needs about a second of audio to recognise a word.
pub const MIN_GAP_S: f64 = 1.0;

/// Sampling rules.
#[derive(Debug, Clone, PartialEq)]
pub struct SamplingPolicy {
    /// Files up to this length (seconds) are transcribed whole and without voice activity
    /// detection. Short musical clips need every second, and whole-file decoding of a few minutes
    /// is fast.
    pub whole_file_max_s: f64,
    /// Length of each window for longer files, seconds (90-120).
    pub window_s: f64,
    /// Window centres as fractions of the runtime. Avoids the first and last minutes, where
    /// opening and closing credits are shared by every episode.
    pub positions: Vec<f64>,
    /// Escalation does not place a window in a gap shorter than this (seconds); such a window
    /// would hold too little dialogue to change a match.
    pub min_window_s: f64,
}

impl Default for SamplingPolicy {
    fn default() -> Self {
        Self {
            whole_file_max_s: 360.0,
            window_s: 105.0,
            positions: vec![0.15, 0.40, 0.65, 0.85],
            min_window_s: 20.0,
        }
    }
}

fn window(start_s: f64, end_s: f64) -> SampleWindow {
    SampleWindow { start_s, end_s }
}

fn valid_duration(duration_s: f64) -> bool {
    duration_s.is_finite() && duration_s > 0.0
}

/// Sorts windows and merges those that overlap or touch.
pub fn merge_windows(windows: &[SampleWindow]) -> Vec<SampleWindow> {
    let mut sorted: Vec<SampleWindow> = windows
        .iter()
        .copied()
        .filter(|w| w.start_s.is_finite() && w.end_s.is_finite() && w.end_s > w.start_s)
        .collect();
    sorted.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
    let mut merged: Vec<SampleWindow> = Vec::with_capacity(sorted.len());
    for w in sorted {
        match merged.last_mut() {
            Some(last) if w.start_s <= last.end_s => last.end_s = last.end_s.max(w.end_s),
            _ => merged.push(w),
        }
    }
    merged
}

/// Total seconds covered by `windows`, counting overlaps once.
pub fn covered_seconds(windows: &[SampleWindow]) -> f64 {
    merge_windows(windows)
        .iter()
        .map(|w| w.end_s - w.start_s)
        .sum()
}

/// Parts of `[0, duration_s]` not covered by `done`, in time order.
fn gaps(duration_s: f64, done: &[SampleWindow]) -> Vec<SampleWindow> {
    let clamped: Vec<SampleWindow> = done
        .iter()
        .map(|w| window(w.start_s.max(0.0), w.end_s.min(duration_s)))
        .collect();
    let mut result = Vec::new();
    let mut cursor = 0.0;
    for w in merge_windows(&clamped) {
        if w.start_s > cursor {
            result.push(window(cursor, w.start_s));
        }
        cursor = cursor.max(w.end_s);
    }
    if cursor < duration_s {
        result.push(window(cursor, duration_s));
    }
    result
}

/// Initial windows for a file of `duration_s` seconds.
///
/// One window covering the whole file when `duration_s <= whole_file_max_s` or when `sample` is
/// false (the user turned sampling off). Otherwise one `window_s` window centred on each
/// position; a window that would cross the start or end of the file is shifted inside it, and
/// windows that overlap are merged. Returns nothing for a zero, negative or non-finite duration.
pub fn plan_windows(duration_s: f64, policy: &SamplingPolicy, sample: bool) -> Vec<SampleWindow> {
    if !valid_duration(duration_s) {
        return Vec::new();
    }
    if !sample || duration_s <= policy.whole_file_max_s || policy.window_s >= duration_s {
        return vec![window(0.0, duration_s)];
    }
    let half = policy.window_s / 2.0;
    let windows: Vec<SampleWindow> = policy
        .positions
        .iter()
        .filter(|p| p.is_finite())
        .map(|p| {
            let centre = p.clamp(0.0, 1.0) * duration_s;
            let start = (centre - half).clamp(0.0, duration_s - policy.window_s);
            window(start, start + policy.window_s)
        })
        .collect();
    merge_windows(&windows)
}

/// Extra windows to transcribe when the match margin after `done` windows is too low. Never
/// returns ranges already covered by `done`.
///
/// - `whole_file` true: every uncovered part of the file of at least [`MIN_GAP_S`].
/// - Otherwise: one window of up to `window_s` centred in each uncovered gap between two
///   transcribed windows that is at least `min_window_s` long, so repeated escalation fills the
///   middle of the file progressively. When no such gap remains, the gaps before the first and
///   after the last window are used the same way; they come last because they hold the opening
///   and closing credits. Empty when nothing worth transcribing is left.
///
/// When `done` is empty this is [`plan_windows`] with sampling on (or the whole file).
pub fn escalation_windows(
    duration_s: f64,
    done: &[SampleWindow],
    policy: &SamplingPolicy,
    whole_file: bool,
) -> Vec<SampleWindow> {
    if !valid_duration(duration_s) {
        return Vec::new();
    }
    let open = gaps(duration_s, done);
    if whole_file {
        return open
            .into_iter()
            .filter(|g| g.end_s - g.start_s >= MIN_GAP_S)
            .collect();
    }
    if done.is_empty() {
        return plan_windows(duration_s, policy, true);
    }
    let centred = |g: &SampleWindow| {
        let len = (g.end_s - g.start_s).min(policy.window_s);
        let mid = (g.start_s + g.end_s) / 2.0;
        window(mid - len / 2.0, mid + len / 2.0)
    };
    let big_enough = |g: &&SampleWindow| g.end_s - g.start_s >= policy.min_window_s;
    let is_edge = |g: &SampleWindow| g.start_s <= 0.0 || g.end_s >= duration_s;

    let interior: Vec<SampleWindow> = open
        .iter()
        .filter(|g| !is_edge(g))
        .filter(big_enough)
        .map(centred)
        .collect();
    if !interior.is_empty() {
        return interior;
    }
    open.iter()
        .filter(|g| is_edge(g))
        .filter(big_enough)
        .map(centred)
        .collect()
}

/// Whether to use voice activity detection for a file.
///
/// Off for files of `whole_file_max_s` or less and for music-heavy content (the caller decides,
/// for example when the reference text is song lyrics), because the detector cuts sung words and
/// short clips are mostly speech or song anyway. On for longer files, where it skips music beds
/// and silence that would otherwise produce invented text.
pub fn use_vad(duration_s: f64, music_heavy: bool, policy: &SamplingPolicy) -> bool {
    !music_heavy && valid_duration(duration_s) && duration_s > policy.whole_file_max_s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlaps(a: &SampleWindow, b: &SampleWindow) -> bool {
        a.start_s < b.end_s && b.start_s < a.end_s
    }

    fn assert_inside(windows: &[SampleWindow], duration: f64) {
        for w in windows {
            assert!(w.start_s >= 0.0 && w.end_s <= duration + 1e-9, "{w:?}");
            assert!(w.end_s > w.start_s, "{w:?}");
        }
    }

    #[test]
    fn short_files_are_transcribed_whole() {
        let p = SamplingPolicy::default();
        assert_eq!(plan_windows(150.0, &p, true), vec![window(0.0, 150.0)]);
        assert_eq!(plan_windows(360.0, &p, true), vec![window(0.0, 360.0)]);
    }

    #[test]
    fn sampling_off_transcribes_long_files_whole() {
        let p = SamplingPolicy::default();
        assert_eq!(plan_windows(1320.0, &p, false), vec![window(0.0, 1320.0)]);
    }

    #[test]
    fn long_files_get_four_windows_at_the_configured_positions() {
        let p = SamplingPolicy::default();
        let d = 1320.0;
        let w = plan_windows(d, &p, true);
        assert_eq!(w.len(), 4);
        for (win, pos) in w.iter().zip(&p.positions) {
            assert!((win.end_s - win.start_s - 105.0).abs() < 1e-9);
            assert!(((win.start_s + win.end_s) / 2.0 - pos * d).abs() < 1e-9);
        }
        assert_inside(&w, d);
    }

    #[test]
    fn windows_near_the_edges_are_shifted_inside_and_overlaps_merged() {
        let p = SamplingPolicy {
            positions: vec![0.0, 0.02, 1.0],
            ..SamplingPolicy::default()
        };
        let d = 1000.0;
        let w = plan_windows(d, &p, true);
        // 0.0 and 0.02 (centre 20 s) both start at 0 and merge; 1.0 ends at the end.
        assert_eq!(w, vec![window(0.0, 105.0), window(895.0, 1000.0)]);
    }

    #[test]
    fn just_over_the_whole_file_limit_windows_merge_and_stay_inside() {
        let p = SamplingPolicy::default();
        let d = 361.0;
        let w = plan_windows(d, &p, true);
        assert_inside(&w, d);
        for pair in w.windows(2) {
            assert!(pair[0].end_s < pair[1].start_s);
        }
    }

    #[test]
    fn invalid_durations_give_no_windows() {
        let p = SamplingPolicy::default();
        for d in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            assert!(plan_windows(d, &p, true).is_empty());
            assert!(escalation_windows(d, &[], &p, true).is_empty());
        }
    }

    #[test]
    fn escalation_fills_gaps_between_windows_without_overlap() {
        let p = SamplingPolicy::default();
        let d = 1320.0;
        let done = plan_windows(d, &p, true);
        let extra = escalation_windows(d, &done, &p, false);
        assert_eq!(extra.len(), 3, "one window between each pair: {extra:?}");
        for e in &extra {
            assert!(done.iter().all(|w| !overlaps(w, e)), "{e:?} overlaps");
            assert!(e.end_s - e.start_s <= p.window_s + 1e-9);
        }
        // The first extra window sits midway between the first two initial windows.
        let mid = (done[0].end_s + done[1].start_s) / 2.0;
        assert!(((extra[0].start_s + extra[0].end_s) / 2.0 - mid).abs() < 1e-9);
        assert_inside(&extra, d);
    }

    #[test]
    fn repeated_escalation_reaches_the_edges_then_stops() {
        let p = SamplingPolicy::default();
        let d = 1320.0;
        let mut done = plan_windows(d, &p, true);
        let mut rounds = 0;
        let mut used_edges = false;
        loop {
            let extra = escalation_windows(d, &done, &p, false);
            if extra.is_empty() {
                break;
            }
            for e in &extra {
                assert!(done.iter().all(|w| !overlaps(w, e)));
                used_edges |= e.start_s <= done[0].start_s || e.end_s >= d - 1e-9;
            }
            done = merge_windows(&[done, extra].concat());
            rounds += 1;
            assert!(rounds < 50, "escalation does not converge");
        }
        assert!(used_edges, "edge gaps are used once the middle is full");
        for g in gaps(d, &done) {
            assert!(g.end_s - g.start_s < p.min_window_s);
        }
    }

    #[test]
    fn whole_file_escalation_returns_exactly_the_uncovered_parts() {
        let p = SamplingPolicy::default();
        let d = 1320.0;
        let done = plan_windows(d, &p, true);
        let rest = escalation_windows(d, &done, &p, true);
        assert_eq!(rest.len(), 5);
        assert_eq!(rest[0].start_s, 0.0);
        assert_eq!(rest[4].end_s, d);
        for r in &rest {
            assert!(done.iter().all(|w| !overlaps(w, r)));
        }
        let all = [done, rest].concat();
        assert!((covered_seconds(&all) - d).abs() < 1e-6);
        assert_eq!(merge_windows(&all), vec![window(0.0, d)]);
    }

    #[test]
    fn whole_file_escalation_skips_tiny_gaps_and_finished_files() {
        let p = SamplingPolicy::default();
        let done = [window(0.0, 100.0), window(100.5, 200.0)];
        assert!(escalation_windows(200.0, &done, &p, true).is_empty());
        let whole = plan_windows(200.0, &p, true);
        assert!(escalation_windows(200.0, &whole, &p, true).is_empty());
        assert!(escalation_windows(200.0, &whole, &p, false).is_empty());
    }

    #[test]
    fn escalation_without_previous_windows_is_the_initial_plan() {
        let p = SamplingPolicy::default();
        assert_eq!(
            escalation_windows(1320.0, &[], &p, false),
            plan_windows(1320.0, &p, true)
        );
    }

    #[test]
    fn covered_seconds_counts_overlaps_once() {
        let w = [window(0.0, 10.0), window(5.0, 15.0), window(20.0, 30.0)];
        assert!((covered_seconds(&w) - 25.0).abs() < 1e-9);
    }

    #[test]
    fn vad_is_used_only_for_long_files_that_are_not_music_heavy() {
        let p = SamplingPolicy::default();
        assert!(!use_vad(120.0, false, &p));
        assert!(!use_vad(360.0, false, &p));
        assert!(use_vad(1320.0, false, &p));
        assert!(!use_vad(1320.0, true, &p));
        assert!(!use_vad(f64::NAN, false, &p));
    }
}
