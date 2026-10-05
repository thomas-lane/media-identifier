# Third-party software and data

Direct dependencies and their licenses. Each module records its own dependencies in its section.

## Shared (integrator)

| Component | License | Used for |
|---|---|---|
| serde, serde_json | MIT OR Apache-2.0 | JSON |
| ts-rs | MIT | TypeScript types from Rust types |
| thiserror | MIT OR Apache-2.0 | error types |
| tokio | MIT | async runtime |
| tracing, tracing-subscriber | MIT | logging |
| tempfile (tests) | MIT OR Apache-2.0 | temporary folders in tests |
| async-trait (`mi-core`) | MIT OR Apache-2.0 | the `Catalog` service trait |
| Tauri, tauri-build, tauri-plugin-updater, tauri-plugin-dialog, tauri-plugin-opener | Apache-2.0 OR MIT | desktop shell, updates, dialogs, opening links |
| React, React DOM | MIT | UI |
| @tauri-apps/api, @tauri-apps/plugin-dialog, @tauri-apps/plugin-opener | Apache-2.0 OR MIT | UI access to Tauri |
| Vite, Vitest, TypeScript, ESLint, typescript-eslint, Testing Library (dev only) | MIT / Apache-2.0 | build and tests |

## media

| Component | License | Used for |
|---|---|---|
| FFmpeg 9.0.2 (ffmpeg, ffprobe sidecars, built by `scripts/build-ffmpeg.sh`) | LGPL-2.1-or-later; notice and configure lines in `third_party/ffmpeg/` | probing, audio decoding and subtitle extraction (separate executables) |

## transcribe

| Component | License | Used for |
|---|---|---|
| whisper-rs, whisper-rs-sys | Unlicense | Rust bindings to whisper.cpp |
| whisper.cpp, ggml | MIT | speech recognition (compiled in) |
| Whisper model weights (`ggerganov/whisper.cpp` conversions) | MIT | downloaded on first run |
| Silero VAD model (`ggml-org/whisper-vad` conversion of snakers4/silero-vad) | MIT | voice activity detection, downloaded with the speech model |
| sha2, hex | MIT OR Apache-2.0 | download verification |
| reqwest, futures-util | MIT OR Apache-2.0 | downloads |
| flate2 | MIT OR Apache-2.0 | compression ratio of recognised text |
| hound (tests and example only) | Apache-2.0 | reading WAV files |

## sources

| Component | License | Used for |
|---|---|---|
| reqwest | MIT OR Apache-2.0 | HTTP |
| rusqlite (bundled SQLite) | MIT (SQLite: public domain) | cache |
| async-trait | MIT OR Apache-2.0 | provider traits with async methods |
| encoding_rs | (Apache-2.0 OR MIT) AND BSD-3-Clause | reading UTF-16 and Windows-1252 subtitle files |
| httpdate | MIT OR Apache-2.0 | `Retry-After` dates |
| zip (deflate through flate2 and zlib-rs: MIT OR Apache-2.0, Zlib) | MIT | SubDL subtitle archives |
| TVmaze data | CC BY-SA 4.0 | episode lists (credited in the app); recorded test fixtures |
| TMDb data (user's own key) | TMDb API terms | optional episode numbering (notice shown in the app) |
| SubDL subtitles (user's own key) | SubDL terms; subtitle rights belong to their authors | reference text, cached locally |
| LRCLIB lyrics | rights belong to their holders | reference text, cached locally; test fixtures keep two-line excerpts |

## match

| Component | License | Used for |
|---|---|---|
| rapidfuzz | MIT | character similarity (`ratio`) for fuzzy matching |
| rphonetic | Apache-2.0 | Double Metaphone codes |
| pathfinding | Apache-2.0 OR MIT | Kuhn-Munkres assignment |
| rustfft | MIT OR Apache-2.0 | spectra for audio fingerprints |

## release

| Component | License | Used for |
|---|---|---|
| csv | Unlicense OR MIT | CSV export |
| libc (macOS only) | MIT OR Apache-2.0 | `renamex_np` for renames that never replace a file |
| windows-sys (Windows only) | MIT OR Apache-2.0 | `MoveFileExW` for renames that never replace a file |

## branding

Build-time tools for `assets/brand/build.sh`; none of them ships with the app. The documentation
graphics in `docs/images/` contain outlines of Inter glyphs.

| Component | License | Used for |
|---|---|---|
| Inter 4.1 (font) | SIL Open Font License 1.1 | text in the README header and documentation figures, drawn as outlines |
| fontTools | MIT | reading glyph outlines |
| uharfbuzz (HarfBuzz) | Apache-2.0 (HarfBuzz: MIT) | text shaping and kerning |
| librsvg (`rsvg-convert`) | LGPL-2.1-or-later | rendering the icon SVGs to PNG |
| Pillow | MIT-CMU | writing `icon.ico` |
| oxipng | MIT | lossless PNG compression |
| Codex CLI image generation | OpenAI terms of use (output owned by the user) | icon and header concepts in `assets/brand/concepts/` |
