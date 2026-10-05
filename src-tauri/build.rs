//! Build script: ensures sidecar paths exist, then runs `tauri_build`.
//!
//! `tauri.conf.json` lists ffmpeg and ffprobe under `bundle.externalBin`, and `tauri_build`
//! refuses to build unless `binaries/<name>-<target triple>[.exe]` exists. Real binaries come from
//! `scripts/build-ffmpeg.sh` (local) or the release workflow. When they are absent, this script
//! creates empty placeholders so `cargo check`/`cargo test` work on a fresh clone; the app treats
//! a zero-byte sidecar as missing (see `mi_media::Sidecars::resolve`). Setting
//! `MI_REQUIRE_SIDECARS=1` (release builds) turns a missing or empty binary into a build error,
//! so a placeholder can never ship.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=MI_REQUIRE_SIDECARS");
    let target = std::env::var("TARGET").expect("cargo sets TARGET");
    let ext = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let require = std::env::var("MI_REQUIRE_SIDECARS").is_ok_and(|v| v == "1");
    let dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it")).join("binaries");
    for tool in ["ffmpeg", "ffprobe"] {
        let path = dir.join(format!("{tool}-{target}{ext}"));
        println!("cargo:rerun-if-changed={}", path.display());
        let size = std::fs::metadata(&path).map(|m| m.len()).ok();
        match size {
            Some(n) if n > 0 => {}
            _ if require => panic!(
                "MI_REQUIRE_SIDECARS=1 but {} is missing or empty; build it with scripts/build-ffmpeg.sh",
                path.display()
            ),
            Some(_) => {}
            None => {
                std::fs::create_dir_all(&dir).expect("create binaries/");
                std::fs::write(&path, b"").expect("write sidecar placeholder");
            }
        }
    }
    tauri_build::build();
}
