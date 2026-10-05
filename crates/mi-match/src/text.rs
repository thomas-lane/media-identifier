//! Fuzzy text similarity that tolerates misheard words.
//!
//! The dialogue signal compares what was heard in a file (its transcript, or its own embedded
//! subtitles) with each episode's reference text. Three measures are combined, because each
//! survives a different kind of speech-recognition error:
//!
//! - **Word TF-IDF cosine** over word unigrams and bigrams: correct words, weighted by how
//!   specific they are to one episode.
//! - **Phonetic TF-IDF cosine** over Double Metaphone codes and code bigrams: words that sound
//!   alike ("there"/"their", "Smith"/"Smyth", "for"/"four") count as equal.
//! - **Phrase coverage**: the share of heard phrases (six words each) found, at the character
//!   level, somewhere in the reference. Character comparison survives misspellings and word
//!   boundary errors ("every body"/"everybody") that both cosines miss.
//!
//! See `docs/identification.md` for the reasons behind each choice and the weights.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use rphonetic::DoubleMetaphone;

pub use crate::normalize::{Tokens, tokenize};

/// Words per heard phrase in phrase coverage.
pub const PHRASE_WORDS: usize = 6;

/// Maximum Double Metaphone code length. Longer than the usual 4, so long words that begin alike
/// ("transportation" and "transformation" both give `TRNS` at length 4) stay distinguishable.
const CODE_LENGTH: usize = 6;

/// The Double Metaphone primary code of one normalised word, or the word itself when the word
/// has no code (Double Metaphone ignores digits and some symbols).
pub fn phonetic_code(word: &str) -> String {
    let code = DoubleMetaphone::new(Some(CODE_LENGTH))
        .double_metaphone(word)
        .primary();
    if code.is_empty() {
        word.to_string()
    } else {
        code
    }
}

/// Double Metaphone codes of each token, joined into shingles of `n` consecutive codes, so words
/// that sound alike ("Smith"/"Smyth", "there"/"their") compare equal. Fewer than `n` tokens
/// give one shorter shingle; no tokens give none.
pub fn phonetic_shingles(tokens: &Tokens, n: usize) -> Vec<String> {
    let codes: Vec<String> = tokens.words.iter().map(|w| phonetic_code(w)).collect();
    let n = n.max(1);
    if codes.is_empty() {
        return Vec::new();
    }
    if codes.len() < n {
        return vec![codes.join(" ")];
    }
    codes.windows(n).map(|w| w.join(" ")).collect()
}

/// Indel similarity (RapidFuzz `ratio`) of two strings, `0.0..=1.0`: one minus the share of
/// characters that must be inserted or deleted to turn one into the other.
pub fn ratio(a: &str, b: &str) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    rapidfuzz::fuzz::ratio(a.chars(), b.chars()) as f32
}

/// Best fuzzy similarity (`0.0..=1.0`) of `needle` against any equally long stretch of
/// `haystack`, at the character level (RapidFuzz partial ratio). Stretches that run off either
/// end of `haystack` are tried too, so a needle cut at the start or end of a transcript still
/// scores. The shorter string is always used as the needle. Empty input scores 0.
pub fn partial_ratio(needle: &str, haystack: &str) -> f32 {
    let (needle, haystack) = if needle.chars().count() <= haystack.chars().count() {
        (needle, haystack)
    } else {
        (haystack, needle)
    };
    let n: Vec<char> = needle.chars().collect();
    let h: Vec<char> = haystack.chars().collect();
    partial_ratio_chars(&n, &h)
}

/// [`ratio`] on character slices.
///
/// RapidFuzz's `RatioBatchComparator` (version 0.5) is not used: for strings of different
/// lengths it divides by the longer length instead of the sum of both, so it disagrees with
/// `ratio` (for example 0.5 instead of 0.667 for "abcdefgh" against "efgh").
pub(crate) fn ratio_chars(a: &[char], b: &[char]) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    rapidfuzz::fuzz::ratio(a.iter().copied(), b.iter().copied()) as f32
}

/// A string prepared for many [`ratio`] comparisons against other strings.
///
/// Strings of up to 64 characters use the bit-parallel longest-common-subsequence algorithm
/// (Hyyrö): each character of the other string costs a few machine-word operations. The indel
/// similarity is `2 × LCS / (len_a + len_b)`, the same value RapidFuzz's `ratio` returns; longer
/// strings fall back to `ratio`.
pub(crate) struct Pattern {
    chars: Vec<char>,
    ascii: [u64; 128],
    other: Vec<(char, u64)>,
}

impl Pattern {
    pub(crate) fn new(chars: &[char]) -> Self {
        let mut ascii = [0u64; 128];
        let mut other: Vec<(char, u64)> = Vec::new();
        if chars.len() <= 64 {
            for (i, &c) in chars.iter().enumerate() {
                let bit = 1u64 << i;
                if (c as u32) < 128 {
                    ascii[c as usize] |= bit;
                } else if let Some(e) = other.iter_mut().find(|(o, _)| *o == c) {
                    e.1 |= bit;
                } else {
                    other.push((c, bit));
                }
            }
        }
        Self {
            chars: chars.to_vec(),
            ascii,
            other,
        }
    }

    fn mask(&self, c: char) -> u64 {
        if (c as u32) < 128 {
            self.ascii[c as usize]
        } else {
            self.other
                .iter()
                .find(|(o, _)| *o == c)
                .map_or(0, |(_, m)| *m)
        }
    }

    /// Indel similarity with `other`, `0.0..=1.0`.
    pub(crate) fn ratio(&self, other: &[char]) -> f32 {
        let m = self.chars.len();
        if m > 64 {
            return ratio_chars(&self.chars, other);
        }
        if m + other.len() == 0 {
            return 1.0;
        }
        let mut v = u64::MAX;
        for &c in other {
            let u = v & self.mask(c);
            v = v.wrapping_add(u) | (v & !self.mask(c));
        }
        let used = if m == 64 { u64::MAX } else { (1u64 << m) - 1 };
        let lcs = (!v & used).count_ones() as usize;
        (2 * lcs) as f32 / (m + other.len()) as f32
    }
}

/// [`partial_ratio`] on character slices; `needle` must not be longer than `haystack`.
pub(crate) fn partial_ratio_chars(n: &[char], h: &[char]) -> f32 {
    if n.is_empty() || n.len() > h.len() {
        return 0.0;
    }
    let pattern = Pattern::new(n);
    let score = |s: &[char]| pattern.ratio(s);
    let len = n.len();
    let mut best = 0.0f32;
    for start in 0..=h.len() - len {
        best = best.max(score(&h[start..start + len]));
        if best >= 1.0 {
            return 1.0;
        }
    }
    for k in (len / 2).max(1)..len {
        best = best.max(score(&h[..k])).max(score(&h[h.len() - k..]));
    }
    best
}

/// Interned ids for words or phonetic codes.
#[derive(Debug, Clone, Default)]
struct Interner {
    ids: HashMap<String, u32>,
}

impl Interner {
    fn intern(&mut self, s: &str) -> u32 {
        let next = self.ids.len() as u32;
        *self.ids.entry(s.to_string()).or_insert(next)
    }

    fn get(&self, s: &str) -> Option<u32> {
        self.ids.get(s).copied()
    }

    fn len(&self) -> u32 {
        self.ids.len() as u32
    }
}

/// Caches Double Metaphone codes; the same words recur throughout a job.
#[derive(Debug, Default)]
pub(crate) struct CodeCache {
    codes: HashMap<String, String>,
}

impl CodeCache {
    pub(crate) fn code(&mut self, word: &str) -> &str {
        if !self.codes.contains_key(word) {
            self.codes.insert(word.to_string(), phonetic_code(word));
        }
        &self.codes[word]
    }
}

/// A tokenised text laid out for the comparisons in this module.
#[derive(Debug, Clone, Default)]
pub struct PreparedText {
    /// The original text.
    pub text: String,
    /// Its tokens.
    pub tokens: Tokens,
    word_ids: Vec<u32>,
    code_ids: Vec<u32>,
    /// Normalised words joined by single spaces, as characters.
    chars: Vec<char>,
    /// Range of each token in `chars`.
    char_ranges: Vec<Range<usize>>,
    /// Positions of each phonetic code id.
    code_positions: HashMap<u32, Vec<u32>>,
}

impl PreparedText {
    fn new(
        text: &str,
        words: &mut dyn FnMut(&str) -> u32,
        codes: &mut dyn FnMut(&str) -> u32,
    ) -> Self {
        let tokens = tokenize(text);
        let mut chars = Vec::new();
        let mut char_ranges = Vec::with_capacity(tokens.len());
        let mut word_ids = Vec::with_capacity(tokens.len());
        let mut code_ids = Vec::with_capacity(tokens.len());
        let mut code_positions: HashMap<u32, Vec<u32>> = HashMap::new();
        for (i, w) in tokens.words.iter().enumerate() {
            if !chars.is_empty() {
                chars.push(' ');
            }
            let start = chars.len();
            chars.extend(w.chars());
            char_ranges.push(start..chars.len());
            word_ids.push(words(w));
            let c = codes(w);
            code_ids.push(c);
            code_positions.entry(c).or_default().push(i as u32);
        }
        Self {
            text: text.to_string(),
            tokens,
            word_ids,
            code_ids,
            chars,
            char_ranges,
            code_positions,
        }
    }

    /// Prepares a text on its own (ids are local to it).
    pub fn standalone(text: &str) -> Self {
        let mut words = Interner::default();
        let mut codes = Interner::default();
        let mut cache = CodeCache::default();
        Self::new(text, &mut |w| words.intern(w), &mut |w| {
            codes.intern(cache.code(w))
        })
    }

    /// Number of tokens.
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// True when the text has no tokens.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// The characters of tokens `start..start + len`, joined by spaces.
    pub(crate) fn window(&self, start: usize, len: usize) -> &[char] {
        let first = self.char_ranges[start].start;
        let last = self.char_ranges[start + len - 1].end;
        &self.chars[first..last]
    }
}

/// A sparse vector sorted by term key.
type SparseVec = Vec<(u64, f32)>;

fn unigram(a: u32) -> u64 {
    u64::from(a)
}

fn bigram(a: u32, b: u32) -> u64 {
    (1u64 << 63) | (u64::from(a) << 32) | u64::from(b)
}

/// Term counts of unigrams and bigrams of `ids`.
fn term_counts(ids: &[u32]) -> HashMap<u64, u32> {
    let mut counts = HashMap::new();
    for &a in ids {
        *counts.entry(unigram(a)).or_insert(0) += 1;
    }
    for w in ids.windows(2) {
        *counts.entry(bigram(w[0], w[1])).or_insert(0) += 1;
    }
    counts
}

/// BM25-style inverse document frequency. Terms in every document weigh almost nothing, so lines
/// every episode shares (theme songs, recurring names) do not make episodes look alike.
fn idf(n_docs: usize, df: u32) -> f32 {
    let n = n_docs as f32;
    let df = df as f32;
    (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
}

fn weighted(counts: &HashMap<u64, u32>, idf_of: impl Fn(u64) -> f32) -> SparseVec {
    let mut v: SparseVec = counts
        .iter()
        .map(|(&k, &tf)| (k, (1.0 + (tf as f32).ln()) * idf_of(k)))
        .collect();
    let norm = v.iter().map(|(_, x)| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for (_, x) in &mut v {
            *x /= norm;
        }
    }
    v.sort_unstable_by_key(|(k, _)| *k);
    v
}

fn dot(a: &SparseVec, b: &SparseVec) -> f32 {
    let (mut i, mut j, mut sum) = (0, 0, 0.0);
    while i < a.len() && j < b.len() {
        match a[i].0.cmp(&b[j].0) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                sum += a[i].1 * b[j].1;
                i += 1;
                j += 1;
            }
        }
    }
    sum.clamp(0.0, 1.0)
}

#[derive(Debug, Clone)]
struct IndexedDocument {
    prepared: PreparedText,
    word_vec: SparseVec,
    code_vec: SparseVec,
}

/// TF-IDF index over the reference texts of all candidate episodes, with word unigrams and
/// bigrams and phonetic-code unigrams and bigrams as terms. Inverse document frequencies come
/// from the candidate episodes themselves, so lines every episode shares (theme songs, credits)
/// weigh little.
#[derive(Debug, Clone, Default)]
pub struct TfIdfIndex {
    /// Document ids in insertion order.
    pub documents: Vec<String>,
    words: Interner,
    codes: Interner,
    word_df: HashMap<u64, u32>,
    code_df: HashMap<u64, u32>,
    docs: Vec<IndexedDocument>,
}

/// A heard text prepared once and compared with every document of a [`TfIdfIndex`].
#[derive(Debug, Clone)]
pub struct Query {
    /// The prepared text.
    pub prepared: PreparedText,
    word_vec: SparseVec,
    code_vec: SparseVec,
    phrases: Vec<Phrase>,
}

#[derive(Debug, Clone)]
struct Phrase {
    tokens: Range<usize>,
    chars: Vec<char>,
    weight: f32,
}

/// Where the heard text and a reference overlap best.
#[derive(Debug, Clone, PartialEq)]
pub struct Overlap {
    /// Token range in the heard text.
    pub heard: Range<usize>,
    /// Token range in the reference text.
    pub reference: Range<usize>,
    /// Character similarity of the two ranges, `0.0..=1.0`.
    pub ratio: f32,
}

/// The dialogue similarity of one heard text to one reference, with its components.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DialogueScore {
    /// Combined similarity, `0.0..=1.0`.
    pub similarity: f32,
    /// Word TF-IDF cosine.
    pub word_cosine: f32,
    /// Phonetic TF-IDF cosine.
    pub phonetic_cosine: f32,
    /// Weighted share of heard phrases found in the reference.
    pub phrase_coverage: f32,
    /// The best-matching phrase, for the Review screen's quotes.
    pub best_overlap: Option<Overlap>,
}

/// A phrase counts as fully found when its character similarity to some stretch of the reference
/// reaches this; between [`PHRASE_FLOOR`] and this it counts partly. Two misheard words in six
/// still give about 0.75.
const PHRASE_FULL: f32 = 0.88;
/// Character similarity that unrelated English phrases of equal length reach by chance.
const PHRASE_FLOOR: f32 = 0.6;
/// Anchor positions tried per phrase.
const MAX_ANCHORS: usize = 12;
/// A cosine at or above this already indicates the same dialogue (sampled transcripts cover only
/// part of an episode, so a match never reaches 1).
const COSINE_FULL: f32 = 0.45;

impl TfIdfIndex {
    /// Builds the index from `(document id, text)` pairs.
    pub fn build(documents: &[(String, String)]) -> Self {
        let mut index = TfIdfIndex::default();
        let mut cache = CodeCache::default();
        let mut prepared = Vec::with_capacity(documents.len());
        for (id, text) in documents {
            index.documents.push(id.clone());
            let words = &mut index.words;
            let codes = &mut index.codes;
            let p = PreparedText::new(text, &mut |w| words.intern(w), &mut |w| {
                codes.intern(cache.code(w))
            });
            prepared.push(p);
        }
        let mut word_counts = Vec::with_capacity(prepared.len());
        let mut code_counts = Vec::with_capacity(prepared.len());
        for p in &prepared {
            let wc = term_counts(&p.word_ids);
            let cc = term_counts(&p.code_ids);
            for k in wc.keys() {
                *index.word_df.entry(*k).or_insert(0) += 1;
            }
            for k in cc.keys() {
                *index.code_df.entry(*k).or_insert(0) += 1;
            }
            word_counts.push(wc);
            code_counts.push(cc);
        }
        let n = prepared.len();
        for ((p, wc), cc) in prepared.into_iter().zip(word_counts).zip(code_counts) {
            let word_vec = weighted(&wc, |k| idf(n, index.word_df[&k]));
            let code_vec = weighted(&cc, |k| idf(n, index.code_df[&k]));
            index.docs.push(IndexedDocument {
                prepared: p,
                word_vec,
                code_vec,
            });
        }
        index
    }

    /// Number of documents.
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// True when the index has no documents.
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// The prepared text of document `i`.
    pub fn document(&self, i: usize) -> &PreparedText {
        &self.docs[i].prepared
    }

    /// Inverse document frequency of a term; a term no document contains is weighted like a term
    /// only one contains (it is most likely a misheard word, and should not outweigh real ones).
    fn term_idf(&self, df: Option<u32>) -> f32 {
        idf(self.docs.len().max(1), df.unwrap_or(1).max(1))
    }

    /// Prepares a heard text for comparison with every document.
    pub fn query(&self, text: &str) -> Query {
        let mut cache = CodeCache::default();
        let mut extra_words: HashMap<String, u32> = HashMap::new();
        let mut extra_codes: HashMap<String, u32> = HashMap::new();
        let base_w = self.words.len();
        let base_c = self.codes.len();
        let prepared = PreparedText::new(
            text,
            &mut |w| {
                self.words.get(w).unwrap_or_else(|| {
                    let next = base_w + extra_words.len() as u32;
                    *extra_words.entry(w.to_string()).or_insert(next)
                })
            },
            &mut |w| {
                let c = cache.code(w);
                self.codes.get(c).unwrap_or_else(|| {
                    let next = base_c + extra_codes.len() as u32;
                    *extra_codes.entry(c.to_string()).or_insert(next)
                })
            },
        );
        let word_vec = weighted(&term_counts(&prepared.word_ids), |k| {
            self.term_idf(self.word_df.get(&k).copied())
        });
        let code_vec = weighted(&term_counts(&prepared.code_ids), |k| {
            self.term_idf(self.code_df.get(&k).copied())
        });
        let phrases = phrase_ranges(prepared.len())
            .into_iter()
            .map(|r| Phrase {
                chars: prepared.window(r.start, r.len()).to_vec(),
                weight: r
                    .clone()
                    .map(|i| {
                        self.term_idf(self.word_df.get(&unigram(prepared.word_ids[i])).copied())
                    })
                    .sum(),
                tokens: r,
            })
            .collect();
        Query {
            prepared,
            word_vec,
            code_vec,
            phrases,
        }
    }

    /// Combined cosine similarity (mean of the word and phonetic cosines) of `query` to every
    /// document, in document order, each in `0.0..=1.0`.
    pub fn similarities(&self, query: &str) -> Vec<f32> {
        let q = self.query(query);
        self.docs
            .iter()
            .map(|d| 0.5 * (dot(&q.word_vec, &d.word_vec) + dot(&q.code_vec, &d.code_vec)))
            .collect()
    }

    /// Dialogue similarity of a prepared query to document `document`.
    pub fn score(&self, query: &Query, document: usize) -> DialogueScore {
        let doc = &self.docs[document];
        let word_cosine = dot(&query.word_vec, &doc.word_vec);
        let phonetic_cosine = dot(&query.code_vec, &doc.code_vec);
        let (phrase_coverage, best_overlap) = phrase_coverage(query, &doc.prepared);
        let calibrated = |c: f32| (c / COSINE_FULL).min(1.0);
        let similarity = (0.5 * phrase_coverage
            + 0.25 * calibrated(word_cosine)
            + 0.25 * calibrated(phonetic_cosine))
        .clamp(0.0, 1.0);
        DialogueScore {
            similarity,
            word_cosine,
            phonetic_cosine,
            phrase_coverage,
            best_overlap,
        }
    }
}

/// Splits `n` tokens into consecutive phrases of [`PHRASE_WORDS`]; a short remainder joins the
/// last phrase.
fn phrase_ranges(n: usize) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut start = 0;
    while start < n {
        let end = (start + PHRASE_WORDS).min(n);
        match out.last_mut() {
            Some(last) if end - start < PHRASE_WORDS / 2 => last.end = end,
            _ => out.push(start..end),
        }
        start = end;
    }
    out
}

/// Maps a phrase's character similarity to how much the phrase counts as found.
fn phrase_hit(ratio: f32) -> f32 {
    ((ratio - PHRASE_FLOOR) / (PHRASE_FULL - PHRASE_FLOOR)).clamp(0.0, 1.0)
}

/// Weighted share of the query's phrases found in `doc`, and the best overlap.
///
/// Comparing each phrase with every stretch of a long reference would be slow, so candidate
/// stretches are anchored: a phrase is compared only where one of its rarer words (by phonetic
/// code) occurs in the reference, with the stretch shifted by up to one word either way to
/// absorb dropped and inserted words.
fn phrase_coverage(query: &Query, doc: &PreparedText) -> (f32, Option<Overlap>) {
    let q = &query.prepared;
    if query.phrases.is_empty() || doc.is_empty() {
        return (0.0, None);
    }
    let mut total_weight = 0.0f32;
    let mut found = 0.0f32;
    let mut best: Option<(f32, Overlap)> = None;
    for phrase in &query.phrases {
        total_weight += phrase.weight;
        let mut anchors: Vec<(usize, &[u32])> = phrase
            .tokens
            .clone()
            .map(|i| {
                let positions = doc
                    .code_positions
                    .get(&q.code_ids[i])
                    .map_or(&[][..], Vec::as_slice);
                (i - phrase.tokens.start, positions)
            })
            .filter(|(_, p)| !p.is_empty())
            .collect();
        anchors.sort_by_key(|(_, p)| p.len());
        let mut tried = HashSet::new();
        let mut best_ratio = 0.0f32;
        let mut best_range = 0..0;
        let mut budget = MAX_ANCHORS;
        let pattern = Pattern::new(&phrase.chars);
        let plen = phrase.tokens.len();
        'anchors: for (offset, positions) in anchors {
            for &pos in positions {
                if budget == 0 {
                    break 'anchors;
                }
                budget -= 1;
                let base = pos as isize - offset as isize;
                // Shifting by one word absorbs a dropped or inserted word before the anchor;
                // character similarity absorbs the length difference it causes.
                for shift in -1isize..=1 {
                    let start = base + shift;
                    if start < 0 || start as usize + plen > doc.len() {
                        continue;
                    }
                    let start = start as usize;
                    if !tried.insert(start) {
                        continue;
                    }
                    let r = pattern.ratio(doc.window(start, plen));
                    if r > best_ratio {
                        best_ratio = r;
                        best_range = start..start + plen;
                    }
                }
            }
        }
        let hit = phrase_hit(best_ratio);
        let strength = hit * phrase.weight;
        found += strength;
        if strength > 0.0 && best.as_ref().is_none_or(|(s, _)| strength > *s) {
            best = Some((
                strength,
                Overlap {
                    heard: phrase.tokens.clone(),
                    reference: best_range,
                    ratio: best_ratio,
                },
            ));
        }
    }
    let coverage = if total_weight > 0.0 {
        (found / total_weight).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (coverage, best.map(|(_, o)| o))
}

/// Combined dialogue similarity of a transcript to one reference text, `0.0..=1.0`, from the
/// TF-IDF, phonetic and partial-match components. Prepare the transcript once with
/// [`TfIdfIndex::query`] and use [`TfIdfIndex::score`] when comparing with many documents.
pub fn dialogue_similarity(index: &TfIdfIndex, document: usize, transcript: &str) -> f32 {
    index.score(&index.query(transcript), document).similarity
}

/// Summaries of episodes that have no dialogue text, for the fallback comparison.
///
/// A summary describes an episode rather than quoting it, so it is compared by its distinctive
/// words only: names, places and objects that the summary mentions and that are heard in the
/// file. Each summary word is weighted by how few of the summaries contain it.
#[derive(Debug, Clone, Default)]
pub struct SummaryIndex {
    docs: Vec<SummaryDoc>,
}

#[derive(Debug, Clone)]
struct SummaryDoc {
    prepared: PreparedText,
    /// Distinct content words: (word, phonetic code, weight).
    keywords: Vec<(String, String, f32)>,
}

/// How a heard text compares with one summary.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SummaryScore {
    /// Similarity, `0.0..=1.0`; at most [`SUMMARY_CAP`], because a summary is weaker evidence
    /// than dialogue.
    pub similarity: f32,
    /// Weighted share of the summary's distinctive words that were heard.
    pub coverage: f32,
    /// For each summary token, whether it was heard (for highlighting).
    pub summary_matched: Vec<bool>,
    /// For each heard token, whether it is one of the summary's words (for highlighting).
    pub heard_matched: Vec<bool>,
}

/// Highest similarity a summary comparison can give.
pub const SUMMARY_CAP: f32 = 0.8;

impl SummaryIndex {
    /// Builds the index from summary texts.
    pub fn build(summaries: &[String]) -> Self {
        let mut cache = CodeCache::default();
        let prepared: Vec<PreparedText> = summaries
            .iter()
            .map(|s| PreparedText::standalone(s))
            .collect();
        let mut df: HashMap<String, u32> = HashMap::new();
        let mut per_doc = Vec::new();
        for p in &prepared {
            let mut seen: Vec<String> = Vec::new();
            for w in &p.tokens.words {
                if w.chars().count() >= 3 && !crate::normalize::is_stopword(w) && !seen.contains(w)
                {
                    seen.push(w.clone());
                }
            }
            for w in &seen {
                *df.entry(w.clone()).or_insert(0) += 1;
            }
            per_doc.push(seen);
        }
        let n = prepared.len().max(1);
        let docs = prepared
            .into_iter()
            .zip(per_doc)
            .map(|(p, seen)| SummaryDoc {
                keywords: seen
                    .into_iter()
                    .map(|w| {
                        let weight = idf(n, df[&w]).max(0.05);
                        let code = cache.code(&w).to_string();
                        (w, code, weight)
                    })
                    .collect(),
                prepared: p,
            })
            .collect();
        Self { docs }
    }

    /// The prepared summary `i`.
    pub fn document(&self, i: usize) -> &PreparedText {
        &self.docs[i].prepared
    }

    /// Compares heard text (already tokenised) with summary `document`. A summary word counts as
    /// heard when the same word, or a word with the same phonetic code of at least three
    /// characters, occurs in the heard text.
    pub fn score(&self, heard: &Tokens, document: usize) -> SummaryScore {
        let doc = &self.docs[document];
        let mut cache = CodeCache::default();
        let heard_codes: Vec<String> = heard
            .words
            .iter()
            .map(|w| cache.code(w).to_string())
            .collect();
        let mut total = 0.0f32;
        let mut hit = 0.0f32;
        let mut summary_matched = vec![false; doc.prepared.len()];
        let mut heard_matched = vec![false; heard.len()];
        for (word, code, weight) in &doc.keywords {
            total += weight;
            let mut was_heard = false;
            for (j, w) in heard.words.iter().enumerate() {
                if w == word || (code.chars().count() >= 3 && &heard_codes[j] == code) {
                    heard_matched[j] = true;
                    was_heard = true;
                }
            }
            if was_heard {
                hit += weight;
                for (i, w) in doc.prepared.tokens.words.iter().enumerate() {
                    if w == word {
                        summary_matched[i] = true;
                    }
                }
            }
        }
        let coverage = if total > 0.0 { hit / total } else { 0.0 };
        let similarity = ((coverage - 0.1) / 0.5).clamp(0.0, 1.0) * SUMMARY_CAP;
        SummaryScore {
            similarity,
            coverage,
            summary_matched,
            heard_matched,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homophones_share_phonetic_codes() {
        for (a, b) in [
            ("there", "their"),
            ("smith", "smyth"),
            ("for", "four"),
            ("write", "right"),
            ("knight", "night"),
        ] {
            assert_eq!(phonetic_code(a), phonetic_code(b), "{a} / {b}");
        }
        assert_ne!(
            phonetic_code("transportation"),
            phonetic_code("transformation")
        );
    }

    #[test]
    fn bit_parallel_ratio_agrees_with_rapidfuzz() {
        let mut rng = crate::testutil::Rng::new(5);
        let alphabet: Vec<char> = "abcde fgé".chars().collect();
        let random = |rng: &mut crate::testutil::Rng, n: usize| -> Vec<char> {
            (0..n)
                .map(|_| alphabet[rng.below(alphabet.len())])
                .collect()
        };
        for _ in 0..500 {
            let (na, nb) = (rng.below(70), rng.below(70));
            let a = random(&mut rng, na);
            let b = random(&mut rng, nb);
            let expected = ratio_chars(&a, &b);
            let got = Pattern::new(&a).ratio(&b);
            assert!(
                (expected - got).abs() < 1e-5,
                "{a:?} {b:?} {expected} {got}"
            );
        }
        assert_eq!(Pattern::new(&[]).ratio(&[]), 1.0);
    }

    #[test]
    fn shingles_join_consecutive_codes() {
        let t = tokenize("their dog ran");
        let s = phonetic_shingles(&t, 2);
        assert_eq!(s.len(), 2);
        assert_eq!(s, phonetic_shingles(&tokenize("there dog ran"), 2));
        assert_eq!(phonetic_shingles(&tokenize("one"), 3).len(), 1);
        assert!(phonetic_shingles(&tokenize(""), 2).is_empty());
    }

    #[test]
    fn partial_ratio_finds_needle_inside_haystack() {
        assert_eq!(
            partial_ratio("magic number", "three is a magic number yes it is"),
            1.0
        );
        let fuzzy = partial_ratio("magik numbre", "three is a magic number yes it is");
        assert!(fuzzy > 0.7 && fuzzy < 1.0, "{fuzzy}");
        assert!(partial_ratio("zebra crossing", "three is a magic number") < 0.6);
        assert_eq!(partial_ratio("", "abc"), 0.0);
        // Order of arguments does not matter.
        assert_eq!(partial_ratio("a long haystack text", "hay"), 1.0);
    }

    #[test]
    fn partial_ratio_tries_stretches_cut_at_the_ends() {
        // Only the end of the needle survives at the start of the haystack. A full-length window
        // shares four of eight characters (0.5); the four-character stretch at the edge scores
        // 2 * 4 / (8 + 4).
        let r = partial_ratio("abcdefgh", "efghxxxxxxxx");
        assert!((r - 2.0 * 4.0 / 12.0).abs() < 1e-6, "{r}");
    }

    fn docs(texts: &[&str]) -> Vec<(String, String)> {
        texts
            .iter()
            .enumerate()
            .map(|(i, t)| (format!("d{i}"), t.to_string()))
            .collect()
    }

    #[test]
    fn idf_suppresses_lines_every_document_shares() {
        let theme = "la la la here comes the show ";
        let index = TfIdfIndex::build(&docs(&[
            &format!("{theme} the dragon guards a golden egg in the mountain"),
            &format!("{theme} a submarine sails under the arctic ice"),
        ]));
        let sims = index.similarities(theme);
        // The theme alone says nothing about which document it is.
        assert!((sims[0] - sims[1]).abs() < 0.05, "{sims:?}");
        let sims = index.similarities("the submarine sails under ice");
        assert!(sims[1] > sims[0] + 0.3, "{sims:?}");
    }

    #[test]
    fn unknown_words_weigh_like_rare_ones() {
        let index = TfIdfIndex::build(&docs(&["only one document here"]));
        let s = index.similarities("only one document here");
        assert!(s[0] > 0.99, "{s:?}");
        // A single-document index still scores partial overlap sensibly.
        let s = index.similarities("only one document here plus extra words");
        assert!(s[0] > 0.3 && s[0] < 0.99, "{s:?}");
    }

    #[test]
    fn phonetic_cosine_catches_homophones_that_words_miss() {
        let index = TfIdfIndex::build(&docs(&[
            "their knight rode right to the sea",
            "a robot builds a rocket from spare parts",
        ]));
        let q = index.query("there night road write two the see");
        let s = index.score(&q, 0);
        assert!(s.phonetic_cosine > 0.6, "{s:?}");
        assert!(s.phonetic_cosine > s.word_cosine + 0.3, "{s:?}");
    }

    #[test]
    fn phrase_coverage_survives_split_and_misspelt_words() {
        let index = TfIdfIndex::build(&docs(&[
            "everybody gather round the campfire tonight because grandpa owl has a story",
            "the robot builds a rocket from spare parts in the garage",
        ]));
        let q = index.query("every body gather around the camp fire tonite because grampa owl");
        let s = index.score(&q, 0);
        let other = index.score(&q, 1);
        assert!(s.phrase_coverage > 0.6, "{s:?}");
        assert!(other.phrase_coverage < 0.2, "{other:?}");
        let overlap = s.best_overlap.expect("overlap");
        assert!(overlap.ratio > 0.7);
    }

    #[test]
    fn dialogue_similarity_ranks_the_right_document() {
        let index = TfIdfIndex::build(&docs(&[
            "the tortoise said slow and steady wins the race while the hare slept under a tree",
            "the boy cried wolf wolf but nobody came because he had lied before",
        ]));
        let heard = "the tortoise said slow and steady wins the race while the hair slept";
        let right = dialogue_similarity(&index, 0, heard);
        let wrong = dialogue_similarity(&index, 1, heard);
        assert!(right > 0.6, "{right}");
        assert!(wrong < 0.2, "{wrong}");
    }

    #[test]
    fn empty_query_scores_zero() {
        let index = TfIdfIndex::build(&docs(&["some text"]));
        let s = index.score(&index.query(""), 0);
        assert_eq!(s.similarity, 0.0);
        assert!(s.best_overlap.is_none());
    }

    #[test]
    fn phrases_split_evenly() {
        assert_eq!(phrase_ranges(0), Vec::<Range<usize>>::new());
        assert_eq!(phrase_ranges(4), vec![0..4]);
        assert_eq!(phrase_ranges(6), vec![0..6]);
        assert_eq!(phrase_ranges(8), vec![0..8]);
        assert_eq!(phrase_ranges(9), vec![0..6, 6..9]);
    }

    #[test]
    fn summary_matches_distinctive_words() {
        let index = SummaryIndex::build(&[
            "Lucy visits the aquarium and meets a talking octopus named Barnaby.".into(),
            "Lucy builds a treehouse with her grandfather during a thunderstorm.".into(),
        ]);
        let heard = tokenize("look Barnaby the octopus is waving at us from the aquarium tank");
        let a = index.score(&heard, 0);
        let b = index.score(&heard, 1);
        assert!(a.similarity > b.similarity + 0.3, "{a:?} {b:?}");
        assert!(a.similarity <= SUMMARY_CAP);
        assert!(a.summary_matched.iter().any(|h| *h));
        assert!(a.heard_matched.iter().any(|h| *h));
    }
}
