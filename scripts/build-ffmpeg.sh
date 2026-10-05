#!/usr/bin/env bash
# Builds the minimal audio-only LGPL ffmpeg/ffprobe sidecars into src-tauri/binaries/.
# The pinned source, configure line and LGPL notices are documented in docs/development.md
# ("ffmpeg sidecars"). Owner: release module.
set -euo pipefail
echo "scripts/build-ffmpeg.sh: the pinned ffmpeg build is not written yet." >&2
echo "Development builds use an ffmpeg/ffprobe on PATH (for example: brew install ffmpeg)." >&2
exit 1
