# Online sources

Media Identifier needs two things from outside the computer: the show's episode list, and text
known to belong to each episode (subtitles, song lyrics or a summary), which it compares with
what it hears in each file. All of this lives in `crates/mi-sources`; `mi_sources::Sources` is
the entry point the job pipeline uses.

| Source | Supplies | Key | Terms and attribution |
|---|---|---|---|
| [TVmaze](https://www.tvmaze.com/api) | show search, episode lists, DVD-order lists | none | data is CC BY-SA 4.0; the app shows a credit linking to TVmaze |
| [TMDb](https://developer.themoviedb.org) | optional episode numbering that matches Jellyfin | the user's own key | cached at most six months; TMDb's notice and logo shown when used |
| [SubDL](https://subdl.com/api-doc) | subtitles, preferably whole-season packs | the user's own free key | each user brings their own key |
| [LRCLIB](https://lrclib.net/docs) | song lyrics for musical shorts | none | identify the app in the User-Agent; one request at a time |
| Local files | subtitle or lyrics files in a folder (`mi_sources::local`) | none | local |
| Embedded subtitles | text subtitle streams inside the files | none | local |

The credits the UI shows come from `mi_sources::attribution` (type `mi_types::Attribution`), so
the wording lives in one place.

## What is sent online

Audio, video, transcripts and file paths never leave the computer. Requests to the sources carry
only:

| Source | Request contents |
|---|---|
| TVmaze | the show name typed or guessed from the folder name; TVmaze show and list ids |
| TMDb | TheTVDB or IMDb id of the show, or its name and first-air year; TMDb show, season and episode-group ids; the user's key |
| SubDL search | the show's IMDb id (else TMDb id, else name and year), season number, language; the user's key |
| SubDL download | the subtitle's download path from the search result, without a key |
| LRCLIB | an episode title and the show name |

Every request to a source goes through `mi_sources::HttpClient` and sends
`User-Agent: MediaIdentifier/<version> (https://github.com/thomas-lane/media-identifier)`. TVmaze
asks for a User-Agent that identifies the application, and LRCLIB requires the application's name,
version and a link to its homepage.

The app makes two other kinds of request, outside `HttpClient` because they are file downloads
rather than provider calls:

- **Speech models** (`mi_transcribe::ModelStore`): the pinned model files from Hugging Face, with
  `User-Agent: MediaIdentifier/<version>`, resumable and verified by SHA-256 (see
  [architecture](architecture.md#transcription)). They send nothing about the user's files.
- **Update checks** (`tauri-plugin-updater`): the release feed `latest.json` and, after the user
  agrees, the update itself (see [development](development.md#releases)). They send the app's
  version and platform.

**Response size.** A response body is read chunk by chunk, after gzip decoding, and abandoned once
it passes a limit: 16 MiB by default (`mi_sources::http::MAX_BODY_BYTES`) and 32 MiB for SubDL
archives (`mi_sources::subdl::MAX_DOWNLOAD_BYTES`). Real responses are far smaller; the limit keeps
a broken or hostile server, or a compressed "bomb" that expands enormously, from exhausting
memory, since bodies are held in memory and cached. A response over the limit fails with a plain
message, is not retried and is not cached.

**Redirects** are followed only to `https` addresses, at most five, so a redirect can never send a
request that may carry a key over plain HTTP.

API keys travel separately from the request URL (`mi_sources::http::Secret`): a key is added only
when the request is sent, so it never appears in logs or cache keys. Every type that holds a key
(`Secret`, `ApiKeys`, the `Subdl` and `Tmdb` providers, the settings store) prints it as
`<hidden>` in `Debug` output, and tests check that.
Settings stores keys outside the settings file (see `AGENTS.md`, "Keys stay private").

## Rate limits and retries

Each provider has its own queue in `HttpClient`, and two requests to one provider are spaced by at
least:

| Provider | Minimum spacing | Why |
|---|---|---|
| TVmaze | 500 ms | TVmaze allows at least 20 calls per 10 seconds per IP address |
| SubDL | 500 ms | SubDL publishes only daily quotas; this keeps bursts small |
| LRCLIB | 250 ms | LRCLIB asks for sequential requests with a 200-500 ms pause |
| TMDb | 50 ms | TMDb's upper limit is about 40 requests per second |

For every request `HttpClient::send`:

1. fails at once with "rate limited" while the provider's allowance is known to be spent for more
   than 60 seconds (for example a daily quota), so a job never waits hours;
2. waits for the provider's next slot;
3. on HTTP 429 or 503, waits for `Retry-After` (seconds or an HTTP date), else the rate-limit reset
   header, else 1, 2, then 4 seconds; other 5xx answers and failed connections are retried with
   the same back-off; four tries in all;
4. after every response, reads `X-RateLimit-Remaining`/`-Reset`, `RateLimit-Remaining`/`-Reset`,
   `X-Rate-Limit-Remaining`/`-Reset` or the combined `RateLimit` header, and when the remaining
   allowance is zero holds the next request until the reset. Providers write the reset as seconds
   from now, Unix seconds or Unix milliseconds; values above 10^12 are read as milliseconds, above
   10^9 as Unix seconds, and anything smaller as seconds from now.

HTTP 401 or 403 on a request with a key means the key was rejected; 404 means "not found". The
outcome of the latest request to each provider feeds the Settings screen (`Sources::status`):
Ready, Needs key, Key rejected, or Unavailable with a plain reason.

## The cache

`<app data>/cache.sqlite` (`mi_sources::Cache`) holds two tables:

- `responses`: raw response bodies (JSON, subtitle files and archives), keyed by provider and the
  request URL without the key;
- `texts`: normalised reference text, keyed by provider, the provider's item id, show, ordering,
  season, episode and kind.

A cached response is used without asking the provider while it is fresh. When the provider is
unreachable or rate limiting, an older copy is used instead, within the limit below, so a show
seen before still works offline.

| Data | Fresh for | Used offline up to |
|---|---|---|
| TVmaze search results | 1 day | any age |
| TVmaze show details, episode lists, DVD lists | 7 days | any age |
| TMDb responses | 30 days | 6 months |
| SubDL search results | 30 days | any age |
| SubDL subtitle files and archives | always (files do not change) | any age |
| LRCLIB search results | 90 days | any age |

TMDb's terms forbid caching its data for more than six months, so opening the cache deletes TMDb
rows older than that (`Cache::purge_expired`), and the offline limit stops older copies from being
used before then. Key checks are never cached, and neither is text from local files, which may
change and are cheap to read again. A cache file that is not a readable SQLite database is renamed to
`cache.sqlite.damaged-<time>` and a new cache is started, because everything in it can be
downloaded again.

Reference text found for an episode is also written to `texts`, so the next job for the same show
reads it directly and makes no requests for that episode.

## Episode lists

`Sources::episodes(show, ordering)` returns the list sorted by season and episode, with specials
as season 0. A *TVmaze show* is the default; with a TMDb key set, TMDb's numbering replaces it (see
TMDb below).

### TVmaze

Endpoints: `/search/shows?q=`, `/shows/:id`, `/shows/:id/episodes?specials=1`,
`/shows/:id/alternatelists`, `/alternatelists/:id/alternateepisodes?embed=episodes`.

- **Search**: scores are TVmaze's relevance scores divided by the best one. The first five results
  get season and episode counts (for the Confirm show screen) from their episode lists, which are
  cached and reused when the show is picked.
- **Aired order**: TVmaze's main list. Specials (episodes without a number) become season 0,
  numbered by air date, because TVmaze gives specials no number of their own.
- **DVD order**: TVmaze's alternate list marked as a DVD release. Each episode keeps its TVmaze
  episode id, so aired and DVD lists can be matched up. A show without one fails with a plain
  "no DVD order" message.
- **Year**: the year of the show's first air date (`premiered`), used in folder names.

TVmaze data is licensed CC BY-SA 4.0: it may be used for any purpose if TVmaze is credited with a
link and changes are shared alike. The app credits TVmaze wherever episode data is shown.

### TMDb (optional)

TMDb numbers episodes the way Jellyfin does by default. The app ships no TMDb key: TMDb's API terms
prohibit using TMDb "in connection with ... a machine learning (ML) or artificial intelligence
(AI) based Application", and the app's speech recognition is machine learning. A user who adds
their own key in Settings accepts TMDb's terms for their own use.

- **Keys**: a 32-character v3 API key is sent as the `api_key` query parameter; anything else is
  sent as a v4 read access token in `Authorization: Bearer`. `Sources::validate_key` checks a key
  with `/authentication`.
- **Finding the show**: a TVmaze show is looked up on TMDb through `/find` with its TheTVDB id, then
  its IMDb id (both from TVmaze), then by `/search/tv`, accepting only a single result with the same
  name and first-air year. A show that cannot be found keeps TVmaze's list.
- **Aired order**: `/tv/{id}` lists the seasons, including season 0 (specials); each comes from
  `/tv/{id}/season/{n}`.
- **DVD order**: the show's first episode group of type 3 (DVD) from `/tv/{id}/episode_groups` and
  `/tv/episode_group/{id}`. Each group becomes a season: a group named "Specials" is season 0,
  otherwise the season is the number in the group's name, otherwise its position. Episodes are
  numbered by their order in the group. Without a DVD group, TVmaze's DVD list is used.
- **Fallback**: when TMDb fails (rejected key, network), TVmaze's list is used and Settings shows
  the problem, so a TMDb outage never stops a job.

Episodes from TMDb carry TMDb's show and episode ids (`Episode::show_ref`,
`Episode::provider_episode_id`). The UI shows TMDb's required notice and logo when TMDb numbering
was used.

## Reference text

`Sources::reference_texts(show, episodes, language, ...)` collects text for each episode, in this
order, stopping for an episode once it has dialogue (subtitles or lyrics):

1. text cached by an earlier job;
2. local files, when the caller added a `LocalReferences` provider;
3. SubDL subtitles, when a SubDL key is set;
4. LRCLIB lyrics, for episodes of ten minutes or less or of unknown length.

An episode still without dialogue gets its summary from the episode list, the weakest evidence
(summaries describe an episode rather than quote it). Embedded subtitle streams are not part of
this call: they belong to a file, not an episode, so `mi-core` reads them from each file
(`mi_sources::embedded`) and matches them like a perfect transcript.

A source that fails (network down, rate limited, key rejected) is skipped for the rest of the call
and the next one is tried, so one failing service never fails a job. Episodes are processed season
by season; the progress callback reports episodes done after each season.

All text is normalised the same way (`mi_sources::text`): cue numbers, timing lines, markup,
speaker labels (`JOHN:`), sound descriptions in brackets or parentheses and music-note symbols
are removed, multi-line cues are joined, repeated consecutive lines are dropped, and one cue or
lyric line is kept per line, in the original words and case. SubRip, WebVTT, ASS/SSA and LRC are
recognised by content. Files without a byte-order mark that are not valid UTF-8 are read as
Windows-1252, the usual encoding of older Western subtitle files.

### SubDL

SubDL's search API (`https://api.subdl.com/api/v1/subtitles`) requires a key: its documentation
lists `api_key` as required, and a request without one is answered HTTP 403 `not_authorized`.
SubDL's terms allow apps in which each user enters their own key, so Settings asks for one; a
free SubDL account gets one from its account panel. `Sources::validate_key` checks it with SubDL's
account endpoint `/api/v1/me`.

For each season (in broadcast numbering, which SubDL uses):

1. One search with `full_season=1&unpack=1`. `unpack=1` makes SubDL list the files inside each
   archive with their season and episode numbers. The pack covering the most wanted episodes is
   chosen (ties: not hearing-impaired) and downloaded once as a ZIP. Each archive entry is matched
   to an episode through that list, else through a marker in its name (`S01E02`, `1x02`).
2. When episodes are still missing, one search without `full_season`. For each missing episode the
   app downloads the single subtitle file SubDL lists for it (not hearing-impaired first), or the
   episode's own archive.

Downloads come from `https://dl.subdl.com` without the key: SubDL counts anonymous downloads per IP
address (300 a day), and authenticated downloads exist only on paid plans. A download link from
the search results must be a path on that host or a full `https://dl.subdl.com/...` address;
anything else (another host, plain HTTP, or a path that would change the host, such as
`@other.example/x`) is refused, so search results can never make the app contact another server.
A free key allows 2,000 searches a day. A season pack costs one download for a whole season, which is why packs come first.

Episodes in DVD order are looked up under their aired numbers (matched through the episode ids of
the aired list) and reported under their DVD numbers, so a subtitle for broadcast episode 11 is
never attached to DVD episode 11.

Archives are read with limits, so a damaged or hostile archive cannot exhaust memory when it is
unpacked: at most 500 entries, 5 MiB per subtitle file and 100 MiB in all; only `.srt`, `.ass`,
`.ssa` and `.vtt` files are read. The archive itself is at most 32 MiB (see "Response size"
above).

SubDL also offers an API v2 (`/api/v2/...`, key in an `Authorization` header). The app uses v1
because v1's documentation describes the season-pack listing (`unpack_files` with season and
episode numbers) that the pack choice above depends on.

### LRCLIB

LRCLIB (`https://lrclib.net/api/search`) needs no key. Musical shorts such as Schoolhouse Rock!
are songs, so lyrics are near-perfect dialogue for them. Each episode is searched with its title
as `track_name` and the show name as `artist_name`, then, if nothing fits, as `album_name`. A record
is accepted only when it is not instrumental, has lyrics, its track name equals the title (or
contains it as whole words, for example `Grammar Rock - Conjunction Junction`), and its artist or
album names the show (ignoring spaces and punctuation, so `School House Rock` counts). Among
accepted records the app prefers an exact title, then plain lyrics, then the duration closest to
the episode's listed runtime, then the lowest record id so the choice is stable. Plain lyrics are
preferred because matching needs only the words; synced lyrics are used without their timestamps
when no plain version exists.

Episodes longer than ten minutes are not looked up: lyrics are evidence only for shorts that are
songs, and skipping long episodes keeps a 100-episode drama from costing 200 lyrics requests.

### Local files

`mi_sources::local::LocalReferences::from_folder(path)` reads `.srt`, `.vtt`, `.ass`, `.ssa`
(subtitles) and `.lrc`, `.txt` (lyrics) files from a folder and its immediate subfolders. A file
belongs to an episode when its name has that episode's marker (`S01E02`, `1x02`,
`Season 1 Episode 2`, in the numbering of the episodes asked for), or else when its name contains
the episode title as whole words; when a name contains several titles, the longest wins, so
`My Hero, Zero.srt` is not taken for an episode called `Zero`. Tests and the check against real
episodes use it; it is also a way to supply subtitles a user already has.

### Embedded subtitles

Text subtitle streams (SubRip, ASS/SSA, WebVTT, MP4 text) are extracted with ffmpeg by `mi-media`
and normalised by `mi_sources::embedded::dialogue_from_srt`. Bitmap subtitles from DVDs and
Blu-rays are images and would need text recognition, so they are not used.

## Tests

Tests never use the network. `mi_sources::testing::FixtureTransport` answers requests from
responses registered per URL and records every request, so tests can assert what was sent and
that nothing was fetched twice. Recorded TVmaze and LRCLIB responses are in
`crates/mi-sources/tests/fixtures/` (lyrics cut to two lines); SubDL and TMDb fixtures are written
by hand in the documented response shapes, because both services need a personal key.
`crates/mi-sources/tests/live.rs` holds smoke tests against the real services, ignored by default:

```bash
cargo test -p mi-sources --test live -- --ignored --nocapture
MI_SUBDL_KEY=... MI_TMDB_KEY=... cargo test -p mi-sources --test live -- --ignored --nocapture
```

The SubDL and TMDb smoke tests return early without their environment variable.
