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

<!-- owner: release module -->

## Updater signing key

<!-- owner: release module -->

Updates are verified with a minisign key pair created by `tauri signer generate`. The public key
is in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). The private key lives outside the
repository at `~/.tauri/media-identifier-updater.key` (no password) and is given to the release
workflow as a GitHub Actions secret.

## Releases

<!-- owner: release module -->
