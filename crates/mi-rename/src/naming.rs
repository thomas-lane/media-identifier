//! File and folder names.

use std::path::PathBuf;

use mi_types::{Episode, NamingScheme, Show};

/// The new path of an episode file relative to the destination root.
///
/// - `JellyfinPlex`: `<Show> (<Year>)/Season NN/<Show> (<Year>) - SNNEMM - <Title>.<ext>`
///   (` (<Year>)` omitted when the year is unknown; specials go to `Season 00`).
/// - `Kodi`: `<Show> (<Year>)/Season NN/<Show> SNNEMM - <Title>.<ext>`.
/// - `Custom`: the template with placeholders `{show}`, `{year}`, `{season}`, `{season:02}`,
///   `{episode}`, `{episode:02}`, `{title}`, `{ext}`; `/` separates folders.
///
/// Every component is passed through [`sanitize_component`].
pub fn render_relative_path(
    scheme: &NamingScheme,
    show: &Show,
    episode: &Episode,
    ext: &str,
) -> crate::Result<PathBuf> {
    let _ = (scheme, show, episode, ext);
    todo!("release module: naming schemes")
}

/// Makes one path component valid on both macOS and Windows: replaces `<>:"/\|?*` and control
/// characters, trims trailing dots and spaces, and suffixes Windows reserved names (`CON`,
/// `NUL`, `COM1`...). Results are identical on both systems so a library moved between them
/// keeps its names.
pub fn sanitize_component(component: &str) -> String {
    let _ = component;
    todo!("release module: sanitising names")
}
