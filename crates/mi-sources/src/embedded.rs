//! Text subtitle streams inside the files being identified.
//!
//! `mi-media` extracts a stream as SRT; this module turns it into text. Embedded subtitles belong
//! to a file, not to an episode, so they are matched like a perfect transcript of that file
//! rather than stored as an episode's reference text.

/// Plain dialogue from an embedded stream's SRT text (see [`crate::text::srt_to_dialogue`]).
pub fn dialogue_from_srt(srt: &str) -> String {
    crate::text::srt_to_dialogue(srt)
}
