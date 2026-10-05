//! Downloading and verifying model files.

use std::path::{Path, PathBuf};

use mi_types::{CancelFlag, ModelStatus, SpeechModel};

/// The folder holding model files (`<app data>/models`).
///
/// A download writes `<file>.part` and resumes it with an HTTP `Range` request; the finished file
/// is hashed and renamed to its final name only when size and SHA-256 match the pinned values in
/// [`crate::catalog`], so a file under its final name is always complete and verified.
#[derive(Debug, Clone)]
pub struct ModelStore {
    dir: PathBuf,
}

impl ModelStore {
    /// Uses `dir`, creating it on first download.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Final path of a model file (whether or not it exists).
    pub fn model_path(&self, model: SpeechModel) -> PathBuf {
        self.dir.join(crate::model_info(model).file_name)
    }

    /// Current state from disk: `Ready` when the final file exists with the pinned size,
    /// `Paused` when a `.part` exists, otherwise `Missing`. Does not hash (that happens once,
    /// after download).
    pub fn status(&self, model: SpeechModel) -> ModelStatus {
        let _ = model;
        todo!("transcribe module: inspect files on disk")
    }

    /// Downloads (or resumes) a model, calling `on_progress` about every 250 ms with a
    /// `Downloading` status, then `Verifying`, then `Ready`. Returns the final path.
    ///
    /// Sends `User-Agent: MediaIdentifier/<version>`. On cancellation the `.part` file is kept
    /// so the next call resumes. On a checksum mismatch the `.part` file is deleted.
    pub async fn download(
        &self,
        model: SpeechModel,
        on_progress: &(dyn Fn(ModelStatus) + Send + Sync),
        cancel: &CancelFlag,
    ) -> crate::Result<PathBuf> {
        let _ = (model, on_progress, cancel);
        todo!("transcribe module: resumable download with SHA-256 verification")
    }
}
