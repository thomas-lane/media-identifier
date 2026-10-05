//! Finding the episode title in what was heard.
//!
//! Songs usually sing their own title ("Conjunction Junction"), and many episodes say theirs, so
//! a fuzzy match of the title inside the transcript is strong evidence. A title is compared with
//! stretches of the transcript of about the same number of words, by characters and by
//! Double Metaphone codes, so a misheard title ("conjunction junkshun") still counts.

use std::collections::HashMap;

use crate::normalize::{is_stopword, tokenize};
use crate::text::{CodeCache, PreparedText, ratio};

/// A heard text laid out for title search (prepared once per file, searched for every title).
#[derive(Debug, Clone, Default)]
pub struct HeardText {
    words: Vec<String>,
    codes: Vec<String>,
    code_positions: HashMap<String, Vec<usize>>,
}

impl HeardText {
    /// Prepares `text` for title search.
    pub fn new(text: &str) -> Self {
        Self::from_prepared(&PreparedText::standalone(text))
    }

    /// Prepares an already tokenised text for title search.
    pub fn from_prepared(prepared: &PreparedText) -> Self {
        let mut cache = CodeCache::default();
        let words = prepared.tokens.words.clone();
        let codes: Vec<String> = words.iter().map(|w| cache.code(w).to_string()).collect();
        let mut code_positions: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, c) in codes.iter().enumerate() {
            code_positions.entry(c.clone()).or_default().push(i);
        }
        Self {
            words,
            codes,
            code_positions,
        }
    }
}

/// Where and how well a title was heard.
#[derive(Debug, Clone, PartialEq)]
pub struct TitleHook {
    /// Score, `0.0..=1.0` (see [`title_hook_score`]).
    pub score: f32,
    /// Token range of the best occurrence in the heard text.
    pub heard: std::ops::Range<usize>,
}

/// Character or phonetic similarity at or below which a title counts as not heard.
const HOOK_FLOOR: f32 = 0.7;
/// Similarity at or above which a title counts as clearly heard.
const HOOK_FULL: f32 = 0.95;
/// Phonetic codes are coarser than spelling, so a phonetic match counts slightly less.
const PHONETIC_DISCOUNT: f32 = 0.95;
/// Anchor positions tried per title.
const MAX_ANCHORS: usize = 400;

/// How well `title` occurs in `transcript`, `0.0..=1.0`.
///
/// Songs usually sing their own title ("Conjunction Junction"), so a fuzzy match of the title
/// inside the transcript is strong evidence. Titles of one or two very common words ("Pilot",
/// "The End") are weighted down, because they occur in many transcripts by chance.
pub fn title_hook_score(title: &str, transcript: &str) -> f32 {
    find_title(title, &HeardText::new(transcript)).map_or(0.0, |h| h.score)
}

/// The words of a title that are matched: text in parentheses or brackets ("Pilot (Part 1)") is
/// left out, because providers add it inconsistently and it is rarely spoken.
fn title_words(title: &str) -> Vec<String> {
    let mut cleaned = String::with_capacity(title.len());
    let mut depth = 0u32;
    for c in title.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => cleaned.push(c),
            _ => {}
        }
    }
    tokenize(&cleaned).words
}

/// How much a title's occurrence means: titles made of long, uncommon words are specific;
/// short and common ones ("Pilot", "The End") are heard by chance. Counts the characters of
/// non-stopword words: ten or more is fully specific.
pub fn title_specificity(title: &str) -> f32 {
    let content: usize = title_words(title)
        .iter()
        .filter(|w| !is_stopword(w))
        .map(|w| w.chars().count())
        .sum();
    if content == 0 {
        0.15
    } else {
        (content as f32 / 10.0).clamp(0.25, 1.0)
    }
}

/// Word-level phonetic agreement of two code sequences: twice the longest common subsequence
/// of equal codes, divided by the total number of words. Whole codes are compared, not their
/// characters, because codes are only one to six characters long and unrelated short codes
/// share characters by chance.
fn code_overlap(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    2.0 * lcs[0][0] as f32 / (a.len() + b.len()) as f32
}

/// Finds the best occurrence of `title` in `heard`.
pub fn find_title(title: &str, heard: &HeardText) -> Option<TitleHook> {
    let words = title_words(title);
    if words.is_empty() || heard.words.is_empty() {
        return None;
    }
    let mut cache = CodeCache::default();
    let codes: Vec<String> = words.iter().map(|w| cache.code(w).to_string()).collect();
    let title_text = words.join(" ");
    let t = words.len();
    let mut best = 0.0f32;
    let mut best_range = 0..0;
    let mut budget = MAX_ANCHORS;
    let mut tried = std::collections::HashSet::new();
    // Anchor on content words first; they are rarer, so fewer positions are tried.
    let mut order: Vec<usize> = (0..t).collect();
    order.sort_by_key(|&i| is_stopword(&words[i]));
    'outer: for i in order {
        let Some(positions) = heard.code_positions.get(&codes[i]) else {
            continue;
        };
        for &p in positions {
            if budget == 0 {
                break 'outer;
            }
            budget -= 1;
            for shift in -1isize..=1 {
                for len in t.saturating_sub(1).max(1)..=t + 1 {
                    let start = p as isize - i as isize + shift;
                    if start < 0 || start as usize + len > heard.words.len() {
                        continue;
                    }
                    let start = start as usize;
                    if !tried.insert((start, len)) {
                        continue;
                    }
                    let range = start..start + len;
                    let chars = ratio(&title_text, &heard.words[range.clone()].join(" "));
                    let phon =
                        code_overlap(&codes, &heard.codes[range.clone()]) * PHONETIC_DISCOUNT;
                    let r = chars.max(phon);
                    if r > best {
                        best = r;
                        best_range = range;
                    }
                }
            }
        }
    }
    let quality = ((best - HOOK_FLOOR) / (HOOK_FULL - HOOK_FLOOR)).clamp(0.0, 1.0);
    (quality > 0.0).then(|| TitleHook {
        score: quality * title_specificity(title),
        heard: best_range,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_title_in_song_scores_high() {
        let s = title_hook_score(
            "Conjunction Junction",
            "conjunction junction what's your function hooking up words and phrases",
        );
        assert!(s > 0.95, "{s}");
    }

    #[test]
    fn misheard_title_still_counts() {
        let s = title_hook_score(
            "Conjunction Junction",
            "conjunction junkshun what's your function",
        );
        assert!(s > 0.5, "{s}");
        let s = title_hook_score(
            "Interplanet Janet",
            "inter planet janet she's a galaxy girl",
        );
        assert!(s > 0.8, "{s}");
        let s = title_hook_score("Three Is a Magic Number", "3 is a magic number yes it is");
        assert!(s > 0.9, "{s}");
    }

    #[test]
    fn absent_title_scores_zero() {
        let s = title_hook_score(
            "Conjunction Junction",
            "interjections show excitement or emotion",
        );
        assert_eq!(s, 0.0);
        assert_eq!(title_hook_score("", "anything"), 0.0);
        assert_eq!(title_hook_score("Title", ""), 0.0);
    }

    #[test]
    fn common_short_titles_weigh_less() {
        let generic = title_hook_score("The End", "and that was the end of that");
        let specific =
            title_hook_score("Interplanet Janet", "interplanet janet she's a galaxy girl");
        assert!(generic < 0.4, "{generic}");
        assert!(specific > 0.9, "{specific}");
        assert!(title_specificity("Pilot") < title_specificity("The Tortoise and the Hare"));
    }

    #[test]
    fn phonetic_matches_need_whole_words() {
        assert_eq!(code_overlap(&[], &[]), 1.0);
        let a: Vec<String> = ["0", "P", "H"].iter().map(|s| s.to_string()).collect();
        let b: Vec<String> = ["0", "PK", "H"].iter().map(|s| s.to_string()).collect();
        assert!((code_overlap(&a, &b) - 2.0 * 2.0 / 6.0).abs() < 1e-6);
        // Unrelated words with short codes do not add up to a title.
        let s = title_hook_score(
            "The Boy Who Cried Wolf",
            "then the writers sit yellow this and big with basket argue jokes",
        );
        assert_eq!(s, 0.0);
    }

    #[test]
    fn parenthesised_parts_are_ignored() {
        let s = title_hook_score("Lucky Seven Sampson (Part 1)", "lucky seven samson");
        assert!(s > 0.8, "{s}");
    }

    #[test]
    fn reports_where_the_title_was_heard() {
        let heard = HeardText::new("well then my friends conjunction junction is here");
        let hook = find_title("Conjunction Junction", &heard).expect("found");
        assert_eq!(hook.heard, 4..6);
    }
}
