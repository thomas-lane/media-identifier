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
  ├── mi-sources     TVmaze, TMDb, SubDL, LRCLIB, local and embedded text, SQLite cache, HTTP client
  ├── mi-match       text similarity, title hooks, duration, audio alignment, assignment
  └── mi-rename      naming, rename/copy/CSV, History journal
mi-types   ── shared serde types, used by every crate above
```

Each crate depends only on `mi-types` and third-party crates, except `mi-core` (all crates) and
`src-tauri` (`mi-core`, `mi-types`, and `mi-media`/`mi-sources` for two configuration types). The
narrow dependency graph lets each module be built and tested alone, and keeps the matching logic
free of I/O so it can be tested with plain data.

## A job, end to end

The Start and Confirm show screens call `Engine::scan` and `Engine::search_shows`; "Identify"
calls `Engine::start_job`, which runs `mi_core::pipeline::run` on the runtime. The pipeline:

1. **Scan** (`mi_media::scan_folder`): list video files, probe each with ffprobe, mark the play-all
   title (duration close to the sum of the others, chapters matching their lengths), guess the
   show from the folder name, and warn when the play-all has more chapters than short files were
   found or when the folder holds a DVD or Blu-ray folder structure instead of ripped titles. The
   job reuses the scan the Confirm show screen made of the same folder.
2. **Speech model** (`mi_transcribe::WhisperTranscriber::load`): loaded once per job, before
   `Started`, so a missing model fails the job at once with a message pointing to the download.
3. **Episode list** (`Sources::episodes`): TVmaze, or TMDb numbering with a user key, limited to
   the seasons chosen on Confirm show. Without an episode list the job fails.
4. **Reference text** (`Sources::reference_texts`): local files, subtitles, lyrics, summaries; all
   cached. When this stage fails the job continues, because titles and summaries still identify
   files.
5. **Disc order** (`mi_media::stream_audio`, `mi_match::align`): fingerprint each file and the
   play-all (`FingerprintBuilder` takes the audio in chunks), locate each file inside the
   play-all, and decide whether that order is trustworthy. Skipped without a play-all or with
   fewer than two files.
6. **Listening** (`mi_transcribe`), file by file: read the file's embedded text subtitle stream
   when it has one (`mi_media::extract_text_subtitles`), choose windows, decode them to 16 kHz mono
   PCM, transcribe, filter invented text. Each file is then matched on its own and its result sent
   (`Matched`), so Review can start before the other files are heard.
7. **Matching** (`mi_match::match_with_outcome`): score every file against every episode, choose
   the best overall assignment, classify each file as Confident, Check or Extra. Files with a low
   margin (`mi_match::needs_more_listening`) get more windows transcribed, then the rest of the
   file, and everything is matched again. Every file's final result is sent, then `Finished`.
8. **Review and save** (`Engine::plan_rename`, `Engine::apply_rename`, `mi_rename`): the user
   approves; files are renamed in place (journaled for undo), copied, or exported as CSV.

`docs/identification.md` explains steps 5-7; `docs/saving.md` explains step 8.

## The engine and its services

`mi_core::Engine` reaches the outside world through three traits in
`crates/mi-core/src/services.rs`, so a whole job can run in a test without ffmpeg, a network or a
model:

| Trait | In the app | Elsewhere |
|---|---|---|
| `MediaBackend` (scan, decode a window, stream a file, read a subtitle stream) | `FfmpegMedia`, over the sidecars | scripted files in `crates/mi-core/tests/common` |
| `Catalog` (show search, episode list, reference text, source status, keys) | `OnlineCatalog`, over `mi_sources::Sources` | `LocalCatalog` (an episode list given in advance plus local subtitle files) in the end-to-end checks; a scripted catalog in tests |
| `SpeechEngine` and `Listener` (load a model, transcribe a window of a file) | `WhisperEngine`, over `WhisperTranscriber` | a scripted listener in tests |

`Listener` differs from `mi_transcribe::Transcriber` only in being told which file the audio
belongs to, so tests can script a transcript per file; every `Transcriber` is a `Listener`.

Each job's results live in a `JobRecord` (`crates/mi-core/src/jobs.rs`): the `JobResults` the UI
shows, the scanned files and what was heard in each file. The record is filled as the job runs,
so `job_results` returns partial results during a job, and written to
`<app data>/jobs/<job id>.json` when the job ends, so Recent and Review work after a relaunch.
Rename plans are built from the record, not from the window: `apply_rename` builds the plan
again from the request it carries and refuses it when the result differs (see
[saving](saving.md#applying)). `crates/mi-core/examples/identify.rs` runs the same engine from
the command line.

## Threads, cancellation and progress

`mi_core::Engine` runs at most one job. Network calls are async on Tokio (Tauri's own runtime in
the app, passed in as `EngineConfig::runtime`). ffmpeg, whisper.cpp and matching block, so they
run on Tokio's blocking pool. Every long-running function takes a `mi_types::CancelFlag` (a
shared atomic flag) and checks it between units of work; ffmpeg child processes are killed and
whisper.cpp stops through its abort callback. A plain flag is used because whisper.cpp runs on
threads that cannot await an async token.

Progress reaches the UI as `JobEvent`s: `mi_core::EventSink` is implemented by
`src-tauri/src/sink.rs`, which emits Tauri events. The last event of a job is always `Finished`,
`Failed` or `Cancelled`. The engine sends it only after the job's record is saved and the job no
longer counts as running, so a window that reacts to it can read the results and start another
job at once.

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

Three `Backend` methods use Tauri plugins and APIs directly instead of a command:

| Method | Implemented with | Used for |
|---|---|---|
| `chooseSaveFile(defaultPath)` | `save` from `@tauri-apps/plugin-dialog`, CSV filter | the CSV path on the Rename screen |
| `openFile(path)` | `openPath` from `@tauri-apps/plugin-opener` | "Play" on the Review screen |
| `onFileDrop(listener)` | `getCurrentWebview().onDragDropEvent` | dropping a folder on the Start screen |

The web view does not give the page the native path of a dropped file, so drops come from
Tauri's drag-and-drop event (`dragDropEnabled` in `tauri.conf.json`). `openFile` needs the
`opener:allow-open-path` permission in `src-tauri/capabilities/default.json`; its scope lists
only video extensions (`**/*.mkv`, `**/*.mp4` and the others the scan accepts, in lower and
upper case), so the UI can open a video in the default player but cannot launch programs.

In the browser, the mock reads simulation options from the page URL, which helps when checking
screens by hand: `stepMs` and `downloadStepMs` (delay between simulated steps, ms), `model=ready`
(skip the model download), `update=available|upToDate|failed` (the "Check now" answer),
`announceUpdate=<ms>` (simulate the launch-time announcement), and `platform=mac|windows`
(follow that OS's conventions). For example
`http://localhost:5173/?model=ready&stepMs=400&platform=windows`.

## The UI

`ui/src/App.tsx` draws the sidebar (Identify, History, Settings, About) and the current
screen (`ui/src/screens/`). State that outlives a screen lives in React context providers under
`ui/src/state/`:

| Provider | Holds |
|---|---|
| `identify.tsx` | the Identify flow's step (Start, Confirm show, Identifying, Review, Rename), the scan, the job folded from `job-event`s (`job.ts`), and the user's review choices |
| `updates.tsx` | the update dialog, download progress and the "Update downloaded" banner |
| `settings.tsx` | the settings, saved on every change |

Leaving the flow for History or Settings keeps the job running and the review choices intact.
Job events that arrive before `startIdentification` returns the job id are buffered and applied
once the id is known, so the first events of a fast job are never lost. Pure logic (review
choices and counts in `lib/review.ts`, the rename preview in `lib/plan.ts`, formatting, paths,
platform conventions) lives in `ui/src/lib/` and is tested without rendering.

Review choices become `ReviewDecision`s only when the rename plan is requested: a confident
suggestion starts approved, a "Check" suggestion starts pending, and an extra starts as "Not an
episode". Pending files are left untouched by the plan, as are the play-all and extras.

The update flow asks before downloading and never interrupts identification: an update announced
while a job runs is shown when the job ends, and "Relaunch now" is disabled while a job runs.
While an update downloads, "Hide" closes the dialog and lets the download finish, and "Cancel"
stops it (`cancel_update_download`).
Dialog buttons follow the platform: the default button is last on macOS and first on Windows
(`lib/platform.ts`, from the web view's user agent). Light and dark follow the system through
`prefers-color-scheme`; colors are tokens on `:root` in `ui/src/styles.css`.

The speech model download starts by itself the first time the Start screen opens without the
model, because nothing can be identified without it; "Identify" stays disabled until it is ready.
Files are listed without video thumbnails because the ffmpeg sidecars are an audio-only build
with no video decoders.

## Helper executables

ffmpeg and ffprobe are Tauri sidecars (`bundle.externalBin` in `src-tauri/tauri.conf.json`):
Tauri copies `src-tauri/binaries/<name>-<target triple>` next to the app executable.
`mi_media::Sidecars::resolve` looks for each tool in this order: the `MI_FFMPEG`/`MI_FFPROBE`
environment variables, the executable's folder, and (debug builds only) `PATH`. A zero-byte file
counts as missing, because `src-tauri/build.rs` writes empty placeholders when the real binaries
are absent so that the workspace compiles on a fresh clone. Release builds set
`MI_REQUIRE_SIDECARS=1`, which makes `build.rs` fail on a missing or empty binary.

The sidecars are a minimal LGPL build made by `scripts/build-ffmpeg.sh` (see
[development](development.md#ffmpeg-sidecars)). It enables these FFmpeg components, named as
FFmpeg's `configure` names them; `configure` adds the few video filters the ffmpeg program
cannot be built without (`crop`, `format`, `hflip`, `null`, `rotate`, `transpose`, `trim`,
`vflip`):

| Purpose | Components |
|---|---|
| Containers | demuxers `matroska`, `mov` (mp4/m4v/mov), `avi`, `mpegps` (VOB), `mpegts`, and `mpegvideo` (raw MPEG video, which the VOB demuxer uses to recognise DVD video streams) |
| Audio | decoders `ac3`, `eac3`, `aac`, `aac_latm`, `mp1`, `mp2`, `mp3` (and their float variants), `dca` (DTS), `truehd`, `mlp`, `flac`, `opus`, `vorbis`, `alac`, common `pcm_*` (including DVD and Blu-ray PCM); filters `aresample`, `aformat`, `anull`, `atrim`; encoders `pcm_f32le`, `pcm_s16le`; muxers `f32le`, `s16le`, `wav`, `null` |
| Embedded subtitles | decoders `subrip`, `ass`, `ssa`, `webvtt`, `mov_text`, `text`; encoders `subrip`/`srt`; muxer `srt` |
| Timestamps and stream details | parsers `aac`, `aac_latm`, `ac3`, `dca`, `flac`, `mlp`, `mpegaudio`, `opus`, `vorbis`, `h264`, `hevc`, `mpegvideo`, `vc1` |
| I/O | protocols `file`, `pipe` |
| Probing | ffprobe with JSON output |

There are no video decoders: video is never decoded, and ffprobe reads picture sizes from the
container or the parsers. The exact configure options, source version and checksum are in
`third_party/ffmpeg/NOTICE.md`; `docs/development.md` ("ffmpeg sidecars") explains the build.

`mi_media` runs every ffmpeg and ffprobe call through one helper (`crates/mi-media/src/run.rs`):
stdin closed, no console window on Windows (`CREATE_NO_WINDOW`), stdout read on a helper
thread, the last lines of stderr kept for the error message, and the child killed when the
job's `CancelFlag` is set or the call returns early.

## Transcription

`mi-transcribe` turns 16 kHz mono PCM into timed, filtered text. The decoding and filtering rules,
and the reasons for them, are in [identification.md](identification.md#listening); this section
covers how the crate is put together and used.

| Module | Responsibility |
|---|---|
| `catalog.rs` | The three pinned files: both speech models and the VAD model (URL at a fixed commit, size, SHA-256) |
| `store.rs` | `ModelStore`: download, resume and verify files in `<app data>/models/`; report their state |
| `sampling.rs` | `plan_windows`, `escalation_windows`, `use_vad`: which time ranges to transcribe |
| `engine.rs` | `Transcriber` trait, `WhisperTranscriber` (whisper.cpp through `whisper-rs`), `DecodeOptions` |
| `vad.rs` | Joining detected speech into one buffer and mapping times back |
| `filter.rs` | `HallucinationFilter`: marking invented segments |
| `eta.rs` | `SpeedEstimator`: transcription speed and remaining time |
| `lib.rs` | `add_window`: merging each window's segments into one `Transcript` |

**Use in a job.** `mi-core` loads one `WhisperTranscriber` per job (`WhisperTranscriber::load`
with the model from `ModelStore::model_path`, then `with_vad_model` with
`ModelStore::vad_model_path`); loading takes about a second and the decoding state is reused for
every window. For each file it plans windows with `plan_windows`, decodes each window's audio
with `mi_media::extract_audio`, picks `DecodeOptions::for_file`, calls `transcribe` on a blocking
thread with the window start as `start_s` (segment times come back as file times), and adds the
result with `add_window`. When matching reports a low margin it asks `escalation_windows` for
more ranges and repeats. `Transcriber` is a trait so that pipeline tests can script transcripts
without a model.

**Processors.** On macOS whisper.cpp runs on the GPU through Metal (`Accelerator::AppleGpu`), with
its shaders embedded in the binary. On Windows it runs on the CPU; the crate feature `vulkan` adds
a Vulkan GPU backend, used when a Vulkan device is present. If the GPU context cannot be created
the model is loaded on the CPU instead. Flash attention is on in both cases.

**Cancellation.** whisper.cpp polls an abort callback between encoder and decoder passes, so a
cancelled `transcribe` returns `Cancelled` within one pass, and the transcriber stays usable.
`whisper-rs` 0.16's `set_abort_callback_safe` calls the stored closure through the wrong type
unless the closure is passed already boxed as `Box<dyn FnMut() -> bool>`; `engine.rs` does that,
and the ignored test `transcription_is_never_aborted_without_cancellation` guards it.

**Downloads.** `ModelStore::download` fetches the speech model and then the VAD model. Each file
is written to `<file>.part`; a later call, or a retry after a dropped connection, continues it with
an HTTP `Range` request, and a server that answers with the whole file restarts it. A
connection that sends nothing for 60 seconds is dropped and retried; the download fails after
three requests in a row deliver no bytes. When the `.part` file reaches the pinned size it is hashed
with SHA-256; on a match it is renamed to its final name, otherwise it is deleted. A file under
its final name is therefore always complete and verified, which is why `ModelStore::status`
checks only sizes. Progress reaches the caller every 250 ms as a `ModelStatus` (`Downloading`
with the speed over the last three seconds, then `Verifying`, then `Ready`); cancelling keeps the
`.part` file and reports `Paused`. A model counts as `Ready` only when the VAD model is present
too.

**Logging.** whisper.cpp's log output goes to `tracing` under the target `whisper_rs`. On macOS a
model load logs `ggml_metal_library_init_from_source: error compiling source` once: whisper.cpp
is probing for the Metal tensor API, which it then disables; decoding continues on the GPU.

**Time estimates.** `audio_cost_seconds` counts each window as its length but at least 30
seconds, because whisper.cpp encodes audio in 30-second blocks. `SpeedEstimator` starts from a
guess per model and processor and moves to the speed measured on the computer as windows finish.

## Data on disk

| What | Where | Owner crate |
|---|---|---|
| Settings | `<app config>/settings.json` | `src-tauri` (`settings_store.rs`) |
| API keys | `<app config>/api-keys.json`, mode 0600 on macOS | `src-tauri` |
| Speech models and the VAD model (and `.part` files of unfinished downloads) | `<app data>/models/` | `mi-transcribe` |
| Provider cache | `<app data>/cache.sqlite` | `mi-sources` |
| History journal (JSON Lines: every rename, copy and folder recorded before and after it happens) | `<app data>/history.jsonl` | `mi-rename` |
| Last update offered (version and time, for "Remind me later") | `<app config>/update-offer.json` | `src-tauri` (`updater.rs`) |
| Saved jobs (one JSON file per job: results, scanned files, what was heard) | `<app data>/jobs/` | `mi-core` (`jobs.rs`) |

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
