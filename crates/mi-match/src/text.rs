//! Fuzzy text similarity that tolerates misheard words.

/// A tokenised, lower-cased text with punctuation removed and numbers spelled as digits.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tokens(pub Vec<String>);

/// Splits text into normalised word tokens.
pub fn tokenize(text: &str) -> Tokens {
    let _ = text;
    todo!("match module: tokenizer")
}

/// Double Metaphone codes of each token, joined into shingles of `n` codes, so words that sound
/// alike ("Sampson"/"Samson", "there"/"their") compare equal.
pub fn phonetic_shingles(tokens: &Tokens, n: usize) -> Vec<String> {
    let _ = (tokens, n);
    todo!("match module: phonetic shingles")
}

/// TF-IDF index over the reference texts of all candidate episodes, with word n-grams (1-3) and
/// phonetic shingles as terms. Inverse document frequencies come from the candidate episodes
/// themselves, so lines every episode shares (theme songs, credits) weigh little.
#[derive(Debug, Clone, Default)]
pub struct TfIdfIndex {
    /// Document ids in insertion order.
    pub documents: Vec<String>,
}

impl TfIdfIndex {
    /// Builds the index from `(document id, text)` pairs.
    pub fn build(documents: &[(String, String)]) -> Self {
        let _ = documents;
        todo!("match module: TF-IDF index")
    }

    /// Cosine similarity of `query` to every document, in document order, each in `0.0..=1.0`.
    pub fn similarities(&self, query: &str) -> Vec<f32> {
        let _ = query;
        todo!("match module: cosine similarity")
    }
}

/// Best fuzzy similarity (`0.0..=1.0`) of `needle` against any equally long stretch of
/// `haystack`, at the character level (RapidFuzz partial ratio).
pub fn partial_ratio(needle: &str, haystack: &str) -> f32 {
    let _ = (needle, haystack);
    todo!("match module: partial ratio")
}

/// Combined dialogue similarity of a transcript to one reference text, `0.0..=1.0`, from the
/// TF-IDF, phonetic and partial-match components.
pub fn dialogue_similarity(index: &TfIdfIndex, document: usize, transcript: &str) -> f32 {
    let _ = (index, document, transcript);
    todo!("match module: dialogue similarity")
}
