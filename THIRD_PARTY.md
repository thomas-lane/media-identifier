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
| Tauri, tauri-build, tauri-plugin-updater, tauri-plugin-dialog, tauri-plugin-opener | Apache-2.0 OR MIT | desktop shell, updates, dialogs, opening links |
| React, React DOM | MIT | UI |
| @tauri-apps/api, @tauri-apps/plugin-dialog, @tauri-apps/plugin-opener | Apache-2.0 OR MIT | UI access to Tauri |
| Vite, Vitest, TypeScript, ESLint, typescript-eslint, Testing Library (dev only) | MIT / Apache-2.0 | build and tests |

## media

| Component | License | Used for |
|---|---|---|
| FFmpeg (ffmpeg, ffprobe sidecars) | LGPL-2.1-or-later | probing and audio decoding (separate executables) |

## transcribe

| Component | License | Used for |
|---|---|---|
| whisper-rs, whisper-rs-sys | Unlicense | Rust bindings to whisper.cpp |
| whisper.cpp, ggml | MIT | speech recognition (compiled in) |
| Whisper model weights (`ggerganov/whisper.cpp` conversions) | MIT | downloaded on first run |
| sha2, hex | MIT OR Apache-2.0 | download verification |
| reqwest | MIT OR Apache-2.0 | downloads |

## sources

| Component | License | Used for |
|---|---|---|
| reqwest | MIT OR Apache-2.0 | HTTP |
| rusqlite (bundled SQLite) | MIT (SQLite: public domain) | cache |
| TVmaze data | CC BY-SA 4.0 | episode lists (credited in the app) |

## match

| Component | License | Used for |
|---|---|---|
| rapidfuzz | MIT | fuzzy partial matching |
| rphonetic | Apache-2.0 | Double Metaphone codes |
| pathfinding | Apache-2.0 OR MIT | Kuhn-Munkres assignment |

## release

| Component | License | Used for |
|---|---|---|
| csv | Unlicense OR MIT | CSV export |

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
