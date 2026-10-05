//! The History journal, which makes renames undoable.

use std::path::PathBuf;

use mi_types::{HistoryEntry, HistoryId, UndoOutcome};

/// An append-only JSON Lines file (`<app data>/history.jsonl`).
///
/// Records: entry started (id, time, show, folder, mode), operation intended (from, to),
/// operation done, entry undone. An operation intended but not marked done is checked on disk
/// when listed, so an interrupted save still shows exactly what moved.
#[derive(Debug, Clone)]
pub struct Journal {
    path: PathBuf,
}

impl Journal {
    /// Uses the journal file at `path` (created on first write).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// All entries, newest first.
    pub fn list(&self) -> crate::Result<Vec<HistoryEntry>> {
        let _ = &self.path;
        todo!("release module: read journal")
    }

    /// Undoes an entry: moves renamed files back (removing folders that became empty) or deletes
    /// copies whose size still matches. Files changed since are left alone and reported.
    pub fn undo(&self, id: &HistoryId) -> crate::Result<UndoOutcome> {
        let _ = id;
        todo!("release module: undo")
    }
}
