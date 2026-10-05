//! Marking text the model invents.

use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use mi_types::{FilterReason, Segment};

/// Marks segments that should not be used for matching (sets [`Segment::filtered`]).
///
/// Whisper models invent fixed phrases over music and silence ("Thank you.", "Subtitles by ...",
/// "Please subscribe") and sometimes loop the same line. Such text would match every episode
/// equally and hide the real signal, so it is excluded from matching and kept only for display.
///
/// Text is compared after normalisation: lowercase, apostrophes removed, every other character
/// that is not a letter or digit replaced by a space, and runs of spaces collapsed.
#[derive(Debug, Clone)]
pub struct HallucinationFilter {
    /// Phrases (normalised) that mark a segment when the segment consists only of them, once or
    /// repeated ("Thank you. Thank you.").
    pub phrases: Vec<String>,
    /// Prefixes (normalised) that mark a segment when it starts with them: credit lines such as
    /// "Subtitles by the Amara.org community", whose continuation varies.
    pub prefixes: Vec<String>,
    /// Segments whose zlib compression ratio exceeds this are marked as looping output; 2.4 is
    /// the threshold OpenAI's Whisper uses for the same check.
    pub max_compression_ratio: f32,
    /// A segment whose `no_speech_prob` is above this ...
    pub no_speech_threshold: f32,
    /// ... and whose `avg_logprob` is below this is marked as not speech. Both conditions are
    /// needed: a confident decode with a high no-speech probability is usually real speech over
    /// music.
    pub logprob_threshold: f32,
}

impl Default for HallucinationFilter {
    fn default() -> Self {
        Self {
            phrases: [
                "thank you",
                "thank you very much",
                "thanks for watching",
                "thank you for watching",
                "thank you so much for watching",
                "please subscribe",
                "like and subscribe",
                "subscribe to my channel",
                "you",
                "bye",
            ]
            .map(String::from)
            .to_vec(),
            prefixes: [
                "subtitles by",
                "subtitled by",
                "captions by",
                "captioning by",
                "closed captioning by",
                "captioned by",
                "transcribed by",
                "transcription by",
                "translated by",
                "translation by",
                "sync and corrections by",
                "subtitles made by",
                "amara org",
            ]
            .map(String::from)
            .to_vec(),
            max_compression_ratio: 2.4,
            no_speech_threshold: 0.6,
            logprob_threshold: -1.0,
        }
    }
}

/// Normalises text for comparison (see [`HallucinationFilter`]).
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for c in text.chars() {
        if c == '\'' || c == '\u{2019}' {
            continue;
        }
        if c.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.extend(c.to_lowercase());
        } else {
            pending_space = true;
        }
    }
    out
}

/// True when `text` contains no words outside brackets, parentheses, asterisks or music symbols:
/// sound descriptions such as "[MUSIC PLAYING]", "(laughs)" or "♪♪".
fn is_sound_description(text: &str) -> bool {
    let mut depth = 0i32;
    let mut in_asterisks = false;
    let mut has_any = false;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            '*' => in_asterisks = !in_asterisks,
            '♪' | '♫' | '♬' | '♩' => has_any = true,
            c if c.is_alphanumeric() => {
                has_any = true;
                if depth == 0 && !in_asterisks {
                    return false;
                }
            }
            _ => {}
        }
    }
    has_any
}

/// True when `norm` is `phrase` repeated one or more times.
fn is_repeated_phrase(norm: &str, phrase: &str) -> bool {
    if phrase.is_empty() {
        return false;
    }
    let mut rest = norm;
    loop {
        match rest.strip_prefix(phrase) {
            Some("") => return true,
            Some(tail) => match tail.strip_prefix(' ') {
                Some(t) => rest = t,
                None => return false,
            },
            None => return false,
        }
    }
}

impl HallucinationFilter {
    /// The reason a segment would be marked, judged on its own (without its neighbours).
    fn own_reason(&self, segment: &Segment, norm: &str) -> Option<FilterReason> {
        let starts_with_prefix = |p: &String| {
            norm.strip_prefix(p.as_str())
                .is_some_and(|tail| tail.is_empty() || tail.starts_with(' '))
        };
        if self.phrases.iter().any(|p| is_repeated_phrase(norm, p))
            || self.prefixes.iter().any(starts_with_prefix)
        {
            return Some(FilterReason::KnownHallucination);
        }
        if compression_ratio(segment.text.trim()) > self.max_compression_ratio {
            return Some(FilterReason::HighCompressionRatio);
        }
        if is_sound_description(&segment.text)
            || (segment.no_speech_prob > self.no_speech_threshold
                && segment.avg_logprob < self.logprob_threshold)
        {
            return Some(FilterReason::NoSpeech);
        }
        None
    }

    /// Marks segments, checking each for these reasons in order and recording the first that
    /// applies:
    ///
    /// 1. [`FilterReason::KnownHallucination`]: the segment is only a known phrase (possibly
    ///    repeated) or starts with a known prefix.
    /// 2. [`FilterReason::HighCompressionRatio`]: [`compression_ratio`] of its text exceeds
    ///    `max_compression_ratio`.
    /// 3. [`FilterReason::NoSpeech`]: it is only a sound description ("[MUSIC]", "♪♪"), or the
    ///    model rated it as probably not speech (see `no_speech_threshold`).
    /// 4. [`FilterReason::Repeated`]: its normalised text equals the previous segment's. The first
    ///    of a run of identical lines is kept, so a chorus sung twice still counts once.
    ///
    /// Segments are never removed, and a segment that is already marked keeps its reason, so
    /// applying the filter twice changes nothing. Segments whose text is empty after
    /// normalisation are left unmarked; they contribute nothing to matching.
    pub fn apply(&self, segments: &mut [Segment]) {
        let mut previous: Option<String> = None;
        for segment in segments.iter_mut() {
            let norm = normalize(&segment.text);
            if segment.filtered.is_none() {
                segment.filtered = self.own_reason(segment, &norm).or_else(|| {
                    (!norm.is_empty() && previous.as_deref() == Some(norm.as_str()))
                        .then_some(FilterReason::Repeated)
                });
            }
            if !norm.is_empty() {
                previous = Some(norm);
            }
        }
    }
}

/// Ratio of UTF-8 length to zlib-compressed length (default compression level); repetitive text
/// scores high. Natural sentences score below about 2; a line looped many times scores far above
/// 2.4. Empty text scores 0.
pub fn compression_ratio(text: &str) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    // Writing to a Vec cannot fail.
    let compressed = encoder
        .write_all(text.as_bytes())
        .and_then(|()| encoder.finish())
        .map(|v| v.len())
        .unwrap_or(text.len());
    text.len() as f32 / compressed.max(1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str) -> Segment {
        Segment {
            start_s: 0.0,
            end_s: 1.0,
            text: text.into(),
            avg_logprob: -0.3,
            no_speech_prob: 0.05,
            filtered: None,
        }
    }

    fn reasons(texts: &[&str]) -> Vec<Option<FilterReason>> {
        let mut segs: Vec<Segment> = texts.iter().map(|t| seg(t)).collect();
        HallucinationFilter::default().apply(&mut segs);
        segs.into_iter().map(|s| s.filtered).collect()
    }

    #[test]
    fn normalize_lowercases_and_drops_punctuation() {
        assert_eq!(
            normalize("  What's YOUR function?!  "),
            "whats your function"
        );
        assert_eq!(normalize("Amara.org"), "amara org");
        assert_eq!(normalize("—"), "");
    }

    #[test]
    fn known_phrases_are_marked_only_when_they_are_the_whole_segment() {
        use FilterReason::KnownHallucination as K;
        assert_eq!(
            reasons(&[
                "Thank you.",
                " thank you thank you. Thank you!",
                "Thank you for the music, said the bird.",
                "Thanks for watching!",
                "You",
                "You know what I mean?",
            ]),
            vec![Some(K), Some(K), None, Some(K), Some(K), None]
        );
    }

    #[test]
    fn credit_prefixes_are_marked_whatever_follows() {
        use FilterReason::KnownHallucination as K;
        assert_eq!(
            reasons(&[
                "Subtitles by the Amara.org community",
                "Transcribed by ESO, translated by —",
                "Subtitlesbyfoo",
                "Subtitles",
            ]),
            vec![Some(K), Some(K), None, None]
        );
    }

    #[test]
    fn back_to_back_repeats_keep_the_first_line() {
        use FilterReason::Repeated as R;
        assert_eq!(
            reasons(&[
                "Conjunction junction, what's your function?",
                "Conjunction Junction what's your function",
                "conjunction junction, what's your function!",
                "Hooking up words and phrases and clauses.",
                "Conjunction junction, what's your function?",
            ]),
            vec![None, Some(R), Some(R), None, None]
        );
    }

    #[test]
    fn repeats_are_detected_across_blank_segments() {
        assert_eq!(
            reasons(&["Lolly, lolly, lolly", "  ", "lolly lolly lolly"]),
            vec![None, None, Some(FilterReason::Repeated)]
        );
    }

    #[test]
    fn looping_text_is_marked_by_compression_ratio() {
        let looped = "get your adverbs here ".repeat(12);
        assert!(compression_ratio(&looped) > 2.4);
        assert_eq!(
            reasons(&[&looped]),
            vec![Some(FilterReason::HighCompressionRatio)]
        );
    }

    #[test]
    fn natural_sentences_have_a_low_compression_ratio() {
        for text in [
            "I'm just a bill, yes I'm only a bill, and I'm sitting here on Capitol Hill.",
            "Three is a magic number.",
            "Interjections show excitement or emotion; they're generally set apart from a sentence by an exclamation point.",
        ] {
            assert!(compression_ratio(text) < 2.0, "{text}");
        }
        assert_eq!(compression_ratio(""), 0.0);
    }

    #[test]
    fn sound_descriptions_are_not_speech() {
        use FilterReason::NoSpeech as N;
        assert_eq!(
            reasons(&[
                "[MUSIC PLAYING]",
                "(upbeat music)",
                "♪♪",
                "*laughs*",
                "♪ Three is a magic number ♪",
                "(sighs) Fine, I'll go.",
            ]),
            vec![Some(N), Some(N), Some(N), Some(N), None, None]
        );
    }

    #[test]
    fn low_confidence_no_speech_segments_are_marked_but_confident_ones_kept() {
        let mut segs = vec![seg("ooh ahh"), seg("We the people")];
        segs[0].no_speech_prob = 0.9;
        segs[0].avg_logprob = -1.5;
        segs[1].no_speech_prob = 0.9;
        segs[1].avg_logprob = -0.2;
        HallucinationFilter::default().apply(&mut segs);
        assert_eq!(segs[0].filtered, Some(FilterReason::NoSpeech));
        assert_eq!(segs[1].filtered, None);
    }

    #[test]
    fn applying_twice_changes_nothing_and_existing_marks_are_kept() {
        let mut segs = vec![seg("Thank you."), seg("Hello there"), seg("Hello there")];
        segs[1].filtered = Some(FilterReason::NoSpeech);
        let filter = HallucinationFilter::default();
        filter.apply(&mut segs);
        let once = segs.clone();
        filter.apply(&mut segs);
        assert_eq!(segs, once);
        assert_eq!(segs[1].filtered, Some(FilterReason::NoSpeech));
        assert_eq!(segs[2].filtered, Some(FilterReason::Repeated));
    }

    #[test]
    fn segments_are_never_removed() {
        let mut segs = vec![seg("Thank you."), seg("you"), seg("[Music]")];
        HallucinationFilter::default().apply(&mut segs);
        assert_eq!(segs.len(), 3);
        assert!(segs.iter().all(|s| s.filtered.is_some()));
    }
}
