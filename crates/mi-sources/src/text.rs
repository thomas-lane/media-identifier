//! Turning subtitle, lyrics and summary text into plain dialogue lines.
//!
//! The output keeps the original words and case, one cue (or lyric line) per line, so that the
//! Review screen can quote it. Tokenisation for matching happens in `mi-match`.

/// Decodes subtitle file bytes to text: a byte-order mark selects UTF-8 or UTF-16; otherwise
/// valid UTF-8 is used as is and anything else is read as Windows-1252, the usual encoding of
/// older Western subtitle files.
pub fn decode_bytes(bytes: &[u8]) -> String {
    if let Some((encoding, bom_len)) = encoding_rs::Encoding::for_bom(bytes) {
        let (text, _) = encoding.decode_without_bom_handling(&bytes[bom_len..]);
        return text.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned(),
    }
}

/// Converts SubRip text to dialogue lines: removes cue numbers, timing lines, HTML/ASS tags,
/// speaker labels (`JOHN:`), sound descriptions in brackets or parentheses (`[music]`,
/// `(laughs)`), music-note symbols and duplicate consecutive lines; joins multi-line cues with a
/// space and returns one cue per line. Accepts CRLF and a UTF-8 BOM.
///
/// Words between music notes are kept, because sung lines are often the best evidence for
/// musical shorts.
pub fn srt_to_dialogue(srt: &str) -> String {
    finish(timed_cues(srt).iter().map(|cue| clean_cue(cue)))
}

/// Same as [`srt_to_dialogue`] for WebVTT, ASS/SSA, LRC and plain text, chosen by content
/// sniffing: `WEBVTT` header → WebVTT; `[Events]`/`Dialogue:` → ASS/SSA; a `-->` timing line →
/// SubRip; `[mm:ss` timestamps → LRC lyrics; anything else is treated as plain lines.
pub fn subtitle_to_dialogue(content: &str) -> String {
    let content = content.trim_start_matches('\u{feff}');
    let head = content.trim_start();
    if head.starts_with("WEBVTT") {
        srt_to_dialogue(content)
    } else if content.contains("[Events]") || content.contains("\nDialogue:") {
        ass_to_dialogue(content)
    } else if content.contains("-->") {
        srt_to_dialogue(content)
    } else if looks_like_lrc(content) {
        lrc_to_lyrics(content)
    } else {
        finish(content.lines().map(clean_cue))
    }
}

/// Converts ASS/SSA subtitles: reads `Dialogue:` lines of the `[Events]` section in start-time
/// order, takes the text field (the last one in the `Format:` line), removes `{...}` override
/// blocks and turns `\N`, `\n` and `\h` into spaces, then cleans each line like
/// [`srt_to_dialogue`].
pub fn ass_to_dialogue(content: &str) -> String {
    let content = content.trim_start_matches('\u{feff}');
    let mut fields: Vec<String> = [
        "layer", "start", "end", "style", "name", "marginl", "marginr", "marginv", "effect", "text",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    let mut in_events = false;
    let mut lines: Vec<(f64, usize, String)> = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_events = line.eq_ignore_ascii_case("[events]");
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Format:") {
            fields = rest
                .split(',')
                .map(|f| f.trim().to_ascii_lowercase())
                .collect();
            continue;
        }
        let Some(rest) = line.strip_prefix("Dialogue:") else {
            continue;
        };
        let parts: Vec<&str> = rest.splitn(fields.len(), ',').collect();
        if parts.len() < fields.len() {
            continue;
        }
        let start = fields
            .iter()
            .position(|f| f == "start")
            .and_then(|i| parse_ass_time(parts[i].trim()))
            .unwrap_or(f64::MAX);
        let text = parts[fields.len() - 1]
            .replace("\\N", " ")
            .replace("\\n", " ")
            .replace("\\h", " ");
        let order = lines.len();
        lines.push((start, order, remove_delimited(&text, '{', '}', "")));
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    finish(lines.iter().map(|(_, _, text)| clean_cue(text)))
}

/// Converts LRC lyrics to plain lines: removes `[mm:ss.xx]` line timestamps (several per line
/// are allowed), `<mm:ss.xx>` word timestamps and `[ar:...]`-style metadata tags; drops empty
/// lines.
pub fn lrc_to_lyrics(lrc: &str) -> String {
    let lines = lrc.lines().map(|line| {
        let mut rest = line.trim();
        while rest.starts_with('[') {
            match rest.find(']') {
                Some(end) => rest = rest[end + 1..].trim_start(),
                None => break,
            }
        }
        remove_delimited(rest, '<', '>', "")
    });
    plain_lyrics(&lines.collect::<Vec<_>>().join("\n"))
}

/// Cleans plain lyrics: trims lines, removes section labels in square brackets (`[Chorus]`),
/// drops empty lines. Parentheses are kept, because in lyrics they usually hold sung words.
pub fn plain_lyrics(text: &str) -> String {
    let lines = text.lines().map(|l| {
        let l = remove_delimited(l, '[', ']', " ");
        collapse_spaces(&l)
    });
    lines
        .filter(|l| l.chars().any(char::is_alphanumeric))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Plain text from an HTML summary (TVmaze returns `<p>...</p>`): removes tags, decodes common
/// entities and collapses whitespace.
pub fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(len) = rest[start..].find('>') else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let tag = rest[start + 1..start + len]
            .trim_start_matches('/')
            .to_ascii_lowercase();
        let name: String = tag
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect();
        // Block-level tags separate words; inline tags such as <i> do not.
        if matches!(
            name.as_str(),
            "p" | "br" | "div" | "li" | "ul" | "ol" | "h1" | "h2" | "h3"
        ) {
            out.push(' ');
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    tidy(&decode_entities(&out))
}

/// Parses LRC synced lyrics into `(seconds, line)` pairs, in time order. Lines with several
/// timestamps appear once per timestamp; metadata tags are skipped.
pub fn lrc_lines(lrc: &str) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    for line in lrc.lines() {
        let mut rest = line.trim();
        let mut times = Vec::new();
        while rest.starts_with('[') {
            let Some(end) = rest.find(']') else { break };
            if let Some(t) = parse_lrc_time(&rest[1..end]) {
                times.push(t);
            }
            rest = rest[end + 1..].trim_start();
        }
        let text = collapse_spaces(&remove_delimited(rest, '<', '>', ""));
        for t in times {
            out.push((t, text.clone()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

fn looks_like_lrc(content: &str) -> bool {
    content
        .lines()
        .filter(|l| {
            let l = l.trim();
            l.starts_with('[')
                && l.find(']')
                    .is_some_and(|end| parse_lrc_time(&l[1..end]).is_some())
        })
        .take(2)
        .count()
        >= 2
}

fn parse_lrc_time(tag: &str) -> Option<f64> {
    let (m, s) = tag.split_once(':')?;
    let minutes: f64 = m.trim().parse().ok()?;
    let seconds: f64 = s.trim().parse().ok()?;
    Some(minutes * 60.0 + seconds)
}

fn parse_ass_time(t: &str) -> Option<f64> {
    let mut parts = t.split(':');
    let h: f64 = parts.next()?.trim().parse().ok()?;
    let m: f64 = parts.next()?.trim().parse().ok()?;
    let s: f64 = parts.next()?.trim().parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

/// Splits SubRip/WebVTT text into the text lines of each timed cue.
///
/// A cue starts at a line containing `-->` and its text runs until the next blank line. Lines
/// outside cues (cue numbers, WebVTT identifiers, `NOTE`/`STYLE` blocks, the `WEBVTT` header)
/// are ignored. When a file omits the blank line between cues, a number-only line just before
/// the next timing line is taken as that cue's number and dropped.
fn timed_cues(content: &str) -> Vec<String> {
    let content = content.trim_start_matches('\u{feff}');
    let mut cues: Vec<Vec<String>> = Vec::new();
    let mut in_cue = false;
    for raw in content.lines() {
        let line = raw.trim();
        if line.contains("-->") {
            if in_cue
                && let Some(current) = cues.last_mut()
                && current
                    .last()
                    .is_some_and(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_digit()))
            {
                current.pop();
            }
            cues.push(Vec::new());
            in_cue = true;
        } else if line.is_empty() {
            in_cue = false;
        } else if in_cue && let Some(current) = cues.last_mut() {
            current.push(line.to_owned());
        }
    }
    cues.into_iter()
        .map(|lines| {
            lines
                .iter()
                .map(|l| strip_line_prefixes(&remove_tags(l)))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

/// Removes `<...>` markup and `{...}` ASS override blocks.
fn remove_tags(line: &str) -> String {
    remove_delimited(&remove_delimited(line, '<', '>', ""), '{', '}', "")
}

/// Removes a leading dialogue dash and a leading upper-case speaker label (`JOHN:`, `DR. NO:`).
fn strip_line_prefixes(line: &str) -> String {
    let mut s = line.trim();
    s = s.trim_start_matches(['-', '–', '—']).trim_start();
    if let Some((label, rest)) = s.split_once(':') {
        let letters = label.chars().filter(|c| c.is_alphabetic()).count();
        let is_label = letters >= 2
            && label.chars().count() <= 30
            && label
                .chars()
                .all(|c| c.is_uppercase() || c.is_ascii_digit() || " .'-()#&".contains(c))
            && !rest.starts_with("//");
        if is_label {
            s = rest.trim_start();
        }
    }
    s.to_owned()
}

/// Final per-cue cleanup: sound descriptions, music notes, entity decoding, whitespace.
fn clean_cue(cue: &str) -> String {
    let s = decode_entities(cue);
    let s = remove_delimited(&s, '[', ']', " ");
    let s = remove_delimited(&s, '(', ')', " ");
    let s: String = s
        .chars()
        .map(|c| if "♪♫♬♩".contains(c) { ' ' } else { c })
        .collect();
    tidy(&strip_line_prefixes(&s))
}

/// Joins cleaned cues, dropping cues without letters or digits and consecutive duplicates.
fn finish(cues: impl Iterator<Item = String>) -> String {
    let mut out: Vec<String> = Vec::new();
    for cue in cues {
        if !cue.chars().any(char::is_alphanumeric) {
            continue;
        }
        if out
            .last()
            .is_some_and(|prev| prev.to_lowercase() == cue.to_lowercase())
        {
            continue;
        }
        out.push(cue);
    }
    out.join("\n")
}

/// Removes every `open ... close` span (unbalanced openers are kept as text).
fn remove_delimited(s: &str, open: char, close: char, replacement: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    let mut pending = String::new();
    for c in s.chars() {
        if c == open {
            depth += 1;
            pending.push(c);
        } else if c == close && depth > 0 {
            depth -= 1;
            if depth == 0 {
                pending.clear();
                out.push_str(replacement);
            } else {
                pending.push(c);
            }
        } else if depth > 0 {
            pending.push(c);
        } else {
            out.push(c);
        }
    }
    out.push_str(&pending);
    out
}

/// Collapses whitespace and removes spaces left before closing punctuation by removed spans.
fn tidy(s: &str) -> String {
    let collapsed = collapse_spaces(s);
    let mut out = String::with_capacity(collapsed.len());
    let mut chars = collapsed.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ' ' && chars.peek().is_some_and(|n| ",.!?;:)".contains(*n)) {
            continue;
        }
        out.push(c);
    }
    out
}

fn collapse_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail[..tail.len().min(12)].find(';') else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let name = &tail[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "hellip" => Some('…'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "rsquo" | "lsquo" => Some('\''),
            "rdquo" | "ldquo" => Some('"'),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_cleanup_keeps_words_and_drops_markup() {
        let srt = "\u{feff}1\r\n00:00:01,000 --> 00:00:03,000\r\n<i>JOHN: Hello there,</i>\r\nfriend.\r\n\r\n\
                   2\r\n00:00:03,500 --> 00:00:04,000\r\n[MUSIC PLAYING]\r\n\r\n\
                   3\r\n00:00:04,000 --> 00:00:06,000\r\n♪ Conjunction Junction, ♪\r\n♪ what's your function? ♪\r\n\r\n\
                   4\r\n00:00:06,000 --> 00:00:07,000\r\n♪ Conjunction Junction, ♪\r\n♪ what's your function? ♪\r\n\r\n\
                   5\r\n00:00:07,000 --> 00:00:09,000\r\n- (laughs) Sure.\r\n- {\\an8}Fine &amp; dandy.\r\n";
        assert_eq!(
            srt_to_dialogue(srt),
            "Hello there, friend.\nConjunction Junction, what's your function?\nSure. Fine & dandy."
        );
    }

    #[test]
    fn srt_without_blank_lines_between_cues_still_splits() {
        let srt = "1\n00:00:01,000 --> 00:00:02,000\nOne\n2\n00:00:02,000 --> 00:00:03,000\nTwo\n";
        assert_eq!(srt_to_dialogue(srt), "One\nTwo");
    }

    #[test]
    fn lowercase_colons_are_not_speaker_labels() {
        let srt = "1\n00:00:01,000 --> 00:00:02,000\nThe time is: now\n";
        assert_eq!(srt_to_dialogue(srt), "The time is: now");
    }

    #[test]
    fn webvtt_is_sniffed_and_cleaned() {
        let vtt = "WEBVTT\nKind: captions\n\nNOTE a comment\nwith two lines\n\nintro\n00:00:01.000 --> 00:00:02.000 align:start\n<v Narrator>Three is a <c.yellow>magic</c> number.\n\n00:00:03.000 --> 00:00:04.000\nYes it is.\n";
        assert_eq!(
            subtitle_to_dialogue(vtt),
            "Three is a magic number.\nYes it is."
        );
    }

    #[test]
    fn ass_dialogue_is_read_in_time_order_with_overrides_removed() {
        let ass = "[Script Info]\nTitle: x\n\n[V4+ Styles]\nFormat: Name, Fontname\n\n[Events]\n\
                   Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
                   Dialogue: 0,0:00:05.00,0:00:06.00,Default,,0,0,0,,Second line, with comma\n\
                   Comment: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,not shown\n\
                   Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\i1}First{\\i0}\\Nline\n";
        assert_eq!(
            subtitle_to_dialogue(ass),
            "First line\nSecond line, with comma"
        );
    }

    #[test]
    fn lrc_timestamps_and_tags_are_removed() {
        let lrc = "[ar:Schoolhouse Rock]\n[00:01.00]Conjunction Junction,\n[00:03.50][01:10.00]what's your <00:04.00>function?\n[00:05.00]\n";
        assert_eq!(
            subtitle_to_dialogue(lrc),
            "Conjunction Junction,\nwhat's your function?"
        );
        assert_eq!(
            lrc_lines(lrc),
            vec![
                (1.0, "Conjunction Junction,".to_owned()),
                (3.5, "what's your function?".to_owned()),
                (5.0, String::new()),
                (70.0, "what's your function?".to_owned()),
            ]
        );
    }

    #[test]
    fn plain_lyrics_keep_parentheses_but_drop_section_labels() {
        assert_eq!(
            plain_lyrics("[Chorus]\n  Three is a magic number  \n\n(Yes it is)\n"),
            "Three is a magic number\n(Yes it is)"
        );
    }

    #[test]
    fn html_summaries_become_plain_text() {
        assert_eq!(
            strip_html("<p>Mal &amp; crew rob a <i>train</i>.&nbsp;It&#39;s a job.</p>"),
            "Mal & crew rob a train. It's a job."
        );
    }

    #[test]
    fn bytes_are_decoded_by_bom_then_utf8_then_windows_1252() {
        assert_eq!(decode_bytes("caf\u{e9}".as_bytes()), "café");
        assert_eq!(decode_bytes(b"caf\xe9"), "café");
        assert_eq!(decode_bytes(b"\xef\xbb\xbfhi"), "hi");
        assert_eq!(decode_bytes(b"\xff\xfeh\x00i\x00"), "hi");
    }
}
