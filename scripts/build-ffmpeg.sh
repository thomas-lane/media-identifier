#!/usr/bin/env bash
# Builds the minimal audio-only LGPL ffmpeg and ffprobe sidecars into src-tauri/binaries/.
# Owner: release module. Background: docs/development.md, "ffmpeg sidecars".
#
# Usage: scripts/build-ffmpeg.sh [--target <triple>]
#            [--verify | --print-configure | --check-notice | --fetch-source]
#
#   --target <triple>   aarch64-apple-darwin (run on a Mac with Apple Silicon) or
#                       x86_64-pc-windows-msvc (run in an MSYS2 MINGW64 shell). Default: this computer.
#   --verify            only check already-built sidecars (components, licence, linked libraries).
#   --print-configure   print the configure options for the target and exit.
#   --check-notice      check that third_party/ffmpeg/NOTICE.md states this script's version,
#                       checksum and configure options, then exit.
#   --fetch-source      download and check the source tarball, print its path, then exit.
#
# The source tarball is pinned by version and SHA-256. Building without --enable-gpl and
# --enable-nonfree keeps the binaries under the LGPL; --disable-autodetect keeps libraries
# installed on the build computer out of them, so they link only operating system libraries.
set -euo pipefail

FFMPEG_VERSION="9.0.2"
FFMPEG_SHA256="8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e"
FFMPEG_URL="https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="${ROOT}/third_party/ffmpeg"
NOTICE="${WORK}/NOTICE.md"
OUT="${ROOT}/src-tauri/binaries"

# Components the app needs (docs/architecture.md, "Helper executables"), as configure names.
PROTOCOLS="file,pipe"
DEMUXERS="matroska,mov,avi,mpegps,mpegts,mpegvideo"
AUDIO_DECODERS="aac,aac_latm,ac3,eac3,mp2,mp2float,mp3,mp3float,dca,truehd,mlp,flac,opus,vorbis,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_f32le,pcm_u8,pcm_dvd,pcm_bluray,pcm_alaw,pcm_mulaw"
SUBTITLE_DECODERS="subrip,ass,ssa,webvtt,movtext,text"
ENCODERS="pcm_f32le,pcm_s16le,subrip,srt"
MUXERS="pcm_f32le,pcm_s16le,wav,srt,null"
PARSERS="aac,aac_latm,ac3,dca,flac,mlp,mpegaudio,opus,vorbis,h264,hevc,mpegvideo,vc1"
FILTERS="aresample,aformat,anull,atrim,format,null"

# The same components as the built ffmpeg lists them (some names differ from configure's).
LISTED_DEMUXERS="${DEMUXERS/mpegps/mpeg}"
LISTED_DECODERS="${AUDIO_DECODERS},${SUBTITLE_DECODERS/movtext/mov_text}"
LISTED_MUXERS="f32le,s16le,wav,srt,null"

common_options() {
  cat <<EOF
--disable-everything
--disable-autodetect
--disable-doc
--disable-debug
--disable-network
--disable-ffplay
--disable-avdevice
--disable-swscale
--enable-ffmpeg
--enable-ffprobe
--enable-static
--disable-shared
--enable-protocol=${PROTOCOLS}
--enable-demuxer=${DEMUXERS}
--enable-decoder=${AUDIO_DECODERS},${SUBTITLE_DECODERS}
--enable-encoder=${ENCODERS}
--enable-muxer=${MUXERS}
--enable-parser=${PARSERS}
--enable-filter=${FILTERS}
EOF
}

# --disable-autodetect also turns off thread detection, so each target names its thread library.
target_options() {
  case "$1" in
    aarch64-apple-darwin)
      cat <<EOF
--arch=aarch64
--cc=clang
--enable-pthreads
--extra-cflags=-mmacosx-version-min=11.0
--extra-ldflags=-mmacosx-version-min=11.0
EOF
      ;;
    x86_64-pc-windows-msvc)
      cat <<EOF
--arch=x86_64
--target-os=mingw32
--enable-w32threads
--extra-ldflags=-static
EOF
      ;;
    *)
      echo "unsupported target: $1 (use aarch64-apple-darwin or x86_64-pc-windows-msvc)" >&2
      exit 2
      ;;
  esac
}

host_target() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) echo "aarch64-apple-darwin" ;;
    MINGW64_NT*-x86_64) echo "x86_64-pc-windows-msvc" ;;
    *)
      echo "no default target for $(uname -s) $(uname -m); pass --target" >&2
      exit 2
      ;;
  esac
}

exe_suffix() {
  case "$1" in *windows*) echo ".exe" ;; *) echo "" ;; esac
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 "$1" | cut -d ' ' -f 1
  fi
}

cpu_count() {
  getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4
}

check_notice() {
  local missing=0 line
  # Version lines appear inside list items; each configure option must be a line of its own.
  while IFS= read -r line; do
    if ! grep -qF -- "${line}" "${NOTICE}"; then
      echo "third_party/ffmpeg/NOTICE.md does not state: ${line}" >&2
      missing=1
    fi
  done < <(
    echo "Version: ${FFMPEG_VERSION}"
    echo "SHA-256: ${FFMPEG_SHA256}"
    echo "Source: ${FFMPEG_URL}"
  )
  while IFS= read -r line; do
    if ! grep -qxF -- "${line}" "${NOTICE}"; then
      echo "third_party/ffmpeg/NOTICE.md does not list the option: ${line}" >&2
      missing=1
    fi
  done < <(
    common_options
    target_options aarch64-apple-darwin
    target_options x86_64-pc-windows-msvc
  )
  if [ "${missing}" -ne 0 ]; then
    echo "Update the notice to match scripts/build-ffmpeg.sh." >&2
    exit 1
  fi
  echo "third_party/ffmpeg/NOTICE.md matches the build script."
}

# Checks that `<bin> -<kind>` lists every name in a comma-separated list.
check_listed() {
  local bin="$1" kind="$2" names="$3" listing name missing=0
  listing="$("${bin}" -hide_banner "-${kind}" 2>/dev/null | tr -d '\r')"
  for name in ${names//,/ }; do
    if ! printf '%s\n' "${listing}" | grep -qE "^ +[A-Z.|]+ +${name}( |,|$)"; then
      echo "$(basename "${bin}") is missing ${kind%s} ${name}" >&2
      missing=1
    fi
  done
  return "${missing}"
}

verify() {
  local target="$1" ext ffmpeg ffprobe failed=0 bin deps
  ext="$(exe_suffix "${target}")"
  ffmpeg="${OUT}/ffmpeg-${target}${ext}"
  ffprobe="${OUT}/ffprobe-${target}${ext}"
  for bin in "${ffmpeg}" "${ffprobe}"; do
    if [ ! -s "${bin}" ]; then
      echo "missing or empty: ${bin}" >&2
      exit 1
    fi
  done

  check_listed "${ffmpeg}" demuxers "${LISTED_DEMUXERS}" || failed=1
  check_listed "${ffmpeg}" decoders "${LISTED_DECODERS}" || failed=1
  check_listed "${ffmpeg}" encoders "${ENCODERS}" || failed=1
  check_listed "${ffmpeg}" muxers "${LISTED_MUXERS}" || failed=1
  check_listed "${ffmpeg}" filters "${FILTERS}" || failed=1
  for bin in "${ffmpeg}" "${ffprobe}"; do
    if ! "${bin}" -hide_banner -L 2>/dev/null | grep -q "GNU Lesser General Public"; then
      echo "$(basename "${bin}") is not LGPL-licensed" >&2
      failed=1
    fi
    if "${bin}" -hide_banner -buildconf 2>/dev/null | grep -qE -- "--enable-(gpl|nonfree)"; then
      echo "$(basename "${bin}") was configured with GPL or non-free components" >&2
      failed=1
    fi
  done

  # Only operating system libraries may be linked, so the sidecars run on any supported system.
  case "${target}" in
    *apple-darwin)
      for bin in "${ffmpeg}" "${ffprobe}"; do
        deps="$(otool -L "${bin}" | tail -n +2 | awk '{print $1}' |
          grep -vE '^(/usr/lib/|/System/Library/)' || true)"
        if [ -n "${deps}" ]; then
          echo "$(basename "${bin}") links non-system libraries: ${deps}" >&2
          failed=1
        fi
      done
      ;;
    *windows*)
      for bin in "${ffmpeg}" "${ffprobe}"; do
        deps="$(objdump -p "${bin}" | sed -n 's/^[[:space:]]*DLL Name: //p' | tr -d '\r' |
          grep -viE '^(kernel32|user32|gdi32|advapi32|bcrypt|ole32|oleaut32|shell32|shlwapi|ws2_32|psapi|msvcrt|ucrtbase|api-ms-win-[a-z0-9-]+)\.dll$' || true)"
        if [ -n "${deps}" ]; then
          echo "$(basename "${bin}") links non-system DLLs: ${deps}" >&2
          failed=1
        fi
      done
      ;;
  esac

  if [ "${failed}" -ne 0 ]; then
    exit 1
  fi
  echo "Verified $(basename "${ffmpeg}") and $(basename "${ffprobe}"): $("${ffmpeg}" -hide_banner -version | head -n 1 | tr -d '\r')"
}

# Downloads the pinned source tarball unless it is already there, and checks its SHA-256.
fetch_source() {
  local tarball="${WORK}/src/ffmpeg-${FFMPEG_VERSION}.tar.xz" actual
  mkdir -p "${WORK}/src"
  if [ ! -f "${tarball}" ] || [ "$(sha256_of "${tarball}")" != "${FFMPEG_SHA256}" ]; then
    echo "Downloading ${FFMPEG_URL}" >&2
    curl --fail --location --retry 3 --continue-at - --output "${tarball}.part" "${FFMPEG_URL}" >&2
    mv "${tarball}.part" "${tarball}"
  fi
  actual="$(sha256_of "${tarball}")"
  if [ "${actual}" != "${FFMPEG_SHA256}" ]; then
    echo "checksum mismatch for ${tarball}: expected ${FFMPEG_SHA256}, got ${actual}" >&2
    rm -f "${tarball}"
    exit 1
  fi
  echo "${tarball}"
}

build() {
  local target="$1" ext src build tarball line
  ext="$(exe_suffix "${target}")"
  src="${WORK}/src/ffmpeg-${FFMPEG_VERSION}"
  build="${WORK}/build/${target}"
  mkdir -p "${OUT}"
  tarball="$(fetch_source)"
  rm -rf "${src}"
  tar -xJf "${tarball}" -C "${WORK}/src"

  local options=()
  while IFS= read -r line; do options+=("${line}"); done < <(common_options; target_options "${target}")
  rm -rf "${build}"
  mkdir -p "${build}"
  (
    cd "${build}"
    "${src}/configure" "${options[@]}"
    make -j"$(cpu_count)"
  )
  # Replace (not overwrite) any placeholder src-tauri/build.rs created, which is not executable.
  rm -f "${OUT}/ffmpeg-${target}${ext}" "${OUT}/ffprobe-${target}${ext}"
  cp "${build}/ffmpeg${ext}" "${OUT}/ffmpeg-${target}${ext}"
  cp "${build}/ffprobe${ext}" "${OUT}/ffprobe-${target}${ext}"
  chmod 755 "${OUT}/ffmpeg-${target}${ext}" "${OUT}/ffprobe-${target}${ext}"
  case "${target}" in
    *apple-darwin)
      strip -x "${OUT}/ffmpeg-${target}" "${OUT}/ffprobe-${target}"
      # Re-sign after stripping, so the ad-hoc signature Apple Silicon requires matches the file.
      codesign --force --sign - "${OUT}/ffmpeg-${target}" "${OUT}/ffprobe-${target}"
      ;;
    *windows*) strip "${OUT}/ffmpeg-${target}${ext}" "${OUT}/ffprobe-${target}${ext}" ;;
  esac
  verify "${target}"
}

main() {
  local target="" mode="build"
  while [ $# -gt 0 ]; do
    case "$1" in
      --target)
        target="${2:?--target needs a value}"
        shift 2
        ;;
      --verify) mode="verify"; shift ;;
      --print-configure) mode="print"; shift ;;
      --check-notice) mode="notice"; shift ;;
      --fetch-source) mode="fetch"; shift ;;
      -h | --help) sed -n '2,18p' "${BASH_SOURCE[0]}"; exit 0 ;;
      *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
  done
  case "${mode}" in
    notice) check_notice; return ;;
    fetch) fetch_source; return ;;
  esac
  [ -n "${target}" ] || target="$(host_target)"
  case "${mode}" in
    print) common_options; target_options "${target}" ;;
    verify) verify "${target}" ;;
    build) build "${target}" ;;
  esac
}

main "$@"
