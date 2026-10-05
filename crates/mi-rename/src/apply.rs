//! Carrying out a plan.

use std::path::Path;

use mi_types::{RenameOutcome, RenamePlan};

use crate::Journal;

/// Applies a plan without conflicts: renames (creating folders) or copies each item, writing the
/// journal entry before each operation and marking it done after. Never overwrites an existing
/// file (rechecked immediately before each operation). Failed operations are reported and the
/// rest continue.
pub fn apply_plan(
    plan: &RenamePlan,
    show_name: &str,
    journal: &Journal,
) -> crate::Result<RenameOutcome> {
    let _ = (plan, show_name, journal);
    todo!("release module: apply plan")
}

/// Writes the plan as CSV with the header
/// `file,season,episode,title,new_name` and one row per item, UTF-8 with a BOM so Excel on
/// Windows reads accents correctly.
pub fn export_csv(plan: &RenamePlan, destination: &Path) -> crate::Result<RenameOutcome> {
    let _ = (plan, destination);
    todo!("release module: CSV export")
}
