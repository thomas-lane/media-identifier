# Development

## Prerequisites

- Rust (stable, via rustup; `rust-toolchain.toml` selects the channel and components).
- Node.js 22 or later and npm.
- CMake (whisper.cpp is compiled by the `whisper-rs-sys` build script).
- macOS: Xcode command-line tools. Windows: Visual Studio Build Tools with the C++ workload.
- For running the app in development: `ffmpeg` and `ffprobe` on `PATH` (`brew install ffmpeg`),
  unless the sidecars have been built into `src-tauri/binaries/`.

## Build and test

The commands are listed in [AGENTS.md](../AGENTS.md#commands). CI runs `cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and in `ui/`
`npm ci`, `npm run lint`, `npm test` and `npm run build`.

## Generated TypeScript types

`ui/src/types/generated/` is generated from `crates/mi-types`. After changing a shared type, run
`MI_UPDATE_BINDINGS=1 cargo test -p mi-types --test bindings` and commit the result; the same test
without the variable fails in CI when the files are stale.

## ffmpeg sidecars

<!-- owner: release module; written by the media module -->

The app runs two helper programs, ffmpeg and ffprobe (see
[architecture](architecture.md#helper-executables)). Releases ship a minimal build made by
`scripts/build-ffmpeg.sh` from the pinned FFmpeg source release: only the demuxers, audio and
text-subtitle decoders, filters, encoders and muxers the app uses, no network access, no external
libraries and no GPL parts. Keeping it minimal keeps the download small (about 3.5 MB per program
on macOS) and keeps the license LGPL-2.1-or-later, which allows shipping the programs inside an
MIT app.

```bash
scripts/build-ffmpeg.sh                                   # this Mac: src-tauri/binaries/{ffmpeg,ffprobe}-aarch64-apple-darwin
scripts/build-ffmpeg.sh --target x86_64-pc-windows-msvc   # Windows .exe files, cross-compiled with mingw-w64
scripts/build-ffmpeg.sh --print-configure                 # print the configure line without building
scripts/build-ffmpeg.sh --force                           # rebuild even when the stamp matches
```

The script:

1. downloads `ffmpeg-<version>.tar.xz` into `third_party/ffmpeg/src/` once, and refuses it unless
   its SHA-256 equals the pinned value at the top of the script;
2. refuses to build unless the exact configure line appears in `LICENSES/ffmpeg/NOTICE.md`, so the
   license notice always states the build it describes;
3. configures in `third_party/ffmpeg/build/<target>/` and fails when configure ignores a
   misspelled component name, drops ffmpeg or ffprobe, or enables GPL code;
4. builds only the two programs, strips them (and gives them the ad-hoc signature Apple Silicon
   requires), and copies them to `src-tauri/binaries/<name>-<target triple>[.exe]`, the names
   Tauri's `bundle.externalBin` expects;
5. writes `third_party/ffmpeg/build/<target>/stamp.txt`; a later run with the same version and
   flags skips straight to step 4.

A macOS build takes a few minutes on an M-series Mac and needs the Xcode command-line
tools. The Windows target needs `x86_64-w64-mingw32-gcc` on `PATH`: `brew install mingw-w64` on
macOS, `apt-get install mingw-w64` on Ubuntu, or the MINGW64 shell of MSYS2 on Windows. The
Windows programs are linked statically (`-static`, Windows' own threads), so they use only DLLs
that are part of Windows 10 and later and need nothing installed next to them. They are named for the `x86_64-pc-windows-msvc` triple because that is the Rust
target of the Windows app, which is how Tauri finds them.

`third_party/ffmpeg/src/`, `third_party/ffmpeg/build/` and `src-tauri/binaries/*` are not
committed. Without built sidecars, development builds use `ffmpeg` and `ffprobe` from `PATH`
(`brew install ffmpeg`); released builds use only the bundled programs (see
[AGENTS.md](../AGENTS.md#invariants-keep-tests-for-each)).

**Changing the FFmpeg version or components.** Edit `FFMPEG_VERSION`, `FFMPEG_URL` and
`FFMPEG_SHA256` (check the tarball's `.asc` signature against the FFmpeg release key
`FCF986EA15E6E293A5644F10B4322F04D67658D8` before pinning its hash) or the flag lists in the
script; run `scripts/build-ffmpeg.sh --print-configure` for each target and put the printed lines
into `LICENSES/ffmpeg/NOTICE.md`, together with the new version, URL and hash; update the
component table in `docs/architecture.md`; then build and run the ffmpeg tests against the new
programs:

```bash
MI_REQUIRE_FFMPEG_TESTS=1 cargo test -p mi-media --test ffmpeg
```

**Testing against real ffmpeg.** `crates/mi-media/tests/ffmpeg.rs` generates small files in a
temporary folder (tones whose pitch changes over time, chapters, SubRip, ASS and MP4 text
subtitles, a no-audio file, an unreadable file and a ripped-disc folder with a play-all) and runs
the crate's real command lines on them. It tests `MI_FFMPEG`/`MI_FFPROBE` when both are set, else
the built sidecars for this platform, else `ffmpeg`/`ffprobe` on `PATH`. The fixtures are made
with a full ffmpeg (`MI_TEST_FIXTURE_FFMPEG`, else `ffmpeg` on `PATH`), because the minimal build
has no encoders for them. Without these programs the tests print why and pass without checking,
so `cargo test` works everywhere; `MI_REQUIRE_FFMPEG_TESTS=1` makes a missing program a failure.

**Building the sidecars in CI.** A release build needs the sidecars in `src-tauri/binaries/`
before `tauri-action` runs, with `MI_REQUIRE_SIDECARS=1` set so that a missing sidecar fails the
build instead of shipping an empty placeholder. These jobs produce and check them:

| Job | Runner | Steps |
|---|---|---|
| macOS sidecars | `macos-latest` (arm64) | restore a cache of `third_party/ffmpeg/src` and `src-tauri/binaries/*-aarch64-apple-darwin` keyed on the hash of `scripts/build-ffmpeg.sh`; on a miss run `scripts/build-ffmpeg.sh` |
| Windows sidecars | `ubuntu-latest` | `sudo apt-get install -y mingw-w64`; restore a cache keyed the same way; on a miss run `scripts/build-ffmpeg.sh --target x86_64-pc-windows-msvc` (the script reads the triple from the flag, so it needs no Rust toolchain for this target); upload `src-tauri/binaries/*.exe` as an artifact |
| Windows app | `windows-latest` | download the artifact into `src-tauri/binaries/`; install a full ffmpeg for the fixtures (`choco install ffmpeg`); run `MI_REQUIRE_FFMPEG_TESTS=1 cargo test -p mi-media --test ffmpeg` with `MI_FFMPEG`/`MI_FFPROBE` pointing at the sidecars; then `tauri-action` |

Cross-compiling the Windows programs on Linux avoids building FFmpeg's shell-based configure under
MSYS2 on the Windows runner, which is much slower because the configure script starts thousands of processes. The Windows job runs the ffmpeg tests
because the Linux job cannot execute the `.exe` files it builds.

A release also has to carry the license files and the source: `LICENSES/ffmpeg/NOTICE.md` and
`LICENSES/ffmpeg/COPYING.LGPLv2.1` (the license text from the FFmpeg tarball) belong in the app
bundle as resources, and the pinned source tarball belongs among the release's files, because the
LGPL requires that whoever receives the programs can also get the source they were built from.

## Updater signing key

<!-- owner: release module -->

Updates are verified with a minisign key pair created by `tauri signer generate`. The public key
is in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). The private key lives outside the
repository at `~/.tauri/media-identifier-updater.key` (no password) and is given to the release
workflow as a GitHub Actions secret.

## Releases

<!-- owner: release module -->
