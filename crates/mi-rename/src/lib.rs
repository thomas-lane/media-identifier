//! Saving identification results: naming, rename in place, copy, CSV export and undo.
//!
//! The flow is plan, preview, apply: [`build_plan`] computes every target path and every conflict
//! without touching the disk (the Rename screen shows it as the preview), and [`apply_plan`]
//! carries out a conflict-free plan. Play-all titles and extras are never in a plan's items, so
//! they stay where they are (`mi-core` rebuilds every plan from the job's record before applying
//! it, so a plan sent by the window cannot add them). Nothing is ever overwritten: every move and copy uses an operation
//! that fails when the target exists. Every operation is recorded in the History [`Journal`]
//! before it happens and marked done after, so History can undo it even after a crash.
//!
//! Owner: release module (see `docs/architecture.md`).

pub mod apply;
mod fsops;
pub mod journal;
pub mod naming;
pub mod plan;
pub mod subtitles;

pub use apply::{apply_plan, export_csv};
pub use journal::Journal;
pub use naming::{MAX_COMPONENT_BYTES, render_relative_path, sanitize_component};
pub use plan::{DiskView, PlanContext, build_plan};
pub use subtitles::heard_srt;

/// Errors from this crate.
#[derive(Debug, thiserror::Error)]
pub enum RenameError {
    /// A custom template is invalid (unknown placeholder, absolute path, `..`, no `{ext}`).
    #[error("invalid naming template: {0}")]
    BadTemplate(String),
    /// A decision names an episode that is not in the episode list.
    #[error("episode {0} is not in the episode list")]
    UnknownEpisode(String),
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

#[cfg(test)]
pub(crate) mod test_support {
    //! Builders shared by the unit tests.

    use std::path::Path;

    use mi_types::{
        Confidence, Episode, EpisodeKey, EpisodeOrdering, FileId, FileMatch, FileRole, MediaFile,
        ProviderId, Show, ShowRef, Suggestion, Verdict,
    };

    pub fn show(name: &str, year: Option<u16>) -> Show {
        Show {
            show_ref: ShowRef {
                provider: ProviderId::Tvmaze,
                id: "1".into(),
            },
            name: name.into(),
            year,
            kind: None,
            season_count: None,
            episode_count: None,
            url: None,
        }
    }

    pub fn episode(season: u32, number: u32, title: &str) -> Episode {
        Episode {
            show_ref: ShowRef {
                provider: ProviderId::Tvmaze,
                id: "1".into(),
            },
            ordering: EpisodeOrdering::Aired,
            key: EpisodeKey { season, number },
            title: title.into(),
            runtime_s: None,
            airdate: None,
            summary: None,
            provider_episode_id: format!("{season}-{number}"),
        }
    }

    pub fn media_file(root: &Path, id: &str, role: FileRole) -> MediaFile {
        let mut path = root.to_path_buf();
        for part in id.split('/') {
            path.push(part);
        }
        MediaFile {
            id: FileId(id.into()),
            path,
            file_name: id.rsplit('/').next().unwrap().into(),
            size_bytes: 0,
            probe: None,
            role,
        }
    }

    pub fn file_match(id: &str, verdict: Verdict) -> FileMatch {
        FileMatch {
            file_id: FileId(id.into()),
            suggestion: Suggestion::NotAnEpisode,
            confidence: Confidence {
                score: 0.0,
                margin: 0.0,
                verdict,
            },
            candidates: vec![],
        }
    }
}
