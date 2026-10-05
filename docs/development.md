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

## Updater signing key

<!-- owner: release module -->

Updates are verified with a minisign key pair created by `tauri signer generate`. The public key
is in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). The private key lives outside the
repository at `~/.tauri/media-identifier-updater.key` (no password) and is given to the release
workflow as a GitHub Actions secret.

## Releases

<!-- owner: release module -->
