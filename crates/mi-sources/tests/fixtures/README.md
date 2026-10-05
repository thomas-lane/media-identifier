# Test fixtures

- `tvmaze/` and `lrclib/`: responses recorded from the live APIs on 2026-10-05 with the app's
  User-Agent, unchanged. TVmaze data is CC BY-SA 4.0 (credit: TVmaze, https://www.tvmaze.com).
  In the LRCLIB files every lyrics field is cut to its first two non-empty lines and the
  `lyricsfile` field is replaced, because full song lyrics belong to their rights holders.
- `subdl/` and `tmdb/`: written by hand in the response shapes given by the providers'
  documentation (SubDL API docs and TMDb API reference), because both need a personal API key.
  They are not recordings and say nothing about what the live services return for these shows.
