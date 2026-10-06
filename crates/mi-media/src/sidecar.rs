//! Locating the ffmpeg and ffprobe executables.

use std::path::{Path, PathBuf};

/// The two helper executables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    /// `ffmpeg`: audio decoding and subtitle extraction.
    Ffmpeg,
    /// `ffprobe`: durations, streams and chapters.
    Ffprobe,
}

impl Tool {
    /// The executable's base name (`ffmpeg` / `ffprobe`), without `.exe`.
    pub fn base_name(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::Ffprobe => "ffprobe",
        }
    }

    /// The environment variable that overrides this tool's path (`MI_FFMPEG` / `MI_FFPROBE`).
    pub fn env_var(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "MI_FFMPEG",
            Tool::Ffprobe => "MI_FFPROBE",
        }
    }
}

impl std::fmt::Display for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.base_name())
    }
}

/// Where to look for the tools.
#[derive(Debug, Clone, Default)]
pub struct SidecarLookup {
    /// The directory containing the app executable. Tauri installs `externalBin` sidecars next to
    /// it (`Contents/MacOS/` on macOS, the install folder on Windows), with the target-triple
    /// suffix removed.
    pub exe_dir: Option<PathBuf>,
    /// Also search `PATH` (a Homebrew ffmpeg, for example). Only development builds set this, so
    /// a released app runs the pinned LGPL build it ships unless `MI_FFMPEG`/`MI_FFPROBE` name
    /// another program explicitly.
    pub allow_path_fallback: bool,
}

/// Resolved absolute paths of both tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidecars {
    /// Path to ffmpeg.
    pub ffmpeg: PathBuf,
    /// Path to ffprobe.
    pub ffprobe: PathBuf,
}

impl Sidecars {
    /// Resolves both tools.
    ///
    /// Search order for each tool: the override environment variable ([`Tool::env_var`]), then
    /// `exe_dir`, then (when `allow_path_fallback`) each `PATH` entry. A zero-byte file counts as
    /// missing: `src-tauri/build.rs` creates empty placeholders so the app compiles before real
    /// binaries exist, and those must never be executed.
    pub fn resolve(lookup: &SidecarLookup) -> crate::Result<Sidecars> {
        Ok(Sidecars {
            ffmpeg: resolve_tool(Tool::Ffmpeg, lookup)?,
            ffprobe: resolve_tool(Tool::Ffprobe, lookup)?,
        })
    }

    /// The path of one tool.
    pub fn path(&self, tool: Tool) -> &Path {
        match tool {
            Tool::Ffmpeg => &self.ffmpeg,
            Tool::Ffprobe => &self.ffprobe,
        }
    }
}

fn executable_name(tool: Tool) -> String {
    if cfg!(windows) {
        format!("{}.exe", tool.base_name())
    } else {
        tool.base_name().to_owned()
    }
}

fn usable(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.len() > 0)
        .unwrap_or(false)
}

fn resolve_tool(tool: Tool, lookup: &SidecarLookup) -> crate::Result<PathBuf> {
    let mut searched = Vec::new();
    if let Some(value) = std::env::var_os(tool.env_var()) {
        let path = PathBuf::from(value);
        if usable(&path) {
            return Ok(path);
        }
        searched.push(path.display().to_string());
    }
    let name = executable_name(tool);
    if let Some(dir) = &lookup.exe_dir {
        let path = dir.join(&name);
        if usable(&path) {
            return Ok(path);
        }
        searched.push(path.display().to_string());
    }
    if lookup.allow_path_fallback {
        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths) {
                let path = dir.join(&name);
                if usable(&path) {
                    return Ok(path);
                }
            }
        }
        searched.push("PATH".to_owned());
    }
    Err(crate::MediaError::SidecarMissing {
        tool,
        searched: searched.join(", "),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_dir_wins_and_empty_placeholders_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = dir.path().join(executable_name(Tool::Ffmpeg));
        let ffprobe = dir.path().join(executable_name(Tool::Ffprobe));
        std::fs::write(&ffmpeg, b"binary").unwrap();
        std::fs::write(&ffprobe, b"").unwrap(); // placeholder

        let lookup = SidecarLookup {
            exe_dir: Some(dir.path().to_path_buf()),
            allow_path_fallback: false,
        };
        assert_eq!(resolve_tool(Tool::Ffmpeg, &lookup).unwrap(), ffmpeg);
        let err = resolve_tool(Tool::Ffprobe, &lookup).unwrap_err();
        assert!(matches!(
            err,
            crate::MediaError::SidecarMissing {
                tool: Tool::Ffprobe,
                ..
            }
        ));
    }
}
