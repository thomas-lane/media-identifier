# Development

## Prerequisites

- Rust (stable, via rustup; `rust-toolchain.toml` selects the channel and components).
- Node.js 22 or later and npm.
- CMake (whisper.cpp is compiled by the `whisper-rs-sys` build script).
- macOS: Xcode command-line tools. Windows: Visual Studio Build Tools with the C++ workload.
- For running the app in development: `ffmpeg` and `ffprobe` on `PATH` (`brew install ffmpeg`),
  unless the sidecars have been built into `src-tauri/binaries/`.

## Build and test

The commands are listed in [AGENTS.md](../AGENTS.md#commands). CI
(`.github/workflows/ci.yml`) runs on every push to `main` and every pull request, on
`macos-latest` and `windows-latest`: in `ui/` `npm ci`, `npm run lint`, `npm test` and
`npm run build`, then `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`
and `cargo test --workspace`. A third job on Linux lints the workflow files with
[actionlint](https://github.com/rhysd/actionlint) and runs `scripts/build-ffmpeg.sh --check-notice`.
Run `actionlint` locally (`brew install actionlint`) after editing a workflow.

## End-to-end checks

Two checks run the whole pipeline with real ffmpeg and real speech models. They are not part of
`cargo test --workspace`, because they need macOS, model files and (for the second) real episodes.

**Synthetic disc.** `scripts/make-synthetic-disc.sh <folder>` writes a fictional eight-episode
show spoken by macOS's `say` (each episode opens with the same instrumental theme; two have a tone
and one a chord under the speech), the separate titles numbered in a shuffled order, a play-all
with one chapter per episode, a bonus clip, reference subtitles of which three are paraphrased,
the episode list as JSON and `truth.json` with the right answers. It needs a full ffmpeg
(Homebrew's), because the sidecars have no encoders. The ignored test runs the script and
identifies the result with the Fast model, failing unless every file is right:

```bash
MI_TEST_MODEL_DIR=<folder with ggml-small.en-q5_1.bin and ggml-silero-v5.1.2.bin> \
  cargo test -p mi-core --test synthetic_disc -- --ignored --nocapture
```

`MI_TEST_MODEL_DIR` is the folder the `mi-transcribe` real-model tests download into
(`target/tmp/models` by default); `MI_SYNTHETIC_DIR` reuses a disc made earlier.

**Real files.** `crates/mi-core/examples/identify.rs` identifies any folder with the app's engine
and prints each file's suggestion, verdict, score and margin, and with `--truth` whether it is
right:

```bash
cargo run --release -p mi-core --example identify -- <folder> --tvmaze <TVmaze show id> \
  --season 1 --references <folder of .srt files> --model fast --models <model folder> \
  --truth <truth.json>
```

`--show <show.json> --episodes <episodes.json>` replaces TVmaze with an episode list in
`mi-types` JSON (as the synthetic disc writes it). `--references` reads subtitle files named with
an `S01E02`-style marker as reference text before any online source. Copy real episodes to a
scratch folder first and give them neutral names (`title_t00.mp4`, ...), so nothing in a file name
gives the answer away; the example only reads the files.

## Generated TypeScript types

`ui/src/types/generated/` is generated from `crates/mi-types`. After changing a shared type, run
`MI_UPDATE_BINDINGS=1 cargo test -p mi-types --test bindings` and commit the result; the same test
without the variable fails in CI when the files are stale.

## Speech recognition (whisper.cpp)

<!-- owner: transcribe module -->

`whisper-rs-sys` compiles the whisper.cpp sources it bundles (version 1.8.3) with CMake and links
them statically; the version is printed by `mi_transcribe::whisper_cpp_version()`. Its build
script also generates Rust bindings with `bindgen`, which needs libclang; when libclang is
missing it prints a warning and uses bindings shipped with the crate. whisper.cpp is compiled
with optimisations even in debug builds, because unoptimised inference is unusably slow.
Environment variables whose names start with `WHISPER_`, `GGML_` or `CMAKE_` are passed to CMake
as definitions, which is how the settings below are made.

**macOS.** The `metal` feature of `whisper-rs` is always on for macOS (see
`crates/mi-transcribe/Cargo.toml`), and the Metal shaders are embedded in the binary, so nothing
extra is shipped.

**Release builds: `GGML_NATIVE=OFF`.** By default ggml compiles its CPU code for the processor of
the build machine. A release built that way on a CI runner with AVX-512 crashes with an illegal
instruction on computers without it. With `GGML_NATIVE=OFF`, x64 builds target SSE 4.2, AVX,
AVX2, BMI2, FMA and F16C (Intel since 2013, AMD since 2015) and leave AVX-512 off; Apple clang
targets the Apple M1 instruction set, which every Apple Silicon Mac supports. Set it in the
environment of every release build.

**macOS release builds link `libclang_rt.osx.a`.** whisper.cpp's Metal code checks the macOS
version at run time. Built for an older macOS than the SDK (release builds target macOS 11), clang
turns each check into a call to `__isPlatformVersionAtLeast`, which lives in clang's runtime
library; rustc links with `-nodefaultlibs`, so `crates/mi-transcribe/build.rs` adds that library
(found with `xcrun clang --print-runtime-dir`). Debug builds target the running macOS and make no
such calls.

**Windows.** The build needs the Visual Studio C++ build tools and CMake (both installed on
GitHub's `windows-latest` image). The speech model runs on the CPU.

**Vulkan (Windows, optional).** `cargo build -p mi-transcribe --features vulkan` adds a Vulkan GPU
backend. It needs:

- The Vulkan SDK, with `VULKAN_SDK` pointing at it and `glslc` on `PATH`: whisper.cpp compiles its
  GPU shaders during the build, which adds many minutes. In GitHub Actions,
  `jakoch/install-vulkan-sdk-action@v1` (with `cache: true`) installs it and sets `VULKAN_SDK`.
- libclang, because the shipped fallback bindings lack the Vulkan functions the crate uses to
  list devices. LLVM is installed on `windows-latest`; set `LIBCLANG_PATH` to its `bin` folder if
  `bindgen` cannot find it.

A build with this feature links against `vulkan-1.dll`, which GPU drivers install. Windows
refuses to start a program whose DLL is missing, so on a computer without a Vulkan driver (for
example a virtual machine) such a build does not launch at all. When a Vulkan driver is present
but reports no device, the model runs on the CPU.

**Tests and measurements.** `cargo test -p mi-transcribe` needs no network and no model. The tests
in `crates/mi-transcribe/tests/real_model.rs` download the real models from Hugging Face (Fast
190 MB, Accurate 574 MB), generate speech with the macOS `say` command and transcribe it; they are
ignored by default:

```bash
cargo test -p mi-transcribe --test real_model -- --ignored --nocapture --test-threads=1
```

They keep the models in `MI_TEST_MODEL_DIR` (default `target/tmp/models`). A fresh download is
cancelled after 5 MB and resumed, which exercises the real redirect and `Range` handling. To time
transcription of any 16 kHz mono WAV file:

```bash
cargo run --release -p mi-transcribe --example transcribe -- <model.bin> <audio.wav> [--cpu] [--vad <ggml-silero-v5.1.2.bin>]
```

## ffmpeg sidecars

<!-- owner: release module -->

The app runs `ffmpeg` and `ffprobe` as separate programs ([sidecars](glossary.md#general)) that
are bundled next to its executable. `scripts/build-ffmpeg.sh` builds them from a pinned FFmpeg
source tarball (version and SHA-256 at the top of the script) into
`src-tauri/binaries/ffmpeg-<target triple>` and `ffprobe-<target triple>` (`.exe` on Windows):

| Target | Where to run it | Needs |
|---|---|---|
| `aarch64-apple-darwin` | a Mac with Apple Silicon | Xcode command-line tools |
| `x86_64-pc-windows-msvc` | an [MSYS2](https://www.msys2.org) MINGW64 shell | `pacman -S make curl diffutils tar xz mingw-w64-x86_64-gcc mingw-w64-x86_64-nasm mingw-w64-x86_64-binutils` |

```bash
scripts/build-ffmpeg.sh                    # build for this computer (about a minute on Apple Silicon)
scripts/build-ffmpeg.sh --verify           # check built sidecars without rebuilding
scripts/build-ffmpeg.sh --print-configure  # the configure options for this computer
scripts/build-ffmpeg.sh --check-notice     # third_party/ffmpeg/NOTICE.md matches the script
```

The build is minimal on purpose. `--disable-everything` and an explicit list of demuxers, decoders,
parsers, filters, encoders and muxers (the components in
[architecture.md](architecture.md#helper-executables)) keep each program between 3 and 4.5 MB.
Leaving out `--enable-gpl` and `--enable-nonfree` keeps it under the LGPL, which the app's MIT
license can ship alongside. `--disable-autodetect` keeps libraries that happen to be installed on
the build computer out of the programs, so they link only operating system libraries and run on
any supported system. Homebrew's ffmpeg is not shipped for these reasons: it is a GPL build that
links dozens of Homebrew libraries. Development builds still fall back to it on `PATH` when the
sidecars are missing.

The build ends with the same checks as `--verify`: every required component is listed by the
built `ffmpeg`, both programs report the LGPL and no GPL or non-free option, and they link only
system libraries (`otool -L` on macOS; `objdump -p` on Windows, allowing Windows system DLLs only).

Earlier, the build also fails when `configure` ignores a misspelled component name, drops
ffmpeg or ffprobe, or enables GPL code. The source tarball is downloaded once into
`third_party/ffmpeg/src/` and refused unless its SHA-256 matches; `third_party/ffmpeg/src/`,
`third_party/ffmpeg/build/` and `src-tauri/binaries/*` are not committed.

**Testing against real ffmpeg.** `crates/mi-media/tests/ffmpeg.rs` generates small files in a
temporary folder (tones whose pitch changes over time, chapters, SubRip, ASS and MP4 text
subtitles, a no-audio file, an unreadable file and a ripped-disc folder with a play-all) and runs
the crate's real command lines on them. It tests `MI_FFMPEG`/`MI_FFPROBE` when both are set, else
the built sidecars for this platform, else `ffmpeg`/`ffprobe` on `PATH`. The fixtures are made
with a full ffmpeg (`MI_TEST_FIXTURE_FFMPEG`, else `ffmpeg` on `PATH`), because the minimal build
has no encoders for them. Without these programs the tests print why and pass without checking,
so `cargo test` works everywhere (CI included, where no sidecars are built);
`MI_REQUIRE_FFMPEG_TESTS=1` makes a missing program a failure:

```bash
MI_REQUIRE_FFMPEG_TESTS=1 cargo test -p mi-media --test ffmpeg
```

To change the components, edit the lists at the top of the script, then update the option lines in
`third_party/ffmpeg/NOTICE.md` (`--check-notice` compares them line by line, in CI and in the
release workflow) and the component table in `docs/architecture.md`. The release workflow caches
the built sidecars under a key derived from the script's contents, so any change to the script
rebuilds them.

The LGPL asks that people who receive the programs can get their source and replace them. The app
bundles `third_party/ffmpeg/NOTICE.md` (version, source, checksum, configure options, how to
replace the programs) and FFmpeg's `COPYING.LGPLv2.1` under `licenses/ffmpeg/` in its resources
(`bundle.resources` in `src-tauri/tauri.conf.json`), and the release workflow attaches the source
tarball to every release.

## Updater signing key

<!-- owner: release module -->

Updates are verified with a minisign key pair created by `tauri signer generate`. The public key
is in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`) and is compiled into the app. The
private key lives outside the repository at `~/.tauri/media-identifier-updater.key` (no password)
and is given to the release workflow as the GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY`:

```bash
gh secret set TAURI_SIGNING_PRIVATE_KEY --repo thomas-lane/media-identifier < ~/.tauri/media-identifier-updater.key
```

The secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` stays unset, because this key has no password
(GitHub does not store empty secrets, and an unset secret reaches the workflow as an empty
password). A key generated with a password needs that secret too.

**Back up the private key** somewhere other than this computer, such as a password manager. An
installed copy of the app accepts only updates signed with the key whose public half it was built
with. If the private key is lost, no installed copy can ever be updated again; every user would
have to download and install a new version by hand. To replace the key while it is still
available: generate a new pair, put the new public key in `tauri.conf.json`, publish one release
signed with the old key (so installed copies accept it and receive the new public key), then
replace the secret with the new private key for later releases.

Never commit the private key or print it in a log; `.gitignore` excludes `*.key`.

## Releases

<!-- owner: release module -->

Builds are not code-signed on either system. On macOS the bundler gives the app an ad-hoc
signature (`bundle.macOS.signingIdentity: "-"`), which Apple Silicon requires to run any code;
it identifies no developer, so macOS still asks the user to confirm the first launch
([install.md](install.md)).

To publish a version:

1. Set the version in the root `Cargo.toml` (`[workspace.package] version`), which is the app
   version Tauri uses, and the same in `ui/package.json`. Commit.
2. Tag the commit with an annotated tag whose message is the release notes, and push the tag.
   The message becomes the "What's new" text of the update dialog and the top of the release
   page (a lightweight tag gets the text "Media Identifier <version>"):

   ```bash
   git tag -a v0.2.0 -m "Kodi naming." -m "Fixed: very short files were skipped."
   git push origin v0.2.0
   ```

3. `.github/workflows/release.yml` runs:
   - **prepare** (Linux) fails unless the tag equals `v` + the `Cargo.toml` version, the
     `TAURI_SIGNING_PRIVATE_KEY` secret is set, and `--check-notice` passes. It reads the notes
     from the tag, creates a draft release for the tag (or reuses the draft a previous run
     created) and attaches the FFmpeg source tarball.
   - **build** runs once on `macos-latest` for `aarch64-apple-darwin` and once on
     `windows-latest` for `x86_64-pc-windows-msvc`. It restores or builds the ffmpeg sidecars,
     then [tauri-action](https://github.com/tauri-apps/tauri-action) runs `tauri build` with
     `MI_REQUIRE_SIDECARS=1` (a missing or empty sidecar fails the build), signs the update files
     with the private key, uploads them to the draft, and merges its platform into the
     release's `latest.json` together with the notes.
4. Check the draft. It holds the macOS `.dmg`, the macOS update archive (`.app.tar.gz` and its
   `.sig`), the Windows installer (`-setup.exe`, per-user, no administrator rights) and its
   `.sig`, `latest.json` and `ffmpeg-<version>.tar.xz`. Editing the release page later does not
   change `latest.json`; to change the notes the update dialog shows, edit its `notes` field and
   upload it again (the signatures cover the downloads, not this file).
5. Publish the draft. The app's update endpoint is
   `https://github.com/thomas-lane/media-identifier/releases/latest/download/latest.json`, and
   GitHub serves `releases/latest` from the newest published release that is not a draft or
   pre-release, so installed copies see the version only once it is published.

The repository and its releases are private for now. GitHub answers requests for a private
repository's release files only when they carry an access token, and the app sends none, so
every update check fails until the repository is public: background checks log the failure and
"Check now" shows "Couldn't check for updates". Putting a token into the app is not an option,
because anyone with a copy could read it.

A release build can be made locally the same way (the signing key variables are needed only
because `createUpdaterArtifacts` is on):

```bash
scripts/build-ffmpeg.sh
CI=true GGML_NATIVE=OFF MI_REQUIRE_SIDECARS=1 \
  TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/media-identifier-updater.key" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
  node ui/node_modules/@tauri-apps/cli/tauri.js build --target aarch64-apple-darwin
```

`CI=true` makes the `.dmg` step skip arranging its Finder window, which otherwise waits for a
Finder automation permission prompt; GitHub Actions sets it by itself.

The bundles land in `target/aarch64-apple-darwin/release/bundle/`.
