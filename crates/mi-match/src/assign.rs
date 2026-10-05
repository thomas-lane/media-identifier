//! Choosing the best overall assignment of files to episodes.
//!
//! Scoring each file on its own would let two files claim the same episode. Both functions here
//! instead maximise the total score over all files at once, so a file whose best episode fits
//! another file better moves to its own next-best episode or becomes an extra.

use pathfinding::kuhn_munkres::kuhn_munkres;
use pathfinding::matrix::Matrix;

/// Scores are converted to integers for the Kuhn-Munkres solver with this many steps per 1.0.
const SCALE: f64 = 1_000_000.0;

/// Hungarian (Kuhn-Munkres) assignment maximising the total score, with one "no episode" column
/// per file scored `no_episode_score`, so a file is left unassigned rather than forced onto a
/// poor episode. `scores[file][episode]`; returns, per file, `Some(episode index)` or `None`.
///
/// Every row must have the same length. Non-finite scores count as impossible.
pub fn hungarian(scores: &[Vec<f32>], no_episode_score: f32) -> Vec<Option<usize>> {
    let files = scores.len();
    if files == 0 {
        return Vec::new();
    }
    let episodes = scores[0].len();
    // Columns: every episode, then one "no episode" column per file, so every file can always
    // be left unassigned and the matrix has at least as many columns as rows.
    let impossible = -(SCALE as i64) * 1000;
    let to_int = |s: f32| {
        if s.is_finite() {
            (f64::from(s) * SCALE).round() as i64
        } else {
            impossible
        }
    };
    let none = to_int(no_episode_score);
    let matrix = Matrix::from_fn(files, episodes + files, |(f, c)| {
        if c < episodes {
            to_int(scores[f][c])
        } else {
            none
        }
    });
    let (_, columns) = kuhn_munkres(&matrix);
    columns
        .into_iter()
        .enumerate()
        .map(|(f, c)| (c < episodes && to_int(scores[f][c]) > impossible).then_some(c))
        .collect()
}

/// Order-preserving assignment for files sorted by disc order and episodes sorted by episode
/// order: dynamic programming that maximises the total score while keeping both orders
/// monotonic, with `skip_file_penalty` for a file left as an extra and no cost for skipped
/// episodes (a disc rarely holds every episode). Returns, per file, `Some(episode index)` or
/// `None`.
///
/// A file is assigned only when its score beats `-skip_file_penalty`, so callers pass scores
/// relative to their "no episode" level. Ties prefer assigning over skipping, and earlier
/// episodes over later ones, so the result is deterministic.
pub fn order_preserving(scores: &[Vec<f32>], skip_file_penalty: f32) -> Vec<Option<usize>> {
    let files = scores.len();
    if files == 0 {
        return Vec::new();
    }
    let episodes = scores[0].len();
    let skip = -f64::from(skip_file_penalty);
    // best[i][j]: best total for files i.. and episodes j..
    let mut best = vec![vec![0.0f64; episodes + 1]; files + 1];
    for i in (0..files).rev() {
        best[i][episodes] = best[i + 1][episodes] + skip;
        for j in (0..episodes).rev() {
            let s = f64::from(scores[i][j]);
            let assign = if s.is_finite() {
                s + best[i + 1][j + 1]
            } else {
                f64::NEG_INFINITY
            };
            let skip_file = skip + best[i + 1][j];
            let skip_episode = best[i][j + 1];
            best[i][j] = assign.max(skip_file).max(skip_episode);
        }
    }
    let mut out = vec![None; files];
    let (mut i, mut j) = (0, 0);
    const EPS: f64 = 1e-9;
    while i < files {
        if j == episodes {
            i += 1;
            continue;
        }
        let s = f64::from(scores[i][j]);
        if s.is_finite() && (s + best[i + 1][j + 1] - best[i][j]).abs() < EPS {
            out[i] = Some(j);
            i += 1;
            j += 1;
        } else if (best[i][j + 1] - best[i][j]).abs() < EPS {
            j += 1;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hungarian_maximises_the_total() {
        // File 0 prefers episode 0 slightly; file 1 needs episode 0 much more.
        let scores = vec![vec![0.9, 0.8], vec![0.85, 0.1]];
        assert_eq!(hungarian(&scores, 0.25), vec![Some(1), Some(0)]);
    }

    #[test]
    fn hungarian_leaves_poor_files_unassigned() {
        let scores = vec![vec![0.9, 0.1], vec![0.1, 0.2]];
        assert_eq!(hungarian(&scores, 0.25), vec![Some(0), None]);
    }

    #[test]
    fn hungarian_handles_more_files_than_episodes() {
        let scores = vec![vec![0.9], vec![0.8], vec![0.7]];
        assert_eq!(hungarian(&scores, 0.25), vec![Some(0), None, None]);
        assert_eq!(hungarian(&[], 0.25), Vec::<Option<usize>>::new());
    }

    #[test]
    fn hungarian_handles_no_episodes() {
        let scores = vec![vec![], vec![]];
        assert_eq!(hungarian(&scores, 0.25), vec![None, None]);
    }

    #[test]
    fn hungarian_never_assigns_impossible_cells() {
        let scores = vec![vec![f32::NEG_INFINITY, 0.1]];
        assert_eq!(hungarian(&scores, -10.0), vec![Some(1)]);
        let scores = vec![vec![f32::NAN]];
        assert_eq!(hungarian(&scores, -10.0), vec![None]);
    }

    #[test]
    fn order_preserving_keeps_both_orders() {
        // Content alone would give file 0 episode 2 and file 1 episode 0; order forbids that.
        let scores = vec![
            vec![0.3, 0.2, 0.5],
            vec![0.6, 0.1, 0.4],
            vec![0.1, 0.2, 0.7],
        ];
        let out = order_preserving(&scores, 0.5);
        assert_eq!(out, vec![Some(0), Some(1), Some(2)]);
        // A cheap skip lets file 0 give up episode 0 to file 1: 0.6 + 0.7 - 0.1 beats 1.1.
        let out = order_preserving(&scores, 0.1);
        assert_eq!(out, vec![None, Some(0), Some(2)]);
    }

    #[test]
    fn order_preserving_skips_episodes_freely() {
        // A disc with episodes 2 and 4 of 5.
        let scores = vec![
            vec![-0.2, 0.6, -0.1, -0.2, -0.2],
            vec![-0.2, -0.1, -0.1, 0.5, -0.2],
        ];
        assert_eq!(order_preserving(&scores, 0.1), vec![Some(1), Some(3)]);
    }

    #[test]
    fn order_preserving_skips_extras_at_a_penalty() {
        let scores = vec![
            vec![0.5, -0.2, -0.2],
            vec![-0.4, -0.4, -0.4],
            vec![-0.2, 0.5, 0.4],
        ];
        assert_eq!(order_preserving(&scores, 0.1), vec![Some(0), None, Some(1)]);
        // With a large penalty, the weak file is placed rather than skipped.
        assert_eq!(
            order_preserving(&scores, 1.0),
            vec![Some(0), Some(1), Some(2)]
        );
    }

    #[test]
    fn order_preserving_empty_inputs() {
        assert!(order_preserving(&[], 0.1).is_empty());
        assert_eq!(order_preserving(&[vec![], vec![]], 0.1), vec![None, None]);
    }
}
