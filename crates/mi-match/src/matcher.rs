//! The matching entry point.

use mi_types::{
    CancelFlag, Candidate, Episode, EpisodeKey, FileId, FileMatch, PlayAllPosition, ReferenceText,
};

use crate::DiscOrder;

/// One file to identify.
#[derive(Debug, Clone, PartialEq)]
pub struct FileInput {
    /// The file.
    pub file_id: FileId,
    /// Its duration, seconds.
    pub duration_s: f64,
    /// Unfiltered transcript text (`Transcript::matching_text`).
    pub transcript: String,
    /// Dialogue from an embedded text subtitle stream, when the file has one.
    pub embedded_text: Option<String>,
    /// True when most of the audio is music (few speech segments over the sampled time).
    pub mostly_music: bool,
    /// Where the file is in the play-all, when located.
    pub play_all_position: Option<PlayAllPosition>,
}

/// One candidate episode with its reference texts.
#[derive(Debug, Clone, PartialEq)]
pub struct EpisodeInput {
    /// The episode.
    pub episode: Episode,
    /// Its reference texts (may be empty).
    pub texts: Vec<ReferenceText>,
}

/// Everything matching needs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MatchInput {
    /// Files to identify (the play-all excluded).
    pub files: Vec<FileInput>,
    /// Candidate episodes, in episode order.
    pub episodes: Vec<EpisodeInput>,
    /// Disc order, when a play-all was found.
    pub disc_order: Option<DiscOrder>,
}

/// Relative weights of the signals in the combined score. Missing signals are left out and the
/// remaining weights renormalised.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalWeights {
    /// Dialogue similarity.
    pub dialogue: f32,
    /// Title hook.
    pub title_hook: f32,
    /// Duration fit.
    pub duration: f32,
    /// Disc order fit.
    pub disc_order: f32,
}

/// Matching thresholds and weights.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchConfig {
    /// Signal weights.
    pub weights: SignalWeights,
    /// Minimum lead over the runner-up for `Confident`.
    pub confident_margin: f32,
    /// Score of the "no episode" option; a file whose best episode scores below this becomes an
    /// extra.
    pub no_episode_score: f32,
    /// Penalty for leaving a file unassigned in the order-preserving assignment.
    pub skip_file_penalty: f32,
    /// Number of candidates kept per file for the Review dropdown.
    pub max_candidates: usize,
}

impl Default for MatchConfig {
    fn default() -> Self {
        Self {
            weights: SignalWeights {
                dialogue: 0.55,
                title_hook: 0.15,
                duration: 0.10,
                disc_order: 0.20,
            },
            confident_margin: 0.15,
            no_episode_score: 0.25,
            skip_file_penalty: 0.10,
            max_candidates: 5,
        }
    }
}

/// Scores for every file/episode pair.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScoreMatrix {
    /// Row labels.
    pub files: Vec<FileId>,
    /// Column labels.
    pub episodes: Vec<EpisodeKey>,
    /// `cells[file][episode]`, each with its signals and evidence.
    pub cells: Vec<Vec<Candidate>>,
}

/// Scores every file against every episode.
pub fn score_all(
    input: &MatchInput,
    config: &MatchConfig,
    cancel: &CancelFlag,
) -> crate::Result<ScoreMatrix> {
    let _ = (input, config, cancel);
    todo!("match module: score matrix")
}

/// Scores, assigns and classifies: one [`FileMatch`] per input file, in input order. Uses
/// order-preserving assignment when `input.disc_order` is trustworthy, otherwise Hungarian.
pub fn match_files(
    input: &MatchInput,
    config: &MatchConfig,
    cancel: &CancelFlag,
) -> crate::Result<Vec<FileMatch>> {
    let _ = (input, config, cancel);
    todo!("match module: assignment and confidence")
}
