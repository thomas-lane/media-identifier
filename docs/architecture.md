# Architecture

Media Identifier is a Tauri 2 desktop app: a Rust core split into workspace crates, a React +
TypeScript UI in a system web view, and two helper executables (ffmpeg and ffprobe). Speech
recognition (whisper.cpp) is compiled into the app, so it adds no helper executable.

## Crates

```text
ui (React, TypeScript)
  │  Tauri commands (invoke) and events (listen); types generated from mi-types
src-tauri  ── commands, events, settings persistence, updater
  │
mi-core    ── engine: one job at a time, stage by stage, progress events
  ├── mi-media       ffprobe/ffmpeg sidecars: scan, probe, play-all, PCM, embedded subtitles
  ├── mi-transcribe  model catalog/download, sampling windows, whisper.cpp, hallucination filter
  ├── mi-sources     TVmaze, TMDb, SubDL, LRCLIB, embedded text, SQLite cache, HTTP client
  ├── mi-match       text similarity, title hooks, duration, audio alignment, assignment
  └── mi-rename      naming, rename/copy/CSV, History journal
mi-types   ── shared serde types, used by every crate above
```

Each crate depends only on `mi-types` and third-party crates, except `mi-core` (all crates) and
`src-tauri` (`mi-core`, `mi-types`, and `mi-media`/`mi-sources` for two configuration types). The
narrow dependency graph lets each module be built and tested alone, and keeps the matching logic
free of I/O so it can be tested with plain data.

## A job, end to end

1. **Scan** (`mi_media::scan_folder`): list video files, probe each with ffprobe, mark the play-all
   title (duration close to the sum of the others), guess the show from the folder name, and warn
   when the play-all has more chapters than short files were found.
2. **Confirm show** (`mi_sources::Sources::search_shows`): the user picks the show.
3. **Episode list** (`Sources::episodes`): TVmaze, or TMDb numbering with a user key.
4. **Reference text** (`Sources::reference_texts`, `mi_media::extract_text_subtitles`): subtitles,
   lyrics, embedded text streams, summaries; all cached.
5. **Disc order** (`mi_media::stream_audio`, `mi_match::align`): fingerprint each file and the
   play-all, locate each file inside the play-all, and decide whether that order is trustworthy.
6. **Listening** (`mi_transcribe`): choose windows, decode them to 16 kHz mono PCM, transcribe,
   filter invented text.
7. **Matching** (`mi_match::match_files`): score every file against every episode, choose the best
   overall assignment, classify each file as Confident, Check or Extra. Files with a low margin get
   more windows transcribed and are matched again.
8. **Review and save** (`mi_rename`): the user approves; files are renamed in place (journaled for
   undo), copied, or exported as CSV.

`docs/identification.md` explains steps 5-7.

## Threads, cancellation and progress

`mi_core::Engine` runs at most one job. Network calls are async on Tokio. ffmpeg and whisper.cpp
calls block, so they run on Tokio's blocking pool. Every long-running function takes a
`mi_types::CancelFlag` (a shared atomic flag) and checks it between units of work; ffmpeg child
processes are killed and whisper.cpp stops through its abort callback. A plain flag is used
because whisper.cpp runs on threads that cannot await an async token.

Progress reaches the UI as `JobEvent`s: `mi_core::EventSink` is implemented by
`src-tauri/src/sink.rs`, which emits Tauri events. The last event of a job is always `Finished`,
`Failed` or `Cancelled`.

## Commands and events

Commands are defined in `src-tauri/src/commands.rs` and `src-tauri/src/updater.rs`, listed in
`commands::COMMANDS`, and called by `ui/src/api/tauri.ts`. A Rust test fails when a registered
command is missing from the list or from the UI client. Every command returns
`Result<T, mi_types::ApiError>`; the UI receives `{ code, message }`.

| Event channel (`mi_types::events`) | Payload | Sent when |
|---|---|---|
| `job-event` | `JobEvent` | a job progresses |
| `model-download` | `ModelStatus` | a speech model download progresses |
| `update-event` | `UpdateEvent` | an accepted update downloads (about once per percent) |
| `update-available` | `UpdateInfo` | a background check (at launch, then daily) finds a version the user has not skipped and was not offered in the last 24 hours |

## Shared types

All types that cross the command/event boundary are defined once in `crates/mi-types` and
generated into `ui/src/types/generated/` with `ts-rs`
(`MI_UPDATE_BINDINGS=1 cargo test -p mi-types --test bindings`). The same test without the
variable fails when the generated files are stale, so a Rust change that forgets the UI fails CI.
JSON uses camelCase fields and `"kind"` tags; 64-bit integers are generated as `number`.

## The UI backend client

Screens talk to the app only through the `Backend` interface in `ui/src/api/backend.ts`.
`ui/src/api/tauri.ts` implements it with Tauri commands and events; `ui/src/api/mock.ts`
implements it in memory with sample data (`mockData.ts`), simulating jobs, downloads and updates
with timers. `getBackend()` picks the mock under `npm run dev:mock` or outside Tauri, so every
screen can be developed in a normal browser and tested in jsdom.

## Helper executables

ffmpeg and ffprobe are Tauri sidecars (`bundle.externalBin` in `src-tauri/tauri.conf.json`):
Tauri copies `src-tauri/binaries/<name>-<target triple>` next to the app executable.
`mi_media::Sidecars::resolve` looks for each tool in this order: the `MI_FFMPEG`/`MI_FFPROBE`
environment variables, the executable's folder, and (debug builds only) `PATH`. A zero-byte file
counts as missing, because `src-tauri/build.rs` writes empty placeholders when the real binaries
are absent so that the workspace compiles on a fresh clone. Release builds set
`MI_REQUIRE_SIDECARS=1`, which makes `build.rs` fail on a missing or empty binary.

The sidecars are a minimal LGPL build. Components it must include:

| Purpose | Components |
|---|---|
| Containers | demuxers `matroska`, `mov` (mp4/m4v/mov), `avi`, `mpegps` (VOB), `mpegts`, and `mpegvideo` (raw MPEG video, which the VOB demuxer uses to recognise DVD video streams) |
| Audio | decoders `ac3`, `eac3`, `aac`, `aac_latm`, `mp2`, `mp3` (and their float variants), `dca` (DTS), `truehd`, `mlp`, `flac`, `opus`, `vorbis`, common `pcm_*` (including DVD and Blu-ray PCM); filters `aresample`, `aformat`, `anull`, `atrim`; encoders `pcm_f32le`, `pcm_s16le`; muxers `f32le`, `s16le`, `wav`, `null` |
| Embedded subtitles | decoders `subrip`, `ass`, `ssa`, `webvtt`, `mov_text`, `text`; encoders `subrip`/`srt`; muxer `srt` |
| Timestamps and stream details | parsers `aac`, `aac_latm`, `ac3`, `dca`, `flac`, `mlp`, `mpegaudio`, `opus`, `vorbis`, `h264`, `hevc`, `mpegvideo`, `vc1` |
| I/O | protocols `file`, `pipe` |
| Probing | ffprobe with JSON output |

The exact configure options, source version and checksum are in `third_party/ffmpeg/NOTICE.md`;
`docs/development.md` ("ffmpeg sidecars") explains the build.

## Data on disk

| What | Where | Owner crate |
|---|---|---|
| Settings | `<app config>/settings.json` | `src-tauri` (`settings_store.rs`) |
| API keys | `<app config>/api-keys.json`, mode 0600 on macOS | `src-tauri` |
| Speech models | `<app data>/models/` | `mi-transcribe` |
| Provider cache | `<app data>/cache.sqlite` | `mi-sources` |
| History journal (JSON Lines: every rename, copy and folder recorded before and after it happens) | `<app data>/history.jsonl` | `mi-rename` |
| Last update offered (version and time, for "Remind me later") | `<app config>/update-offer.json` | `src-tauri` (`updater.rs`) |
| Saved job results | `<app data>/jobs/` | `mi-core` |

`<app config>` and `<app data>` are Tauri's per-app folders for the identifier
`com.thomaslane.mediaidentifier` (`~/Library/Application Support/com.thomaslane.mediaidentifier`
on macOS, `%APPDATA%\com.thomaslane.mediaidentifier` on Windows). `mi_core::DataPaths` lays them
out.

## File ownership

Modules are developed in parallel. Each file has one owner; only the owner edits it. A module
that needs a change in a file it does not own reports the change to the integrator instead.

| Module | Owns |
|---|---|
| media | `crates/mi-media/**`; `crates/mi-types/src/media.rs`; the "Scanning and play-all detection" section of `docs/identification.md` |
| transcribe | `crates/mi-transcribe/**`; `crates/mi-types/src/transcript.rs`, `crates/mi-types/src/models.rs`; the "Listening" section of `docs/identification.md` |
| sources | `crates/mi-sources/**`; `crates/mi-types/src/catalog.rs`, `crates/mi-types/src/reference.rs`; `docs/sources.md` |
| match | `crates/mi-match/**`; `crates/mi-types/src/matching.rs`; the "Matching" sections of `docs/identification.md` |
| ui | `ui/**` except `ui/src/types/generated/` (generated) and `ui/src/api/tauri.ts` (integrator) |
| release | `crates/mi-rename/**`; `crates/mi-types/src/rename.rs`, `crates/mi-types/src/update.rs`; `src-tauri/src/updater.rs`; the `bundle` and `plugins.updater` sections of `src-tauri/tauri.conf.json`; `.github/workflows/**`; `scripts/build-ffmpeg.sh`; `third_party/ffmpeg/**`; `docs/install.md`; the "Releases", "Updater signing key" and "ffmpeg sidecars" sections of `docs/development.md` |
| branding | `assets/brand/**`; `src-tauri/icons/**`; `docs/images/**` |
| integrator | `crates/mi-core/**`; `crates/mi-types/src/{lib,cancel,error,events,job,settings}.rs` and `crates/mi-types/tests/**`; `src-tauri/src/{lib,main,commands,state,sink,settings_store}.rs`, `src-tauri/build.rs`, `src-tauri/Cargo.toml`, `src-tauri/capabilities/**`, the rest of `src-tauri/tauri.conf.json`; `ui/src/api/tauri.ts`; root `Cargo.toml`, `README.md`, `AGENTS.md`, `CLAUDE.md`, `THIRD_PARTY.md`; `docs/architecture.md`, `docs/glossary.md`, the remaining sections of `docs/development.md`; final consistency of all documents |

Rules for shared files:

- **Types**: a module changes only its own `crates/mi-types/src/` files, keeps changes additive
  where possible, runs `MI_UPDATE_BINDINGS=1 cargo test -p mi-types --test bindings`, and lists
  every type change in its report. The integrator resolves `ui/src/types/generated/index.ts` at
  merge by regenerating.
- **Dependencies**: a module adds third-party dependencies to its own crate's `Cargo.toml` with an
  explicit version, and adds a row to its section of `THIRD_PARTY.md`. The integrator may move
  shared versions into the root `[workspace.dependencies]`.
- **Glossary**: a module adds its terms under its own heading in `docs/glossary.md`; the
  integrator merges headings.
- **Public API**: a crate's public functions and types are the contract other modules build
  against. Changing a signature requires reporting it; adding is free.
