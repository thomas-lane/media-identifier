//! Confidence and verdicts.

use mi_types::{Confidence, FileId, FileMatch, Verdict};

use crate::MatchConfig;

/// The verdict for a file whose assigned option scored `best` and whose runner-up (another
/// episode or "no episode") scored `runner_up`.
///
/// - When the assignment chose "no episode", `best` is the "no episode" score and `runner_up`
///   the file's best episode. The file is an `Extra` when that episode scores below the
///   "no episode" level (nothing fits well). When it scores at or above it, the episode went to
///   another file that fits it better, so the file is `Check`: the user should decide.
/// - Otherwise the file is `Confident` when `best - runner_up >= config.confident_margin`, and
///   `Check` below that.
///
/// `score` is the suggestion's score and `margin` is `best - runner_up`.
pub fn classify(
    best: f32,
    runner_up: f32,
    chose_no_episode: bool,
    config: &MatchConfig,
) -> Confidence {
    let margin = best - runner_up;
    let verdict = if chose_no_episode {
        if runner_up < config.no_episode_score {
            Verdict::Extra
        } else {
            Verdict::Check
        }
    } else if margin >= config.confident_margin {
        Verdict::Confident
    } else {
        Verdict::Check
    };
    Confidence {
        score: best,
        margin,
        verdict,
    }
}

/// Files whose suggestion is uncertain enough that hearing more of them could change it: every
/// `Check` file, and every `Extra` whose best episode came within the confident margin of the
/// "no episode" level. The caller transcribes more windows of these files (or the whole file)
/// and matches again; a file that was already transcribed whole gains nothing from this.
pub fn needs_more_listening(matches: &[FileMatch], config: &MatchConfig) -> Vec<FileId> {
    matches
        .iter()
        .filter(|m| match m.confidence.verdict {
            Verdict::Check => true,
            Verdict::Extra => m.confidence.margin < config.confident_margin,
            Verdict::Confident | Verdict::PlayAll => false,
        })
        .map(|m| m.file_id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_types::Suggestion;

    #[test]
    fn clear_lead_is_confident() {
        let c = classify(0.8, 0.3, false, &MatchConfig::default());
        assert_eq!(c.verdict, Verdict::Confident);
        assert!((c.margin - 0.5).abs() < 1e-6);
        assert_eq!(c.score, 0.8);
    }

    #[test]
    fn small_lead_needs_checking() {
        let c = classify(0.5, 0.45, false, &MatchConfig::default());
        assert_eq!(c.verdict, Verdict::Check);
    }

    #[test]
    fn no_episode_with_nothing_close_is_extra() {
        let config = MatchConfig::default();
        let c = classify(config.no_episode_score, 0.1, true, &config);
        assert_eq!(c.verdict, Verdict::Extra);
        assert!(c.margin > 0.0);
    }

    #[test]
    fn no_episode_because_another_file_won_is_check() {
        let config = MatchConfig::default();
        let c = classify(config.no_episode_score, 0.7, true, &config);
        assert_eq!(c.verdict, Verdict::Check);
        assert!(c.margin < 0.0);
    }

    fn file_match(id: &str, verdict: Verdict, margin: f32) -> FileMatch {
        FileMatch {
            file_id: FileId(id.into()),
            suggestion: Suggestion::NotAnEpisode,
            confidence: Confidence {
                score: 0.5,
                margin,
                verdict,
            },
            candidates: vec![],
        }
    }

    #[test]
    fn uncertain_files_get_more_listening() {
        let config = MatchConfig::default();
        let matches = vec![
            file_match("confident", Verdict::Confident, 0.4),
            file_match("check", Verdict::Check, 0.05),
            file_match("close-extra", Verdict::Extra, 0.02),
            file_match("clear-extra", Verdict::Extra, 0.2),
            file_match("play-all", Verdict::PlayAll, 0.0),
        ];
        let ids: Vec<String> = needs_more_listening(&matches, &config)
            .into_iter()
            .map(|f| f.0)
            .collect();
        assert_eq!(ids, ["check", "close-extra"]);
    }
}
