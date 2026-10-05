//! Choosing the best overall assignment of files to episodes.

/// Hungarian (Kuhn-Munkres) assignment maximising the total score, with one "no episode" column
/// per file scored `no_episode_score`, so a file is left unassigned rather than forced onto a
/// poor episode. `scores[file][episode]`; returns, per file, `Some(episode index)` or `None`.
pub fn hungarian(scores: &[Vec<f32>], no_episode_score: f32) -> Vec<Option<usize>> {
    let _ = (scores, no_episode_score);
    todo!("match module: Hungarian assignment")
}

/// Order-preserving assignment for files sorted by disc order and episodes sorted by episode
/// order: dynamic programming that maximises the total score while keeping both orders
/// monotonic, with `skip_file_penalty` for a file left as an extra and no cost for skipped
/// episodes (a disc rarely holds every episode). Returns, per file, `Some(episode index)` or
/// `None`.
pub fn order_preserving(scores: &[Vec<f32>], skip_file_penalty: f32) -> Vec<Option<usize>> {
    let _ = (scores, skip_file_penalty);
    todo!("match module: order-preserving DP")
}
