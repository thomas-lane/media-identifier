//! Turning subtitle files into plain dialogue.

/// Converts SubRip text to dialogue lines: removes cue numbers, timing lines, HTML/ASS tags,
/// speaker labels (`JOHN:`), sound descriptions in brackets or parentheses (`[music]`,
/// `(laughs)`), music-note symbols and duplicate consecutive lines; joins multi-line cues with a
/// space and returns one cue per line. Accepts CRLF and a UTF-8 BOM.
pub fn srt_to_dialogue(srt: &str) -> String {
    let _ = srt;
    todo!("sources module: SRT normalisation")
}

/// Same as [`srt_to_dialogue`] for WebVTT and ASS/SSA, chosen by content sniffing.
pub fn subtitle_to_dialogue(content: &str) -> String {
    let _ = content;
    todo!("sources module: subtitle normalisation")
}
