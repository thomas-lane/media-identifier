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
so `job_results` returns partial results during a job. It is written to
`<app data>/jobs/<job id>.json` when the job finishes, or when it fails or is cancelled with at
least one file result, so Recent and Review work after a relaunch; a job that stopped before any
file was matched has nothing to review and is not kept.
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
`Failed` or `Cancelled`. The engine sends it only after the job's record is saved (when it is
kept, as above) and the job no longer counts as running, so a window that reacts to it can read
the results and start another job at once. The pipeline runs as its own Tokio task inside the
job's task, so even a panic (a bug) ends the job with `Failed` and frees the engine instead of
leaving it busy.

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

Five `Backend` methods use Tauri plugins and APIs directly instead of a command:

| Method | Implemented with | Permission in `src-tauri/capabilities/default.json` | Used for |
|---|---|---|---|
| `chooseFolder()` | `open` from `@tauri-apps/plugin-dialog`, folders only | `dialog:allow-open` | picking a folder on the Start screen and the Rename screen |
| `chooseSaveFile(defaultPath)` | `save` from `@tauri-apps/plugin-dialog`, CSV filter | `dialog:allow-save` | the CSV path on the Rename screen |
| `openUrl(url)` | `openUrl` from `@tauri-apps/plugin-opener` | `opener:allow-open-url`, scoped | credit and key links |
| `openFile(path)` | `openPath` from `@tauri-apps/plugin-opener` | `opener:allow-open-path`, scoped | "Play" on the Review screen |
| `onFileDrop(listener)` | `getCurrentWebview().onDragDropEvent` | none (`core:default`) | dropping a folder on the Start screen |

The web view does not give the page the native path of a dropped file, so drops come from
Tauri's drag-and-drop event (`dragDropEnabled` in `tauri.conf.json`).

Both opener permissions are scoped, so the window cannot use them to launch programs or open
arbitrary sites:

- `opener:allow-open-url` lists the sites the window links to: TVmaze, TMDb, SubDL, LRCLIB,
  Creative Commons (the TVmaze data license), whisper.cpp, FFmpeg, Tauri and React, each as its
  bare address and as `<address>/*`. The plugin matches these as glob patterns against the whole
  URL, so `https://www.tvmaze.com/*` does not match `https://www.tvmaze.com.example.org`.
- `opener:allow-open-path` lists `**/*.<ext>` for every extension the scan accepts
  (`mi_media::VIDEO_EXTENSIONS`). Tauri matches file scopes without regard to letter case, so
  `Title_t01.MKV` is covered. `plugins.opener.requireLiteralLeadingDot` is `false` in
  `tauri.conf.json`, so a video inside a folder whose name starts with a dot can be played too.

Two Rust tests in `src-tauri/src/commands.rs` keep these lists complete: every `https://` link in
the UI sources and in `mi_sources::attributions()` must be allowed, and every scanned extension
must be playable.

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
| `identify.tsx` | the Identify flow's step (Start, Confirm show, Identifying, Review, Rename), the scan, the job folded from `job-event`s (`job.ts`), the user's review choices, and the result of saving them |
| `updates.tsx` | the update dialog, download progress and the "Update downloaded" banner |
| `settings.tsx` | the settings, saved on every change |

Leaving the flow for History or Settings keeps the job running, the review choices and the
result of a save intact, so returning shows the same screen. While a job runs, Review offers
"Show progress" and "Cancel identifying". When the flow moves to another step, focus moves to the
new screen's heading (Review focuses its file list instead), so keyboard and screen-reader users
are not left on a button that is gone.
Job events that arrive before `startIdentification` returns the job id are buffered and applied
once the id is known, so the first events of a fast job are never lost. Pure logic (review
choices and counts in `lib/review.ts`, the rename preview in `lib/plan.ts`, formatting, paths,
platform conventions) lives in `ui/src/lib/` and is tested without rendering.

Review choices become `ReviewDecision`s only when the rename plan is requested: a confident
suggestion starts approved, a "Check" suggestion starts pending, and an extra starts as "Not an
episode". Picking another episode in the dropdown makes the file pending until the user presses
Approve, and an approval can be taken back with "Check again", so a slip in the dropdown (on
Windows the arrow keys change a closed list) never renames a file. Pending files are left
untouched by the plan, as are the play-all and extras.

The update flow asks before downloading and never interrupts identification: an update announced
while a job runs is shown when the job ends, and "Relaunch now" is disabled while a job runs.
While an update downloads, "Hide" closes the dialog and lets the download finish, and "Cancel"
stops it (`cancel_update_download`). "Check now" during a hidden download shows its progress
again rather than offering the update a second time, and a hidden download that fails is
reported in the banner instead of a dialog over the user's work.
Dialog buttons follow the platform: the default button is last on macOS and first on Windows
(`lib/platform.ts`, from the web view's user agent). "Back" is not a dialog button but wizard
navigation, so it stays at the left edge on both.

The credits for show and episode data (TVmaze with its CC BY-SA 4.0 license, and TMDb's notice
and logo when its numbering is used) are shown on Confirm show, Review and Rename, and all credits
on About. Their texts and links come from the app (`attributions` command, built from
`mi_sources::attribution`), so the window cannot drift from the source of truth. Light and dark follow the system through
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
[development](development.md#ffmpeg-sidecars)). It enables these FFmpeg components, named exactly
as in the script's lists and FFmpeg's `configure` (the built `ffmpeg -decoders` and `-muxers`
list three of them differently: `movtext` as `mov_text`, `pcm_f32le` and `pcm_s16le` as `f32le`
and `s16le`). `configure` also adds the few video filters the ffmpeg program cannot be built
without (`crop`, `hflip`, `rotate`, `transpose`, `trim`, `vflip`):

| Purpose | Components |
|---|---|
| Containers | demuxers `matroska`, `mov` (mp4/m4v/mov), `avi`, `mpegps` (VOB), `mpegts`, and `mpegvideo` (raw MPEG video, which the VOB demuxer uses to recognise DVD video streams) |
| Audio | decoders `ac3`, `eac3`, `aac`, `aac_latm`, `mp1`, `mp2`, `mp3` (and their float variants), `dca` (DTS), `truehd`, `mlp`, `flac`, `opus`, `vorbis`, `alac`, common `pcm_*` (including DVD and Blu-ray PCM); filters `aresample`, `aformat`, `anull`, `atrim`, and the video filters `format` and `null`, which the ffmpeg program requires; encoders `pcm_f32le`, `pcm_s16le`; muxers `pcm_f32le`, `pcm_s16le`, `wav`, `null` |
| Embedded subtitles | decoders `subrip`, `ass`, `ssa`, `webvtt`, `movtext`, `text`; encoders `subrip`/`srt`; muxer `srt` |
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
its shaders embedded in the binary. On Windows it runs on the CPU; an app built with its `vulkan`
feature (off in releases, see [development](development.md#speech-recognition-whispercpp)) uses a
Vulkan GPU when one is present. If the GPU context cannot be created the model is loaded on the
CPU instead. Flash attention is on in both cases. On x86-64, `WhisperTranscriber::load` first
checks that the processor has the instructions the release build uses (AVX, AVX2, FMA, F16C,
BMI2) and fails with a plain message otherwise, because running without them would end the app
with an illegal-instruction fault.

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

`<app config>` and `<app data>` are Tauri's per-app config folder and local data folder for the
identifier `com.thomaslane.mediaidentifier`. On macOS both are
`~/Library/Application Support/com.thomaslane.mediaidentifier`. On Windows `<app config>` is
`%APPDATA%\com.thomaslane.mediaidentifier` (roaming, so the small settings follow the user) and
`<app data>` is `%LOCALAPPDATA%\com.thomaslane.mediaidentifier`, because the speech models
(about 0.75 GB), cache and History would otherwise be copied to and from a server at every
sign-in on a roaming profile. `mi_core::DataPaths` lays out `<app data>`.
