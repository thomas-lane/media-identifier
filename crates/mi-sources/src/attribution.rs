//! Credits the UI shows for each online source, and display names.

use mi_types::{Attribution, ProviderId};

/// The provider's name as users know it.
pub fn provider_name(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Tvmaze => "TVmaze",
        ProviderId::Tmdb => "TMDb",
        ProviderId::Subdl => "SubDL",
        ProviderId::Lrclib => "LRCLIB",
        ProviderId::Embedded => "Embedded subtitles",
        ProviderId::Local => "Local files",
    }
}

/// The credit to show when data from `provider` is used, or `None` for local sources.
///
/// - TVmaze: its data is CC BY-SA 4.0, which requires crediting TVmaze with a link.
/// - TMDb: its API terms require this notice and the TMDB logo wherever TMDb data is used (the
///   logo is the UI's to show).
/// - SubDL and LRCLIB ask for no credit; the app names them anyway so users know where text
///   came from.
pub fn attribution(provider: ProviderId) -> Option<Attribution> {
    let (text, url, license, license_url) = match provider {
        ProviderId::Tvmaze => (
            "Episode lists from TVmaze",
            "https://www.tvmaze.com",
            Some("CC BY-SA 4.0"),
            Some("https://creativecommons.org/licenses/by-sa/4.0/"),
        ),
        ProviderId::Tmdb => (
            "This application uses TMDB and the TMDB APIs but is not endorsed, certified, or otherwise approved by TMDB.",
            "https://www.themoviedb.org",
            None,
            None,
        ),
        ProviderId::Subdl => ("Subtitles from SubDL", "https://subdl.com", None, None),
        ProviderId::Lrclib => ("Lyrics from LRCLIB", "https://lrclib.net", None, None),
        ProviderId::Embedded | ProviderId::Local => return None,
    };
    Some(Attribution {
        provider,
        text: text.to_owned(),
        url: url.to_owned(),
        license: license.map(str::to_owned),
        license_url: license_url.map(str::to_owned),
    })
}

/// Credits for every online source, in the order of the Settings screen.
pub fn attributions() -> Vec<Attribution> {
    [
        ProviderId::Tvmaze,
        ProviderId::Lrclib,
        ProviderId::Subdl,
        ProviderId::Tmdb,
    ]
    .into_iter()
    .filter_map(attribution)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tvmaze_credit_links_to_tvmaze_and_names_the_license() {
        let a = attribution(ProviderId::Tvmaze).unwrap();
        assert_eq!(a.url, "https://www.tvmaze.com");
        assert_eq!(a.license.as_deref(), Some("CC BY-SA 4.0"));
        assert!(a.text.contains("TVmaze"));
    }

    #[test]
    fn tmdb_credit_is_the_required_notice() {
        assert_eq!(
            attribution(ProviderId::Tmdb).unwrap().text,
            "This application uses TMDB and the TMDB APIs but is not endorsed, certified, or otherwise approved by TMDB."
        );
        assert!(attribution(ProviderId::Local).is_none());
        assert_eq!(attributions().len(), 4);
    }
}
