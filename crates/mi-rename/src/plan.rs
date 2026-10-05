//! Building a rename plan.

use std::path::Path;

use mi_types::{Episode, FileMatch, MediaFile, RenamePlan, RenamePlanRequest, Show};

/// What a plan is built from (supplied by `mi-core` from the job's results).
#[derive(Debug, Clone, Copy)]
pub struct PlanContext<'a> {
    /// The show.
    pub show: &'a Show,
    /// All candidate episodes.
    pub episodes: &'a [Episode],
    /// The scanned files.
    pub files: &'a [MediaFile],
    /// The job's match results (identifies the play-all).
    pub matches: &'a [FileMatch],
}

/// Builds the plan for `request`.
///
/// Approved files get a `RenameItem`; the play-all, files decided `NotAnEpisode`, `Skip` and
/// still-`Pending` files are listed in `untouched`. `exists` reports whether a path exists (the
/// real file system in the app, a set in tests); a target that exists, or two items with the same
/// target, become conflicts. For `ExportList` the targets are still computed (they fill the CSV).
pub fn build_plan(
    context: PlanContext<'_>,
    request: &RenamePlanRequest,
    exists: &dyn Fn(&Path) -> bool,
) -> crate::Result<RenamePlan> {
    let _ = (context, request, exists);
    todo!("release module: rename plan")
}
