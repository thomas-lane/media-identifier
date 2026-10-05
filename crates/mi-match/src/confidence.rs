//! Confidence and verdicts.

use mi_types::Confidence;

use crate::MatchConfig;

/// The verdict for a file whose assigned option scored `best` and whose runner-up (another
/// episode or "no episode") scored `runner_up`: `Extra` when the assignment chose "no episode",
/// `Confident` when `best - runner_up >= config.confident_margin`, otherwise `Check`.
pub fn classify(
    best: f32,
    runner_up: f32,
    chose_no_episode: bool,
    config: &MatchConfig,
) -> Confidence {
    let _ = (best, runner_up, chose_no_episode, config);
    todo!("match module: confidence")
}
