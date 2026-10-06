//! Where the app keeps its files.

use std::path::{Path, PathBuf};

/// Files under the app's data folder (Tauri's `app_local_data_dir`, for example
/// `~/Library/Application Support/com.thomaslane.mediaidentifier` on macOS and
/// `%LOCALAPPDATA%\com.thomaslane.mediaidentifier` on Windows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPaths {
    /// The data folder.
    pub root: PathBuf,
    /// Speech model files.
    pub models: PathBuf,
    /// The SQLite cache of provider data.
    pub cache_db: PathBuf,
    /// The History journal.
    pub history: PathBuf,
    /// Saved job results (one JSON file per job), for Recent and for reopening Review.
    pub jobs: PathBuf,
}

impl DataPaths {
    /// Lays out the files under `root`. Creates nothing.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            models: root.join("models"),
            cache_db: root.join("cache.sqlite"),
            history: root.join("history.jsonl"),
            jobs: root.join("jobs"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_lives_under_the_root() {
        let p = DataPaths::new(Path::new("/data"));
        for path in [&p.models, &p.cache_db, &p.history, &p.jobs] {
            assert!(path.starts_with("/data"));
        }
    }
}
