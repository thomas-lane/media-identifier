//! File and folder names.
//!
//! Every scheme is a template of folder and file components. Each component is rendered from the
//! show and episode, then passed through [`sanitize_component`], so a name is valid on both macOS
//! and Windows whatever the provider's titles contain.

use std::path::PathBuf;

use mi_types::{Episode, NamingScheme, Show};

use crate::RenameError;

/// The longest file or folder name produced, in UTF-8 bytes.
///
/// Both macOS (APFS) and Windows (NTFS) allow 255 characters per name. 200 bytes leaves room for
/// macOS storing accented letters decomposed (one letter becomes two code points) and keeps full
/// paths short enough for players and file managers that still assume 260-character paths.
pub const MAX_COMPONENT_BYTES: usize = 200;

/// The Jellyfin/Plex template (`NamingScheme::JellyfinPlex`).
pub const JELLYFIN_PLEX_TEMPLATE: &str = "{show} ({year})/Season {season:02}/{show} ({year}) - S{season:02}E{episode:02} - {title}.{ext}";

/// The Kodi template (`NamingScheme::Kodi`).
pub const KODI_TEMPLATE: &str =
    "{show} ({year})/Season {season:02}/{show} S{season:02}E{episode:02} - {title}.{ext}";

/// The new path of an episode file relative to the destination root.
///
/// - `JellyfinPlex`: `<Show> (<Year>)/Season NN/<Show> (<Year>) - SNNEMM - <Title>.<ext>`
/// - `Kodi`: `<Show> (<Year>)/Season NN/<Show> SNNEMM - <Title>.<ext>`
/// - `Custom`: the user's template. Placeholders: `{show}`, `{year}`, `{season}`, `{episode}`,
///   `{title}`, `{ext}`; `{season:02}` and `{episode:02}` pad with zeros to the given width (1-9).
///   `/` or `\` separates folders. The last component must contain `{ext}`.
///
/// When the year is unknown, `({year})` and `[{year}]` are removed with their brackets. Specials
/// (season 0) go to `Season 00`. An empty title becomes `Episode <number>`. `ext` is the original
/// extension, with or without its dot. Every component is passed through
/// [`sanitize_component`]; a file name longer than [`MAX_COMPONENT_BYTES`] is shortened from the
/// end of its stem so the extension survives.
pub fn render_relative_path(
    scheme: &NamingScheme,
    show: &Show,
    episode: &Episode,
    ext: &str,
) -> crate::Result<PathBuf> {
    let template = match scheme {
        NamingScheme::JellyfinPlex => JELLYFIN_PLEX_TEMPLATE,
        NamingScheme::Kodi => KODI_TEMPLATE,
        NamingScheme::Custom { template } => template.as_str(),
    };
    let ext = ext.trim_start_matches('.');
    let year = show.year.map(|y| y.to_string());
    let template = if year.is_none() {
        template.replace("({year})", "").replace("[{year}]", "")
    } else {
        template.to_owned()
    };
    let components = parse_template(&template)?;
    let title = if episode.title.trim().is_empty() {
        format!("Episode {}", episode.key.number)
    } else {
        episode.title.clone()
    };
    let values = Values {
        show: &show.name,
        year: year.as_deref().unwrap_or(""),
        season: episode.key.season,
        episode: episode.key.number,
        title: &title,
        ext,
    };

    let mut path = PathBuf::new();
    let last = components.len() - 1;
    for (i, component) in components.iter().enumerate() {
        if i < last {
            path.push(sanitize_component(&render(component, &values)));
            continue;
        }
        // The file name: render the stem separately so it can be shortened while the extension
        // is kept.
        let name = match strip_extension_suffix(component) {
            Some(stem_tokens) => fit_file_name(&render(&stem_tokens, &values), ext),
            None => sanitize_component(&render(component, &values)),
        };
        path.push(name);
    }
    Ok(path)
}

/// Makes one path component valid on both macOS and Windows.
///
/// - `: ` becomes ` - ` and any other `:` becomes `-`, so `Show: Part 2` reads `Show - Part 2`.
/// - `/` and `\` become `-` (a value can never create a folder).
/// - `"` becomes `'`; `<`, `>`, `|`, `?` and `*` are removed.
/// - Control characters become spaces; runs of white space become one space.
/// - Leading dots are removed (a leading dot hides a file on macOS); trailing dots and spaces are
///   removed (Windows strips them silently, which would make two names refer to one file).
/// - The result is cut to [`MAX_COMPONENT_BYTES`] at a character boundary.
/// - Windows reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`, `COM0`-`COM9`,
///   `LPT0`-`LPT9`, and the superscript-digit forms), compared case-insensitively on the part
///   before the first dot, get `_` appended to that part (`con.txt` becomes `con_.txt`).
/// - An empty result becomes `_`.
///
/// The output does not depend on the operating system, so a library moved between the two keeps
/// its names.
pub fn sanitize_component(component: &str) -> String {
    let mut out = String::with_capacity(component.len());
    let mut chars = component.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ':' => {
                if chars.peek().is_some_and(|n| n.is_whitespace()) {
                    let trimmed = out.trim_end().len();
                    out.truncate(trimmed);
                    out.push_str(" -");
                } else {
                    out.push('-');
                }
            }
            '/' | '\\' => out.push('-'),
            '"' => out.push('\''),
            '<' | '>' | '|' | '?' | '*' => {}
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut name = trim_name(collapsed.trim_start_matches('.')).to_owned();
    if name.len() > MAX_COMPONENT_BYTES {
        name = trim_name(truncate_bytes(&name, MAX_COMPONENT_BYTES)).to_owned();
    }
    if name.is_empty() {
        return "_".to_owned();
    }
    avoid_reserved(name)
}

/// Builds `<stem>.<ext>` within [`MAX_COMPONENT_BYTES`], shortening the stem when needed.
fn fit_file_name(stem: &str, ext: &str) -> String {
    let ext = sanitize_extension(ext);
    let stem = sanitize_component(stem);
    if ext.is_empty() {
        return stem;
    }
    let room = MAX_COMPONENT_BYTES.saturating_sub(ext.len() + 1).max(1);
    let stem = if stem.len() > room {
        let cut = trim_name(truncate_bytes(&stem, room));
        if cut.is_empty() { "_" } else { cut }.to_owned()
    } else {
        stem
    };
    format!("{stem}.{ext}")
}

/// An extension keeps only letters, digits, `_` and `-`, at most 16 of them.
fn sanitize_extension(ext: &str) -> String {
    ext.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .take(16)
        .collect()
}

fn trim_name(s: &str) -> &str {
    s.trim_end_matches(['.', ' '])
}

fn truncate_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM0", "COM1", "COM2", "COM3", "COM4",
    "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
];

fn avoid_reserved(name: String) -> String {
    let stem_end = name.find('.').unwrap_or(name.len());
    let stem = name[..stem_end].trim_end();
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        format!("{stem}_{}", &name[stem_end..])
    } else {
        name
    }
}

// ---------------------------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Field {
    Show,
    Year,
    Season,
    Episode,
    Title,
    Ext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(String),
    Placeholder { field: Field, width: usize },
}

struct Values<'a> {
    show: &'a str,
    year: &'a str,
    season: u32,
    episode: u32,
    title: &'a str,
    ext: &'a str,
}

fn bad(message: impl Into<String>) -> RenameError {
    RenameError::BadTemplate(message.into())
}

fn is_ext(token: &Token) -> bool {
    matches!(
        token,
        Token::Placeholder {
            field: Field::Ext,
            ..
        }
    )
}

/// Splits a template into components of tokens and validates it.
fn parse_template(template: &str) -> crate::Result<Vec<Vec<Token>>> {
    let template = template.trim();
    if template.is_empty() {
        return Err(bad("the template is empty"));
    }
    if template.starts_with(['/', '\\']) {
        return Err(bad("the template must be a relative path"));
    }
    let mut components = Vec::new();
    for part in template.split(['/', '\\']) {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            return Err(bad("the template has an empty folder name"));
        }
        if trimmed == "." || trimmed == ".." {
            return Err(bad("folders named '.' or '..' are not allowed"));
        }
        components.push(parse_component(part)?);
    }
    let last = components.last().expect("at least one component");
    if !last.iter().any(is_ext) {
        return Err(bad("the file name must include {ext}"));
    }
    Ok(components)
}

fn parse_component(part: &str) -> crate::Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut literal = String::new();
    let mut rest = part;
    while let Some(c) = rest.chars().next() {
        match c {
            '{' => {
                let close = rest
                    .find('}')
                    .ok_or_else(|| bad(format!("missing '}}' in \"{part}\"")))?;
                let inner = &rest[1..close];
                if inner.contains('{') {
                    return Err(bad(format!("missing '}}' in \"{part}\"")));
                }
                if !literal.is_empty() {
                    tokens.push(Token::Literal(std::mem::take(&mut literal)));
                }
                tokens.push(parse_placeholder(inner)?);
                rest = &rest[close + 1..];
            }
            '}' => return Err(bad(format!("unmatched '}}' in \"{part}\""))),
            c => {
                literal.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    if !literal.is_empty() {
        tokens.push(Token::Literal(literal));
    }
    Ok(tokens)
}

fn parse_placeholder(inner: &str) -> crate::Result<Token> {
    let (name, format) = match inner.split_once(':') {
        Some((n, f)) => (n, Some(f)),
        None => (inner, None),
    };
    let field = match name {
        "show" => Field::Show,
        "year" => Field::Year,
        "season" => Field::Season,
        "episode" => Field::Episode,
        "title" => Field::Title,
        "ext" => Field::Ext,
        other => return Err(bad(format!("unknown placeholder {{{other}}}"))),
    };
    let width = match format {
        None => 0,
        Some(f) if matches!(field, Field::Season | Field::Episode) => f
            .strip_prefix('0')
            .filter(|d| d.len() == 1)
            .and_then(|d| d.parse::<usize>().ok())
            .filter(|w| (1..=9).contains(w))
            .ok_or_else(|| {
                bad(format!(
                    "unknown format {{{inner}}}; use for example {{{name}:02}}"
                ))
            })?,
        Some(_) => return Err(bad(format!("{{{name}}} takes no format"))),
    };
    Ok(Token::Placeholder { field, width })
}

/// When a component ends with `.{ext}`, returns its tokens without that suffix.
fn strip_extension_suffix(tokens: &[Token]) -> Option<Vec<Token>> {
    let (last, rest) = tokens.split_last()?;
    if !is_ext(last) {
        return None;
    }
    let (Token::Literal(lit), before) = rest.split_last()? else {
        return None;
    };
    let shortened = lit.strip_suffix('.')?;
    let mut out = before.to_vec();
    if !shortened.is_empty() {
        out.push(Token::Literal(shortened.to_owned()));
    }
    Some(out)
}

fn render(tokens: &[Token], v: &Values<'_>) -> String {
    let mut out = String::new();
    for token in tokens {
        match token {
            Token::Literal(s) => out.push_str(s),
            Token::Placeholder { field, width } => match field {
                Field::Show => out.push_str(v.show),
                Field::Year => out.push_str(v.year),
                Field::Season => out.push_str(&format!("{:0width$}", v.season, width = *width)),
                Field::Episode => out.push_str(&format!("{:0width$}", v.episode, width = *width)),
                Field::Title => out.push_str(v.title),
                Field::Ext => out.push_str(v.ext),
            },
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{episode, show};

    fn render_str(scheme: &NamingScheme, s: &Show, e: &Episode, ext: &str) -> String {
        render_relative_path(scheme, s, e, ext)
            .unwrap()
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    }

    #[test]
    fn jellyfin_plex_layout() {
        let got = render_str(
            &NamingScheme::JellyfinPlex,
            &show("Schoolhouse Rock!", Some(1973)),
            &episode(4, 1, "Conjunction Junction"),
            "mkv",
        );
        assert_eq!(
            got,
            "Schoolhouse Rock! (1973)/Season 04/Schoolhouse Rock! (1973) - S04E01 - Conjunction Junction.mkv"
        );
    }

    #[test]
    fn unknown_year_drops_the_brackets() {
        let got = render_str(
            &NamingScheme::JellyfinPlex,
            &show("Show", None),
            &episode(1, 2, "Two"),
            ".mp4",
        );
        assert_eq!(got, "Show/Season 01/Show - S01E02 - Two.mp4");
    }

    #[test]
    fn specials_and_three_digit_episodes() {
        let s = show("Show", Some(2001));
        assert_eq!(
            render_str(
                &NamingScheme::JellyfinPlex,
                &s,
                &episode(0, 3, "Pilot"),
                "mkv"
            ),
            "Show (2001)/Season 00/Show (2001) - S00E03 - Pilot.mkv"
        );
        assert_eq!(
            render_str(
                &NamingScheme::JellyfinPlex,
                &s,
                &episode(2, 104, "Late"),
                "mkv"
            ),
            "Show (2001)/Season 02/Show (2001) - S02E104 - Late.mkv"
        );
    }

    #[test]
    fn kodi_layout() {
        let got = render_str(
            &NamingScheme::Kodi,
            &show("Show", Some(1999)),
            &episode(1, 5, "Five"),
            "mkv",
        );
        assert_eq!(got, "Show (1999)/Season 01/Show S01E05 - Five.mkv");
    }

    #[test]
    fn custom_template_with_widths_and_backslashes() {
        let scheme = NamingScheme::Custom {
            template: r"{show}\S{season}\{episode:03} {title}.{ext}".into(),
        };
        let got = render_str(
            &scheme,
            &show("Show", Some(1999)),
            &episode(2, 7, "Seven"),
            "mkv",
        );
        assert_eq!(got, "Show/S2/007 Seven.mkv");
    }

    #[test]
    fn bad_templates_are_rejected() {
        let s = show("Show", None);
        let e = episode(1, 1, "One");
        for template in [
            "",
            "/abs/{title}.{ext}",
            "{show}/../{title}.{ext}",
            "{show}//{title}.{ext}",
            "{show}/{title}",
            "{show}/{nope}.{ext}",
            "{show}/{title.{ext}",
            "{show}/{title}}.{ext}",
            "{show}/{title:02}.{ext}",
            "{show}/{season:2}.{ext}",
        ] {
            let scheme = NamingScheme::Custom {
                template: template.into(),
            };
            let result = render_relative_path(&scheme, &s, &e, "mkv");
            assert!(
                matches!(result, Err(RenameError::BadTemplate(_))),
                "{template:?} was accepted: {result:?}"
            );
        }
    }

    #[test]
    fn values_never_create_folders() {
        let got = render_str(
            &NamingScheme::JellyfinPlex,
            &show("AC/DC: Live", None),
            &episode(1, 1, r"Back\In/Black?"),
            "mkv",
        );
        assert_eq!(
            got,
            "AC-DC - Live/Season 01/AC-DC - Live - S01E01 - Back-In-Black.mkv"
        );
    }

    #[test]
    fn empty_title_gets_a_readable_name() {
        let got = render_str(
            &NamingScheme::JellyfinPlex,
            &show("Show", None),
            &episode(1, 4, "  "),
            "mkv",
        );
        assert_eq!(got, "Show/Season 01/Show - S01E04 - Episode 4.mkv");
    }

    #[test]
    fn long_titles_are_shortened_and_keep_the_extension() {
        let title = "Überlänge ".repeat(40);
        let path = render_relative_path(
            &NamingScheme::JellyfinPlex,
            &show("Show", Some(2000)),
            &episode(1, 1, &title),
            "mkv",
        )
        .unwrap();
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(name.len() <= MAX_COMPONENT_BYTES, "{} bytes", name.len());
        assert!(name.ends_with(".mkv"));
        assert!(name.starts_with("Show (2000) - S01E01 - Überlänge"));
        assert!(!name.trim_end_matches(".mkv").ends_with(' '));
    }

    #[test]
    fn sanitize_replaces_characters_invalid_on_windows() {
        assert_eq!(sanitize_component("Who? Me*"), "Who Me");
        assert_eq!(sanitize_component("a<b>c|d"), "abcd");
        assert_eq!(sanitize_component("Say \"Hi\""), "Say 'Hi'");
        assert_eq!(
            sanitize_component("Star Trek: Picard"),
            "Star Trek - Picard"
        );
        assert_eq!(sanitize_component("10:30"), "10-30");
        assert_eq!(sanitize_component("tab\there\nnewline"), "tab here newline");
        assert_eq!(sanitize_component("  many   spaces  "), "many spaces");
    }

    #[test]
    fn sanitize_trims_dots_and_spaces() {
        assert_eq!(sanitize_component("The End..."), "The End");
        assert_eq!(sanitize_component(".hidden"), "hidden");
        assert_eq!(sanitize_component("Mr. Smith. "), "Mr. Smith");
        assert_eq!(sanitize_component("..."), "_");
        assert_eq!(sanitize_component(""), "_");
        assert_eq!(sanitize_component("???"), "_");
    }

    #[test]
    fn sanitize_avoids_windows_reserved_names() {
        assert_eq!(sanitize_component("CON"), "CON_");
        assert_eq!(sanitize_component("nul.txt"), "nul_.txt");
        assert_eq!(sanitize_component("Com1"), "Com1_");
        assert_eq!(sanitize_component("LPT9"), "LPT9_");
        assert_eq!(sanitize_component("COM¹"), "COM¹_");
        assert_eq!(sanitize_component("Conjunction"), "Conjunction");
        assert_eq!(sanitize_component("CONSOLE"), "CONSOLE");
    }

    #[test]
    fn sanitize_limits_length_on_a_character_boundary() {
        let long = "é".repeat(150); // 300 bytes
        let out = sanitize_component(&long);
        assert_eq!(out, "é".repeat(MAX_COMPONENT_BYTES / 2));
    }
}
