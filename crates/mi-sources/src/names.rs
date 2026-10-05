//! Comparing titles and reading episode numbers from file names.

/// Lower-case words of `s`: letters and digits only, `&` read as `and`, accents kept.
/// `"Schoolhouse Rock!"` → `"schoolhouse rock"`.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '&' {
            out.push_str(" and ");
        } else if c == '\'' || c == '’' {
            // "Don't" and "Dont" compare equal.
        } else if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// [`normalize`] without spaces, so `"School House Rock"` and `"Schoolhouse Rock"` compare equal.
pub fn squash(s: &str) -> String {
    normalize(s).replace(' ', "")
}

/// Whether two titles name the same thing after [`squash`].
pub fn same_title(a: &str, b: &str) -> bool {
    let (a, b) = (squash(a), squash(b));
    !a.is_empty() && a == b
}

/// Whether the words of `needle` occur as consecutive whole words in `haystack` (both
/// normalised). `"Conjunction Junction"` is in `"Grammar Rock - Conjunction Junction"`, but
/// `"Zero"` is not in `"Zeros and Ones"`.
pub fn contains_words(haystack: &str, needle: &str) -> bool {
    let hay = normalize(haystack);
    let needle = normalize(needle);
    if needle.is_empty() {
        return false;
    }
    format!(" {hay} ").contains(&format!(" {needle} "))
}

/// Whether `name` refers to the show `show`: equal or containing it after [`squash`]
/// (`"Schoolhouse Rock: Grammar Rock"` refers to `"Schoolhouse Rock!"`).
pub fn refers_to(name: &str, show: &str) -> bool {
    let (name, show) = (squash(name), squash(show));
    !show.is_empty() && name.contains(&show)
}

/// Season and episode from a file name: `S01E02`, `s1e2`, `S01.E02`, `1x02`, or
/// `Season 1 Episode 2`. Multi-episode markers (`S01E01E02`, `S01E01-E02`) give the first.
pub fn episode_marker(name: &str) -> Option<(u32, u32)> {
    let lower = name.to_lowercase();
    let b = lower.as_bytes();
    for i in 0..b.len() {
        // S01E02 / S01.E02 / S01_E02
        if b[i] == b's'
            && (i == 0 || !b[i - 1].is_ascii_alphabetic())
            && let Some((season, j)) = digits(b, i + 1)
        {
            let mut k = j;
            if k < b.len() && matches!(b[k], b'.' | b'_' | b' ' | b'-') {
                k += 1;
            }
            if k < b.len()
                && b[k] == b'e'
                && let Some((episode, _)) = digits(b, k + 1)
            {
                return Some((season, episode));
            }
        }
        // 1x02
        if b[i].is_ascii_digit()
            && (i == 0 || !b[i - 1].is_ascii_alphanumeric())
            && let Some((season, j)) = digits(b, i)
            && j < b.len()
            && b[j] == b'x'
            && let Some((episode, end)) = digits(b, j + 1)
            && (end == b.len() || !b[end].is_ascii_alphanumeric())
            && end - (j + 1) >= 2
        {
            return Some((season, episode));
        }
    }
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    for w in words.windows(4) {
        if w[0] == "season"
            && matches!(w[2], "episode" | "ep")
            && let (Ok(s), Ok(e)) = (w[1].parse(), w[3].parse())
        {
            return Some((s, e));
        }
    }
    None
}

/// A run of 1-3 ASCII digits starting at `start`: its value and the index after it.
fn digits(b: &[u8], start: usize) -> Option<(u32, usize)> {
    let mut end = start;
    while end < b.len() && b[end].is_ascii_digit() && end - start < 4 {
        end += 1;
    }
    if end == start || end - start > 3 {
        return None;
    }
    std::str::from_utf8(&b[start..end])
        .ok()?
        .parse()
        .ok()
        .map(|v| (v, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_and_squash() {
        assert_eq!(normalize("Schoolhouse Rock!"), "schoolhouse rock");
        assert_eq!(
            normalize("Don't Be a Carbon Sasquatch"),
            "dont be a carbon sasquatch"
        );
        assert_eq!(normalize("Rock & Roll"), "rock and roll");
        assert!(same_title("School House Rock", "Schoolhouse Rock!"));
        assert!(!same_title("", ""));
    }

    #[test]
    fn whole_word_containment() {
        assert!(contains_words(
            "Grammar Rock - Conjunction Junction",
            "Conjunction Junction"
        ));
        assert!(!contains_words("Zeros and Ones", "Zero"));
        assert!(refers_to(
            "Schoolhouse Rock: Grammar Rock",
            "Schoolhouse Rock!"
        ));
        assert!(refers_to("School House Rock", "Schoolhouse Rock!"));
        assert!(!refers_to("Couch", "Schoolhouse Rock!"));
    }

    #[test]
    fn episode_markers() {
        assert_eq!(episode_marker("Show.S01E02.720p.srt"), Some((1, 2)));
        assert_eq!(episode_marker("show s3e12 title"), Some((3, 12)));
        assert_eq!(episode_marker("Show.S02.E05.srt"), Some((2, 5)));
        assert_eq!(episode_marker("Show - 1x02 - Title.srt"), Some((1, 2)));
        assert_eq!(episode_marker("Show S01E01E02.srt"), Some((1, 1)));
        assert_eq!(episode_marker("Season 4 Episode 7.srt"), Some((4, 7)));
        assert_eq!(episode_marker("Show 1920x1080.srt"), None);
        assert_eq!(episode_marker("Conjunction Junction.srt"), None);
        assert_eq!(episode_marker("Seasons.srt"), None);
    }
}
