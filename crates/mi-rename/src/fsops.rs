//! File system operations that never replace an existing file.
//!
//! `std::fs::rename` silently replaces an existing target on macOS and Windows, and a separate
//! "does it exist?" check before it leaves a window in which another program can create the
//! target. So moves use the operating system's own no-replace rename, which checks and renames
//! in one step: `renamex_np(RENAME_EXCL)` on macOS and `MoveFileExW` without
//! `MOVEFILE_REPLACE_EXISTING` on Windows. Both are reached through FFI, the only `unsafe` code
//! in this crate. When a volume does not support the exclusive rename (some network shares
//! return `ENOTSUP` on macOS), the move falls back to checking and then renaming.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Whether something (a file, folder or link) exists at `path`, without following links.
pub(crate) fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Moves `from` to `to`, failing with `AlreadyExists` when `to` exists.
pub(crate) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    match platform::rename_exclusive(from, to) {
        Ok(true) => Ok(()),
        Ok(false) => {
            if exists(to) {
                Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "the target already exists",
                ))
            } else {
                fs::rename(from, to)
            }
        }
        Err(e) => Err(e),
    }
}

/// A free name in `dir`: `.mi-<tag>.<suffix>`, with a counter added when that name is taken.
/// The leading dot hides it on macOS while it exists.
pub(crate) fn free_temp_name(dir: &Path, tag: &str, suffix: &str) -> PathBuf {
    let mut candidate = dir.join(format!(".mi-{tag}.{suffix}"));
    let mut n = 1;
    while exists(&candidate) {
        candidate = dir.join(format!(".mi-{tag}-{n}.{suffix}"));
        n += 1;
    }
    candidate
}

/// A plain-language description of a failed file operation.
pub(crate) fn describe(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::AlreadyExists => "a file with that name already exists".to_owned(),
        io::ErrorKind::NotFound => "the file or folder is no longer there".to_owned(),
        io::ErrorKind::PermissionDenied => "permission was denied".to_owned(),
        io::ErrorKind::CrossesDevices => {
            "the destination is on a different drive; use Copy into a new folder instead".to_owned()
        }
        io::ErrorKind::StorageFull => "the disk is full".to_owned(),
        io::ErrorKind::ReadOnlyFilesystem => "the disk is read-only".to_owned(),
        _ if platform::is_sharing_violation(error) => {
            "the file is open in another program".to_owned()
        }
        _ => error.to_string(),
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod platform {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    /// `Ok(true)` when renamed, `Ok(false)` when the volume does not support exclusive renames.
    pub fn rename_exclusive(from: &Path, to: &Path) -> io::Result<bool> {
        let from = CString::new(from.as_os_str().as_bytes())?;
        let to = CString::new(to.as_os_str().as_bytes())?;
        // SAFETY: both arguments are valid NUL-terminated strings that outlive the call.
        let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
        if result == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOTSUP) {
            Ok(false)
        } else {
            Err(error)
        }
    }

    pub fn is_sharing_violation(_: &io::Error) -> bool {
        false
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    use std::ffi::OsString;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    /// Converts an absolute path to the `\\?\` form, which lifts the 260-character limit, and to
    /// a NUL-terminated wide string.
    fn wide(path: &Path) -> Vec<u16> {
        let text = path.as_os_str().to_string_lossy().replace('/', "\\");
        let long = if text.starts_with(r"\\?\") {
            text
        } else if let Some(unc) = text.strip_prefix(r"\\") {
            format!(r"\\?\UNC\{unc}")
        } else if path.is_absolute() {
            format!(r"\\?\{text}")
        } else {
            text
        };
        OsString::from(long).encode_wide().chain(Some(0)).collect()
    }

    /// `Ok(true)` when renamed. Windows always supports the exclusive form.
    pub fn rename_exclusive(from: &Path, to: &Path) -> io::Result<bool> {
        let from = wide(from);
        let to = wide(to);
        // SAFETY: both arguments are valid NUL-terminated wide strings that outlive the call.
        let ok = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) };
        if ok != 0 {
            Ok(true)
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn is_sharing_violation(error: &io::Error) -> bool {
        error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32)
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use std::io;
    use std::path::Path;

    /// Other systems are not release targets; they use the check-then-rename fallback.
    pub fn rename_exclusive(_: &Path, _: &Path) -> io::Result<bool> {
        Ok(false)
    }

    pub fn is_sharing_violation(_: &io::Error) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_refuses_to_replace_and_leaves_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mkv");
        let b = dir.path().join("b.mkv");
        fs::write(&a, b"aaa").unwrap();
        fs::write(&b, b"bb").unwrap();
        let err = rename_no_replace(&a, &b).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&a).unwrap(), b"aaa");
        assert_eq!(fs::read(&b).unwrap(), b"bb");
    }

    #[test]
    fn rename_moves_when_the_target_is_free() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mkv");
        let c = dir.path().join("c.mkv");
        fs::write(&a, b"aaa").unwrap();
        rename_no_replace(&a, &c).unwrap();
        assert!(!a.exists());
        assert_eq!(fs::read(&c).unwrap(), b"aaa");
    }

    #[test]
    fn temp_names_skip_taken_names() {
        let dir = tempfile::tempdir().unwrap();
        let first = free_temp_name(dir.path(), "x", "tmp");
        assert_eq!(first.file_name().unwrap(), ".mi-x.tmp");
        fs::write(&first, b"").unwrap();
        let second = free_temp_name(dir.path(), "x", "tmp");
        assert_eq!(second.file_name().unwrap(), ".mi-x-1.tmp");
    }
}
