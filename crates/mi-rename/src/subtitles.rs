//! Subtitle files written from what the speech model heard.

use mi_types::Segment;

/// Formats heard segments as SubRip (`.srt`) text.
///
/// Segments the hallucination filter dropped and blank segments are left out, because they are
/// text the model invented rather than dialogue. Times are the segments' own, measured from the
/// start of the file, so a sampled file gets subtitles only for its sampled windows. Returns
/// `None` when nothing is left, so no empty file is written.
pub fn heard_srt(segments: &[Segment]) -> Option<String> {
    let mut out = String::new();
    let mut n = 0;
    for segment in segments.iter().filter(|s| s.filtered.is_none()) {
        let text = segment.text.trim();
        if text.is_empty() {
            continue;
        }
        n += 1;
        out.push_str(&format!(
            "{n}\n{} --> {}\n{text}\n\n",
            timestamp(segment.start_s),
            timestamp(segment.end_s.max(segment.start_s)),
        ));
    }
    (n > 0).then_some(out)
}

fn timestamp(seconds: f64) -> String {
    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_types::FilterReason;

    fn seg(start_s: f64, end_s: f64, text: &str, filtered: Option<FilterReason>) -> Segment {
        Segment {
            start_s,
            end_s,
            text: text.into(),
            avg_logprob: -0.3,
            no_speech_prob: 0.01,
            filtered,
        }
    }

    #[test]
    fn writes_numbered_cues_and_skips_filtered_text() {
        let srt = heard_srt(&[
            seg(1.5, 3.25, " Conjunction junction ", None),
            seg(
                4.0,
                5.0,
                "Thank you.",
                Some(FilterReason::KnownHallucination),
            ),
            seg(5.0, 6.0, "  ", None),
            seg(3725.001, 3727.0, "what's your function", None),
        ])
        .unwrap();
        assert_eq!(
            srt,
            "1\n00:00:01,500 --> 00:00:03,250\nConjunction junction\n\n\
             2\n01:02:05,001 --> 01:02:07,000\nwhat's your function\n\n"
        );
    }

    #[test]
    fn nothing_heard_means_no_file() {
        assert_eq!(heard_srt(&[]), None);
        assert_eq!(
            heard_srt(&[seg(0.0, 1.0, "x", Some(FilterReason::Repeated))]),
            None
        );
    }
}
