//! Saving results: rename plans, outcomes and the undo history.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::catalog::EpisodeKey;
use crate::job::JobId;
use crate::matching::ReviewDecision;
use crate::media::FileId;

/// File naming scheme.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NamingScheme {
    /// `<Show> (<Year>)/Season NN/<Show> (<Year>) - SNNEMM - <Title>.<ext>`.
    #[default]
    JellyfinPlex,
    /// Kodi's TV naming.
    Kodi,
    /// A user template; placeholders are documented in `mi-rename`.
    Custom {
        /// The template.
        template: String,
    },
}

/// Ways to save results, without destinations (used as the remembered default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SaveModeKind {
    /// Rename files where they are; undo from History.
    #[default]
    RenameInPlace,
    /// Copy into a new folder; originals untouched.
    CopyToFolder,
    /// Only export a CSV of file to episode.
    ExportList,
}

/// A way to save results, with its destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SaveMode {
    /// Rename in place; new folders are created under `root` (the scanned folder by default).
    RenameInPlace {
        /// Folder the show folder is created in.
        root: PathBuf,
    },
    /// Copy into `destination`.
    CopyToFolder {
        /// Destination folder.
        destination: PathBuf,
    },
    /// Write a CSV to `destination`.
    ExportList {
        /// CSV file path.
        destination: PathBuf,
        /// Replace a file already at `destination`. The window sets it only for a path the user
        /// picked in the save dialog, which asked before replacing; a typed path never replaces
        /// a file.
        #[serde(default)]
        replace: bool,
    },
}

/// The user's decision for one file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileDecision {
    /// The file.
    pub file_id: FileId,
    /// The decision.
    pub decision: ReviewDecision,
}

/// What the Rename screen sends to build a preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenamePlanRequest {
    /// The job whose results are saved.
    pub job_id: JobId,
    /// One decision per file the user saw.
    pub decisions: Vec<FileDecision>,
    /// How to save.
    pub mode: SaveMode,
    /// Naming scheme.
    pub naming: NamingScheme,
    /// Also write `.srt` files from what was heard.
    pub save_heard_subtitles: bool,
}

/// One planned rename or copy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenameItem {
    /// The file.
    pub file_id: FileId,
    /// Current absolute path.
    pub from: PathBuf,
    /// New absolute path.
    pub to: PathBuf,
    /// The episode.
    pub episode: EpisodeKey,
    /// Episode title.
    pub title: String,
    /// Path of the `.srt` to write from what was heard, when requested.
    pub heard_subtitles_to: Option<PathBuf>,
    /// Size of the file when it was scanned. Saving leaves a file whose size differs alone,
    /// because a different file may now have the scanned name.
    pub size_bytes: u64,
}

/// Why a file is left where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum UntouchedReason {
    /// The play-all title.
    PlayAll,
    /// Not an episode.
    Extra,
    /// The user chose Skip, or never approved a Check file.
    Skipped,
}

/// A file the plan leaves alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UntouchedFile {
    /// The file.
    pub file_id: FileId,
    /// Its path.
    pub path: PathBuf,
    /// Why.
    pub reason: UntouchedReason,
}

/// A problem that blocks applying a plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PlanConflict {
    /// The target path already exists on disk.
    TargetExists {
        /// The file.
        file_id: FileId,
        /// The target.
        path: PathBuf,
    },
    /// Two files would get the same target (two files approved as the same episode).
    DuplicateTarget {
        /// The files.
        file_ids: Vec<FileId>,
        /// The shared target.
        path: PathBuf,
    },
    /// The file is no longer at its scanned path, or has a different size: it was moved,
    /// renamed or replaced since it was identified (for example by an earlier save of the same
    /// job, or by a new rip with the same file names).
    SourceChanged {
        /// The file.
        file_id: FileId,
        /// Its scanned path.
        path: PathBuf,
    },
    /// The CSV file of an export already exists and was not chosen in the save dialog.
    ListExists {
        /// The CSV file path.
        path: PathBuf,
    },
}

/// A preview of what saving will do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenamePlan {
    /// The job.
    pub job_id: JobId,
    /// The request the plan was built from. Applying builds the plan again from this request
    /// and the job's own record, and refuses when the result differs from this plan.
    pub request: RenamePlanRequest,
    /// How to save.
    pub mode: SaveMode,
    /// Planned renames or copies, sorted by target path.
    pub items: Vec<RenameItem>,
    /// Files left alone.
    pub untouched: Vec<UntouchedFile>,
    /// Conflicts; a plan with conflicts cannot be applied.
    pub conflicts: Vec<PlanConflict>,
}

/// Identifier of a History entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct HistoryId(pub String);

/// A file operation that failed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OperationFailure {
    /// The file.
    pub file_id: FileId,
    /// Path that was being moved or copied.
    pub path: PathBuf,
    /// Plain-language reason.
    pub message: String,
}

/// The result of applying a plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenameOutcome {
    /// The History entry recorded for this save (present for renames and copies).
    pub history_id: Option<HistoryId>,
    /// Files renamed, copied or listed.
    pub completed: u32,
    /// Operations that failed; completed ones stay done and are listed in History.
    pub failed: Vec<OperationFailure>,
}

/// One completed operation in a History entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    /// Original path.
    pub from: PathBuf,
    /// New path.
    pub to: PathBuf,
}

/// One save, as listed on the History screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// Identifier.
    pub id: HistoryId,
    /// When it was saved, Unix milliseconds.
    pub created_at_ms: i64,
    /// Show name, for display.
    pub show_name: String,
    /// Folder the files were in.
    pub folder: PathBuf,
    /// How it was saved.
    pub mode: SaveModeKind,
    /// Completed operations, in the order performed.
    pub items: Vec<HistoryItem>,
    /// When it was undone, if it was.
    pub undone_at_ms: Option<i64>,
}

/// The result of undoing a History entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UndoOutcome {
    /// Files restored (renames moved back, copies removed).
    pub restored: u32,
    /// Operations that could not be undone (for example, the file was moved again since).
    pub failed: Vec<OperationFailure>,
}
