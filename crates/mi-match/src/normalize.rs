//! Turning text into comparable word tokens.
//!
//! Speech recognition and subtitles write the same words differently: "3" or "three",
//! "Mr." or "mister", "what's" or "whats", "café" or "cafe". Normalisation removes those
//! differences before any comparison, so they never count as mismatches:
//!
//! - letters are lower-cased and common Latin accents removed;
//! - apostrophes inside a word are dropped ("what's" becomes `whats`);
//! - every other character that is not a letter or digit separates words, so hyphenated words
//!   split ("seventy-three" becomes `seventy three`);
//! - numbers are spelled as words ("3" becomes `three`, "1973" becomes `nineteen seventy three`,
//!   "21st" becomes `twenty first`), because Whisper writes numbers either way;
//! - `&` becomes `and`, `%` becomes `percent`, and a few title abbreviations are spelled out.
//!
//! Each token keeps the byte range of the original text it came from, so the Review screen can
//! show the original words with matched ones highlighted.

use std::ops::Range;

/// Normalised word tokens of a text, each with the byte range of the original text it came from.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tokens {
    /// Normalised words, in order.
    pub words: Vec<String>,
    /// For each word, the byte range in the original text. Several words share a range when one
    /// original word expanded into several ("1973" into `nineteen seventy three`).
    pub spans: Vec<Range<usize>>,
}

impl Tokens {
    /// Number of tokens.
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// True when there are no tokens.
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    fn push(&mut self, word: impl Into<String>, span: Range<usize>) {
        self.words.push(word.into());
        self.spans.push(span);
    }
}

/// Splits text into normalised word tokens (see the module documentation for the rules).
pub fn tokenize(text: &str) -> Tokens {
    let mut out = Tokens::default();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let end_of = |i: usize| -> usize { chars.get(i).map_or(text.len(), |(b, _)| *b) };
    let mut i = 0;
    while i < chars.len() {
        let (start, c) = chars[i];
        if c.is_ascii_digit() {
            // A number: digits, thousands separators ("1,000") and one decimal point ("3.5").
            let mut digits = String::new();
            let mut decimals = String::new();
            let mut j = i;
            while j < chars.len() {
                let d = chars[j].1;
                if d.is_ascii_digit() {
                    digits.push(d);
                    j += 1;
                } else if d == ','
                    && !digits.is_empty()
                    && (1..=3).all(|k| chars.get(j + k).is_some_and(|x| x.1.is_ascii_digit()))
                    && !chars.get(j + 4).is_some_and(|x| x.1.is_ascii_digit())
                {
                    j += 1;
                } else {
                    break;
                }
            }
            if j + 1 < chars.len() && chars[j].1 == '.' && chars[j + 1].1.is_ascii_digit() {
                j += 1;
                while j < chars.len() && chars[j].1.is_ascii_digit() {
                    decimals.push(chars[j].1);
                    j += 1;
                }
            }
            // Ordinal suffix: 1st, 2nd, 3rd, 4th.
            let mut ordinal = false;
            if j < chars.len() {
                let suffix: String = chars[j..chars.len().min(j + 2)]
                    .iter()
                    .map(|x| x.1.to_ascii_lowercase())
                    .collect();
                let after_alpha = chars.get(j + 2).is_some_and(|x| x.1.is_alphanumeric());
                if decimals.is_empty()
                    && matches!(suffix.as_str(), "st" | "nd" | "rd" | "th")
                    && !after_alpha
                {
                    ordinal = true;
                    j += 2;
                }
            }
            let span = start..end_of(j);
            let mut words = number_words(&digits);
            if ordinal && let Some(last) = words.pop() {
                words.push(ordinal_word(&last));
            }
            if !decimals.is_empty() {
                words.push("point".into());
                words.extend(decimals.chars().map(|d| digit_word(d).to_string()));
            }
            for w in words {
                out.push(w, span.clone());
            }
            i = j;
        } else if c.is_alphabetic() {
            let mut word = String::new();
            let mut j = i;
            while j < chars.len() {
                let d = chars[j].1;
                if d.is_alphabetic() {
                    for l in d.to_lowercase() {
                        fold_accent(l, &mut word);
                    }
                    j += 1;
                } else if is_apostrophe(d)
                    && chars.get(j + 1).is_some_and(|x| x.1.is_alphabetic())
                    && !word.is_empty()
                {
                    j += 1;
                } else {
                    break;
                }
            }
            let span = start..end_of(j);
            match expand_abbreviation(&word) {
                Some(expansion) => {
                    for w in expansion {
                        out.push(*w, span.clone());
                    }
                }
                None => out.push(word, span),
            }
            i = j;
        } else {
            match c {
                '&' => out.push("and", start..end_of(i + 1)),
                '%' => out.push("percent", start..end_of(i + 1)),
                _ => {}
            }
            i += 1;
        }
    }
    out
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}' | '\u{2018}' | '\u{02bc}' | '`')
}

/// Appends `c` to `out` with common Latin diacritics removed.
fn fold_accent(c: char, out: &mut String) {
    let base = match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
        'ç' | 'č' | 'ć' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' => 'u',
        'ý' | 'ÿ' => 'y',
        'š' | 'ś' => 's',
        'ž' | 'ź' | 'ż' => 'z',
        'ł' => 'l',
        'ř' => 'r',
        'ß' => {
            out.push_str("ss");
            return;
        }
        'æ' => {
            out.push_str("ae");
            return;
        }
        'œ' => {
            out.push_str("oe");
            return;
        }
        other => other,
    };
    out.push(base);
}

fn expand_abbreviation(word: &str) -> Option<&'static [&'static str]> {
    Some(match word {
        "mr" => &["mister"],
        "mrs" => &["missus"],
        "dr" => &["doctor"],
        "ok" => &["okay"],
        "vs" => &["versus"],
        "tv" => &["t", "v"],
        _ => return None,
    })
}

const ONES: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

fn digit_word(d: char) -> &'static str {
    ONES[d.to_digit(10).unwrap_or(0) as usize]
}

/// Words for a run of ASCII digits. Four-digit numbers that read as years ("1973", "2015") are
/// spoken in pairs; numbers with a leading zero ("007") and numbers too long to be quantities are
/// spelled digit by digit.
fn number_words(digits: &str) -> Vec<String> {
    let spelled = || digits.chars().map(|d| digit_word(d).to_string()).collect();
    if digits.len() > 1 && digits.starts_with('0') || digits.len() > 12 {
        return spelled();
    }
    let Ok(n) = digits.parse::<u64>() else {
        return spelled();
    };
    if digits.len() == 4 && ((1100..=1999).contains(&n) || (2010..=2099).contains(&n)) {
        let (hi, lo) = (n / 100, n % 100);
        let mut words = cardinal(hi);
        match lo {
            0 => words.push("hundred".into()),
            1..=9 => {
                words.push("oh".into());
                words.extend(cardinal(lo));
            }
            _ => words.extend(cardinal(lo)),
        }
        return words;
    }
    cardinal(n)
}

/// English cardinal words for `n`, without "and" ("one hundred five").
fn cardinal(n: u64) -> Vec<String> {
    if n < 20 {
        return vec![ONES[n as usize].to_string()];
    }
    if n < 100 {
        let mut words = vec![TENS[(n / 10) as usize].to_string()];
        if !n.is_multiple_of(10) {
            words.push(ONES[(n % 10) as usize].to_string());
        }
        return words;
    }
    if n < 1000 {
        let mut words = cardinal(n / 100);
        words.push("hundred".into());
        if !n.is_multiple_of(100) {
            words.extend(cardinal(n % 100));
        }
        return words;
    }
    for (scale, name) in [
        (1_000_000_000u64, "billion"),
        (1_000_000, "million"),
        (1_000, "thousand"),
    ] {
        if n >= scale {
            let mut words = cardinal(n / scale);
            words.push(name.into());
            if !n.is_multiple_of(scale) {
                words.extend(cardinal(n % scale));
            }
            return words;
        }
    }
    unreachable!("all values below one trillion are covered")
}

/// The ordinal form of a cardinal number word ("one" to "first", "twenty" to "twentieth").
fn ordinal_word(word: &str) -> String {
    match word {
        "one" => "first".into(),
        "two" => "second".into(),
        "three" => "third".into(),
        "five" => "fifth".into(),
        "eight" => "eighth".into(),
        "nine" => "ninth".into(),
        "twelve" => "twelfth".into(),
        w if w.ends_with('y') => format!("{}ieth", &w[..w.len() - 1]),
        w => format!("{w}th"),
    }
}

/// Sung syllables and hesitation sounds that speech recognition writes over music and pauses
/// ("la la la", "oh", "mm"). They say nothing about which episode a file is.
pub(crate) fn is_filler(word: &str) -> bool {
    matches!(
        word,
        "la" | "na"
            | "da"
            | "doo"
            | "ba"
            | "oh"
            | "ooh"
            | "ah"
            | "aah"
            | "uh"
            | "um"
            | "umm"
            | "hmm"
            | "mm"
            | "mmm"
            | "huh"
            | "hey"
            | "yeah"
            | "whoa"
            | "wow"
            | "ha"
            | "hoo"
    )
}

/// Words that carry content: neither common function words nor fillers.
pub(crate) fn is_content_word(word: &str) -> bool {
    !is_stopword(word) && !is_filler(word)
}

/// Very common English words. They carry little evidence on their own: a title made only of
/// them ("The End") is weighted down, and summary matching ignores them.
pub(crate) fn is_stopword(word: &str) -> bool {
    matches!(
        word,
        "a" | "about"
            | "after"
            | "all"
            | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "been"
            | "but"
            | "by"
            | "can"
            | "do"
            | "for"
            | "from"
            | "get"
            | "go"
            | "has"
            | "have"
            | "he"
            | "her"
            | "him"
            | "his"
            | "how"
            | "i"
            | "if"
            | "in"
            | "into"
            | "is"
            | "it"
            | "its"
            | "just"
            | "me"
            | "my"
            | "no"
            | "not"
            | "of"
            | "on"
            | "one"
            | "or"
            | "our"
            | "out"
            | "part"
            | "she"
            | "so"
            | "that"
            | "the"
            | "their"
            | "them"
            | "then"
            | "there"
            | "they"
            | "this"
            | "to"
            | "up"
            | "was"
            | "we"
            | "were"
            | "what"
            | "when"
            | "who"
            | "will"
            | "with"
            | "you"
            | "your"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        tokenize(text).words
    }

    #[test]
    fn lowercases_and_strips_punctuation() {
        assert_eq!(
            words("Conjunction Junction, what's your FUNCTION?"),
            ["conjunction", "junction", "whats", "your", "function"]
        );
    }

    #[test]
    fn curly_apostrophes_and_hyphens() {
        assert_eq!(words("Don\u{2019}t stop"), ["dont", "stop"]);
        assert_eq!(words("seventy-three"), ["seventy", "three"]);
        assert_eq!(words("'quoted'"), ["quoted"]);
    }

    #[test]
    fn spells_numbers_as_words() {
        assert_eq!(words("3"), ["three"]);
        assert_eq!(words("42"), ["forty", "two"]);
        assert_eq!(words("105"), ["one", "hundred", "five"]);
        assert_eq!(words("1,000"), ["one", "thousand"]);
        assert_eq!(words("3.5"), ["three", "point", "five"]);
        assert_eq!(words("007"), ["zero", "zero", "seven"]);
        assert_eq!(
            words("2,500,000"),
            ["two", "million", "five", "hundred", "thousand"]
        );
    }

    #[test]
    fn years_are_read_in_pairs() {
        assert_eq!(words("1973"), ["nineteen", "seventy", "three"]);
        assert_eq!(words("1900"), ["nineteen", "hundred"]);
        assert_eq!(words("1905"), ["nineteen", "oh", "five"]);
        assert_eq!(words("2015"), ["twenty", "fifteen"]);
        assert_eq!(words("2005"), ["two", "thousand", "five"]);
    }

    #[test]
    fn ordinals() {
        assert_eq!(words("1st"), ["first"]);
        assert_eq!(words("22nd"), ["twenty", "second"]);
        assert_eq!(words("20th"), ["twentieth"]);
        assert_eq!(words("4th of July"), ["fourth", "of", "july"]);
    }

    #[test]
    fn digits_and_words_normalise_the_same() {
        assert_eq!(
            words("Three is a magic number"),
            words("3 is a magic number")
        );
    }

    #[test]
    fn symbols_accents_and_abbreviations() {
        assert_eq!(words("Mr. Smith & me"), ["mister", "smith", "and", "me"]);
        assert_eq!(words("100%"), ["one", "hundred", "percent"]);
        assert_eq!(words("Café Noël"), ["cafe", "noel"]);
        assert_eq!(words("Straße"), ["strasse"]);
    }

    #[test]
    fn spans_point_at_original_words() {
        let text = "I'm 21, okay?";
        let t = tokenize(text);
        assert_eq!(t.words, ["im", "twenty", "one", "okay"]);
        assert_eq!(&text[t.spans[0].clone()], "I'm");
        assert_eq!(&text[t.spans[1].clone()], "21");
        assert_eq!(t.spans[1], t.spans[2]);
        assert_eq!(&text[t.spans[3].clone()], "okay");
    }

    #[test]
    fn empty_and_punctuation_only() {
        assert!(tokenize("").is_empty());
        assert!(tokenize(" ... !!! -- ").is_empty());
    }
}
