//! The "heard" and "reference" quotes on the Review screen, with overlapping words highlighted.
//!
//! Quotes show the original words (not the normalised tokens), cut around the best overlap with
//! a few words of context on each side. Words are marked as matched when a word-by-word
//! alignment pairs them with a word on the other side that is the same, sounds the same
//! (same Double Metaphone code) or is spelt almost the same; the alignment is a longest common
//! subsequence, so matched words appear in the same order on both sides.

use std::ops::Range;

use mi_types::QuotePart;

use crate::text::{CodeCache, PreparedText, Tokens, ratio};

/// Words of context shown either side of the overlap.
const CONTEXT_WORDS: usize = 6;
/// Two different words count as matched when their character similarity reaches this.
const WORD_MATCH: f32 = 0.8;
/// Marks a cut at either end of a quote.
const ELLIPSIS: &str = "\u{2026}";

/// Extends `range` by [`CONTEXT_WORDS`] on both sides within `0..len`.
fn with_context(range: &Range<usize>, len: usize) -> Range<usize> {
    range.start.saturating_sub(CONTEXT_WORDS)..(range.end + CONTEXT_WORDS).min(len)
}

/// Quotes for an overlap between heard text and a reference, each cut to the overlap plus
/// context, with words marked where the two sides agree.
pub(crate) fn overlap_quotes(
    heard: &PreparedText,
    heard_range: &Range<usize>,
    reference: &PreparedText,
    reference_range: &Range<usize>,
) -> (Vec<QuotePart>, Vec<QuotePart>) {
    let h = with_context(heard_range, heard.len());
    let r = with_context(reference_range, reference.len());
    let (hm, rm) = align_words(
        &heard.tokens.words[h.clone()],
        &reference.tokens.words[r.clone()],
    );
    (
        render(&heard.text, &heard.tokens, h, &hm),
        render(&reference.text, &reference.tokens, r, &rm),
    )
}

/// A quote of `range` of a text (extended by context) with the given per-token marks for the
/// whole text.
pub(crate) fn marked_quote(
    text: &str,
    tokens: &Tokens,
    range: &Range<usize>,
    matched: &[bool],
) -> Vec<QuotePart> {
    let r = with_context(range, tokens.len());
    render(text, tokens, r.clone(), &matched[r])
}

/// Marks words of `a` and `b` paired by a longest common subsequence under fuzzy word equality.
fn align_words(a: &[String], b: &[String]) -> (Vec<bool>, Vec<bool>) {
    let mut cache = CodeCache::default();
    let ac: Vec<String> = a.iter().map(|w| cache.code(w).to_string()).collect();
    let bc: Vec<String> = b.iter().map(|w| cache.code(w).to_string()).collect();
    let same =
        |i: usize, j: usize| a[i] == b[j] || ac[i] == bc[j] || ratio(&a[i], &b[j]) >= WORD_MATCH;
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if same(i, j) {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut am = vec![false; n];
    let mut bm = vec![false; m];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if same(i, j) && lcs[i][j] == lcs[i + 1][j + 1] + 1 {
            am[i] = true;
            bm[j] = true;
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    (am, bm)
}

/// Renders tokens `range` of `text` as quote parts. `matched` has one flag per token in `range`.
/// Tokens that came from one original word (a number spelled as several words) form one piece,
/// matched when any of them is. Whitespace runs, including line breaks between subtitle cues,
/// become single spaces.
fn render(text: &str, tokens: &Tokens, range: Range<usize>, matched: &[bool]) -> Vec<QuotePart> {
    let mut parts: Vec<QuotePart> = Vec::new();
    let push = |s: &str, m: bool, parts: &mut Vec<QuotePart>| {
        if s.is_empty() {
            return;
        }
        match parts.last_mut() {
            Some(last) if last.matched == m => last.text.push_str(s),
            _ => parts.push(QuotePart {
                text: s.to_string(),
                matched: m,
            }),
        }
    };
    if range.is_empty() {
        return parts;
    }
    // Group tokens sharing one original span.
    let mut units: Vec<(Range<usize>, bool)> = Vec::new();
    for (k, i) in range.clone().enumerate() {
        let span = tokens.spans[i].clone();
        match units.last_mut() {
            Some((s, m)) if *s == span => *m |= matched[k],
            _ => units.push((span, matched[k])),
        }
    }
    if range.start > 0 {
        push(&format!("{ELLIPSIS} "), false, &mut parts);
    }
    for (u, (span, m)) in units.iter().enumerate() {
        if u > 0 {
            let gap = collapse_whitespace(&text[units[u - 1].0.end..span.start]);
            let joins_matches = units[u - 1].1 && *m;
            push(&gap, joins_matches, &mut parts);
        }
        push(&text[span.clone()], *m, &mut parts);
    }
    // Keep punctuation that closes the last word ("word," or "word?").
    let last = &units.last().expect("non-empty").0;
    let tail: String = text[last.end..]
        .chars()
        .take_while(|c| !c.is_whitespace() && !c.is_alphanumeric())
        .collect();
    push(&tail, false, &mut parts);
    if range.end < tokens.len() {
        push(&format!(" {ELLIPSIS}"), false, &mut parts);
    }
    parts
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(c);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(parts: &[QuotePart]) -> String {
        parts.iter().map(|p| p.text.as_str()).collect()
    }

    fn matched(parts: &[QuotePart]) -> Vec<&str> {
        parts
            .iter()
            .filter(|p| p.matched)
            .map(|p| p.text.as_str())
            .collect()
    }

    #[test]
    fn marks_words_both_sides_agree_on() {
        let heard = PreparedText::standalone("there knight rode to the see");
        let reference = PreparedText::standalone("Their knight rode quickly to the sea.");
        let (h, r) = overlap_quotes(&heard, &(0..6), &reference, &(0..7));
        assert_eq!(joined(&h), "there knight rode to the see");
        assert_eq!(joined(&r), "Their knight rode quickly to the sea.");
        assert_eq!(matched(&h), ["there knight rode to the see"]);
        assert_eq!(matched(&r), ["Their knight rode", "to the sea"]);
        assert_eq!(r.last().map(|p| p.text.as_str()), Some("."));
    }

    #[test]
    fn cuts_long_texts_with_ellipses() {
        let words: Vec<String> = (0..40).map(|i| format!("w{i}")).collect();
        let text = words.join(" ");
        let p = PreparedText::standalone(&text);
        let marks = vec![false; p.len()];
        let q = marked_quote(&p.text, &p.tokens, &(20..22), &marks);
        let s = joined(&q);
        assert!(s.starts_with('\u{2026}'), "{s}");
        assert!(s.ends_with('\u{2026}'), "{s}");
        // Tokens 20..22 are "w10"; six tokens of context reach back to "w7" and on to "w13".
        assert!(s.contains("w7 w8 w9 w10 w11 w12 w13"), "{s}");
        assert!(!s.contains("w6 ") && !s.contains("w14"), "{s}");
    }

    #[test]
    fn numbers_spelled_as_several_words_stay_one_piece() {
        let heard = PreparedText::standalone("born in 1973 they said");
        let reference = PreparedText::standalone("born in nineteen seventy three");
        let (h, _) = overlap_quotes(&heard, &(0..5), &reference, &(0..5));
        assert_eq!(matched(&h), ["born in 1973"]);
    }

    #[test]
    fn line_breaks_become_spaces() {
        let reference = PreparedText::standalone("first cue\nsecond cue");
        let marks = vec![true; reference.len()];
        let q = marked_quote(&reference.text, &reference.tokens, &(0..4), &marks);
        assert_eq!(joined(&q), "first cue second cue");
        assert_eq!(q.len(), 1);
    }
}
