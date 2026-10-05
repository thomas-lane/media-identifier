//! Finding the episode title in what was heard.

/// How well `title` occurs in `transcript`, `0.0..=1.0`.
///
/// Songs usually sing their own title ("Conjunction Junction"), so a fuzzy match of the title
/// inside the transcript is strong evidence. Titles of one or two very common words ("Pilot",
/// "The End") are weighted down, because they occur in many transcripts by chance.
pub fn title_hook_score(title: &str, transcript: &str) -> f32 {
    let _ = (title, transcript);
    todo!("match module: title hook")
}
