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

## Generated TypeScript types

`ui/src/types/generated/` is generated from `crates/mi-types`. After changing a shared type, run
`MI_UPDATE_BINDINGS=1 cargo test -p mi-types --test bindings` and commit the result; the same test
without the variable fails in CI when the files are stale.

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
MI_REQUIRE_SIDECARS=1 TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/media-identifier-updater.key" \
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
  node ui/node_modules/@tauri-apps/cli/tauri.js build --target aarch64-apple-darwin
```

The bundles land in `target/aarch64-apple-darwin/release/bundle/`.
