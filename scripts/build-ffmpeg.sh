#!/usr/bin/env bash
# Builds the minimal audio-only LGPL ffmpeg and ffprobe sidecars into src-tauri/binaries/.
#
# Usage: scripts/build-ffmpeg.sh [--target TRIPLE] [--jobs N] [--force] [--print-configure]
#
# Targets:
#   aarch64-apple-darwin    native build on an Apple Silicon Mac (Xcode command-line tools)
#   x86_64-pc-windows-msvc  cross build with mingw-w64 (x86_64-w64-mingw32-gcc on PATH), on macOS
#                           (brew install mingw-w64), Linux (apt install mingw-w64) or MSYS2.
#                           The output is named for the MSVC triple because that is the Rust
#                           target of the Windows app and Tauri looks sidecars up by it; the
#                           executables are self-contained, so the compiler that built them
#                           does not matter to the app.
# The default target is the host's Rust triple (rustc -vV).
#
# What the script guarantees:
#   - The source is the pinned release tarball, accepted only when its SHA-256 matches.
#   - The configure line (printed by --print-configure) must appear verbatim in
#     LICENSES/ffmpeg/NOTICE.md, so the shipped notice always states the exact build.
#   - The build is LGPL-2.1-or-later: no --enable-gpl, --enable-version3 or --enable-nonfree, and
#     --disable-autodetect keeps external libraries out. The script fails if configure enables GPL.
#   - A finished build writes a stamp; rerunning with the same version and flags only recopies the
#     binaries unless --force is given.
set -euo pipefail

FFMPEG_VERSION="9.0.2"
FFMPEG_URL="https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz"
FFMPEG_SHA256="8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC_CACHE="$ROOT/third_party/ffmpeg/src"
BUILD_ROOT="$ROOT/third_party/ffmpeg/build"
BIN_DIR="$ROOT/src-tauri/binaries"
NOTICE="$ROOT/LICENSES/ffmpeg/NOTICE.md"

# Components, matching the table in docs/architecture.md ("Helper executables").
COMMON_FLAGS=(
  --disable-everything
  --disable-autodetect
  --disable-doc
  --disable-debug
  --disable-network
  --disable-ffplay
  --disable-avdevice
  --disable-swscale
  --enable-static
  --disable-shared
  --enable-ffmpeg
  --enable-ffprobe
  --enable-swresample
  --enable-protocol=file,pipe
  --enable-demuxer=matroska,mov,avi,mpegps,mpegts
  '--enable-decoder=ac3,ac3_fixed,eac3,aac,aac_fixed,aac_latm,mp1,mp1float,mp2,mp2float,mp3,mp3float,dca,truehd,mlp,flac,opus,vorbis,alac,pcm_*'
  --enable-decoder=subrip,ass,ssa,webvtt,movtext,text
  --enable-parser=aac,aac_latm,ac3,dca,flac,mlp,mpegaudio,opus,vorbis,mpegvideo,h264,hevc,vc1
  --enable-filter=aresample,aformat,anull,atrim
  --enable-encoder=pcm_f32le,srt
  --enable-muxer=pcm_f32le,srt
)

TARGET=""
JOBS=""
FORCE=0
PRINT_ONLY=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) TARGET="$2"; shift 2 ;;
    --jobs) JOBS="$2"; shift 2 ;;
    --force) FORCE=1; shift ;;
    --print-configure) PRINT_ONLY=1; shift ;;
    -h|--help) sed -n '2,23p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1 (see --help)" >&2; exit 2 ;;
  esac
done

host_triple() { rustc -vV 2>/dev/null | sed -n "s/^host: //p" || true; }
if [[ -z "$TARGET" ]]; then
  TARGET="$(host_triple)"
fi

EXE=""
case "$TARGET" in
  aarch64-apple-darwin)
    TARGET_FLAGS=(
      --arch=arm64
      --cc=clang
      --enable-pthreads
      --extra-cflags=-mmacosx-version-min=11.0
      --extra-ldflags=-mmacosx-version-min=11.0
    )
    ;;
  x86_64-pc-windows-msvc)
    EXE=".exe"
    TARGET_FLAGS=(
      --enable-cross-compile
      --target-os=mingw32
      --arch=x86_64
      --cross-prefix=x86_64-w64-mingw32-
      --enable-w32threads
      --disable-x86asm
      --extra-ldflags=-static
    )
    ;;
  *)
    echo "unsupported target: $TARGET (supported: aarch64-apple-darwin, x86_64-pc-windows-msvc)" >&2
    exit 2
    ;;
esac

CONFIGURE_LINE="./configure ${COMMON_FLAGS[*]} ${TARGET_FLAGS[*]}"
if [[ $PRINT_ONLY -eq 1 ]]; then
  echo "$CONFIGURE_LINE"
  exit 0
fi

if ! grep -qxF "$CONFIGURE_LINE" "$NOTICE"; then
  echo "LICENSES/ffmpeg/NOTICE.md does not contain the configure line for $TARGET." >&2
  echo "Add this exact line to it (the notice must state the shipped build):" >&2
  echo "$CONFIGURE_LINE" >&2
  exit 1
fi

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    sha256sum "$1" | cut -d' ' -f1
  fi
}

if [[ -z "$JOBS" ]]; then
  JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)"
fi

# 1. Source tarball, downloaded once and verified on every run.
TARBALL="$SRC_CACHE/ffmpeg-${FFMPEG_VERSION}.tar.xz"
mkdir -p "$SRC_CACHE"
if [[ ! -f "$TARBALL" ]]; then
  echo "Downloading $FFMPEG_URL"
  curl -fL --retry 3 -o "$TARBALL.part" "$FFMPEG_URL"
  mv "$TARBALL.part" "$TARBALL"
fi
ACTUAL_SHA="$(sha256_of "$TARBALL")"
if [[ "$ACTUAL_SHA" != "$FFMPEG_SHA256" ]]; then
  echo "SHA-256 mismatch for $TARBALL" >&2
  echo "  expected $FFMPEG_SHA256" >&2
  echo "  actual   $ACTUAL_SHA" >&2
  echo "Delete the file and run again; if it persists, the download is not the pinned release." >&2
  exit 1
fi

# 2. Configure and build, unless the stamp says this exact build already exists.
BUILD_DIR="$BUILD_ROOT/$TARGET"
SOURCE_DIR="$BUILD_DIR/ffmpeg-${FFMPEG_VERSION}"
STAMP="$BUILD_DIR/stamp.txt"
STAMP_CONTENT="$FFMPEG_VERSION $FFMPEG_SHA256 $CONFIGURE_LINE"
if [[ $FORCE -eq 1 || ! -f "$STAMP" || "$(cat "$STAMP")" != "$STAMP_CONTENT" ]]; then
  rm -rf "$BUILD_DIR"
  mkdir -p "$BUILD_DIR"
  tar -xJf "$TARBALL" -C "$BUILD_DIR"
  (
    cd "$SOURCE_DIR"
    echo "$CONFIGURE_LINE"
    ./configure "${COMMON_FLAGS[@]}" "${TARGET_FLAGS[@]}" > "$BUILD_DIR/configure.log" 2>&1
    # configure only warns about a misspelled component name and builds without it; fail instead.
    if grep -q "did not match anything" "$BUILD_DIR/configure.log"; then
      grep "did not match anything" "$BUILD_DIR/configure.log" >&2
      exit 1
    fi
    # configure silently drops a program whose dependencies are disabled; fail loudly instead.
    for config in CONFIG_FFMPEG CONFIG_FFPROBE; do
      if ! grep -q "^${config}=yes" ffbuild/config.mak; then
        echo "configure did not enable ${config}; see $BUILD_DIR/configure.log" >&2
        exit 1
      fi
    done
    if grep -q "^CONFIG_GPL=yes" ffbuild/config.mak; then
      echo "configure enabled GPL components; the sidecars must be LGPL" >&2
      exit 1
    fi
    make -j"$JOBS" "ffmpeg${EXE}" "ffprobe${EXE}"
  )
  echo "$STAMP_CONTENT" > "$STAMP"
fi

# 3. Install under the names Tauri's externalBin expects.
mkdir -p "$BIN_DIR"
for program in ffmpeg ffprobe; do
  out="$BIN_DIR/${program}-${TARGET}${EXE}"
  cp "$SOURCE_DIR/${program}${EXE}" "$out"
  case "$TARGET" in
    *-apple-darwin)
      strip -x "$out"
      # Apple Silicon runs only signed code; an ad-hoc signature is enough.
      codesign --force --sign - "$out" >/dev/null 2>&1
      ;;
    *-windows-*)
      x86_64-w64-mingw32-strip "$out"
      ;;
  esac
  echo "Installed $out ($(wc -c < "$out" | tr -d ' ') bytes)"
done

# 4. Smoke test the binaries when they can run on this machine.
if [[ "$(host_triple)" == "$TARGET" ]]; then
  "$BIN_DIR/ffmpeg-${TARGET}${EXE}" -hide_banner -version | head -1
  "$BIN_DIR/ffprobe-${TARGET}${EXE}" -hide_banner -version | head -1
fi
