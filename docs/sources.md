# Online sources

<!-- owner: sources module -->

Media Identifier sends only show names, provider ids and episode numbers online. Audio and video
stay on the computer. Every request goes through one HTTP client (`mi_sources::HttpClient`) that
sends `User-Agent: MediaIdentifier/<version>`, waits when a provider's rate-limit headers say the
allowance is spent, and retries HTTP 429 after `Retry-After` (or exponential back-off). Responses
and normalised reference text are stored in a local SQLite cache (`<app data>/cache.sqlite`), so
nothing is downloaded twice.

| Source | Supplies | Key | Terms and attribution |
|---|---|---|---|
| [TVmaze](https://www.tvmaze.com/api) | show search, episode lists, DVD-order lists | none | CC BY-SA 4.0; the app shows a visible credit linking to TVmaze |
| [TMDb](https://developer.themoviedb.org) | optional episode numbering that matches Jellyfin | user's own key | cached at most six months; TMDb attribution shown when used |
| [SubDL](https://subdl.com/api-doc) | subtitles, preferably whole-season packs | user's own free key | |
| [LRCLIB](https://lrclib.net/docs) | song lyrics for musical shorts | none | |
| Embedded subtitles | text subtitle streams inside the files | none | local |

## TVmaze

Endpoints: `/search/shows?q=`, `/shows/:id/episodes?specials=1`, `/shows/:id/alternatelists`,
`/alternatelists/:id/alternateepisodes?embed=episodes`.

## TMDb

The app ships no TMDb key: TMDb's terms make each key holder responsible for how the key is used,
so a user who wants Jellyfin-consistent numbering adds their own key in Settings.

## SubDL

The search API answers `403 not_authorized` without an `api_key`, so users enter their own free
key in Settings. Season packs are requested with `full_season=1`.

## LRCLIB

## Embedded subtitles

Text subtitle streams (SubRip, ASS/SSA, WebVTT, MP4 text) are extracted with ffmpeg and used as a
transcript of that file. Bitmap subtitles from DVDs and Blu-rays need OCR and are not used.
