# Sidecar binaries

`ffmpeg-<target triple>[.exe]` and `ffprobe-<target triple>[.exe]` live here. They are not
committed. `scripts/build-ffmpeg.sh` builds them locally; the release workflow builds them in CI.
When they are missing, `src-tauri/build.rs` writes empty placeholders so the app compiles, and the
app falls back to an `ffmpeg` on `PATH` in development builds only.
