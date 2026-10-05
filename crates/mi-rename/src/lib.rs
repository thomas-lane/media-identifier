//! Saving identification results: naming, rename in place, copy, CSV export and undo.
//!
//! Play-all titles and extras are never renamed; they stay where they are. Every rename and copy
//! is recorded in a journal before it happens, so History can undo it even after a crash.
//!
//! Owner: release module (see `docs/architecture.md`).

pub mod apply;
pub mod journal;
pub mod naming;
pub mod plan;

pub use apply::{apply_plan, export_csv};
pub use journal::Journal;
pub use naming::{render_relative_path, sanitize_component};
pub use plan::{PlanContext, build_plan};

/// Errors from this crate.
#[derive(Debug, thiserror::Error)]
pub enum RenameError {
    /// A custom template is invalid (unknown placeholder, absolute path, `..`).
    #[error("invalid naming template: {0}")]
    BadTemplate(String),
    /// The plan has conflicts and cannot be applied.
    #[error("the plan has {0} conflicts")]
    Conflicts(usize),
    /// A history entry was not found.
    #[error("history entry {0} not found")]
    NotFound(String),
    /// The journal could not be read or written.
    #[error("history journal error: {0}")]
    Journal(String),
    /// A file system error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// CSV writing failed.
    #[error(transparent)]
    Csv(#[from] csv::Error),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, RenameError>;
