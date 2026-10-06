//! Carrying out a plan: renames, copies, subtitle files and the CSV export.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use mi_types::{
    FileId, OperationFailure, RenameItem, RenameOutcome, RenamePlan, SaveMode, SaveModeKind,
    UntouchedReason,
};

use crate::Journal;
use crate::fsops::{describe, exists, free_temp_name, modified_ns, modified_of, rename_no_replace};
use crate::journal::{Action, EntryWriter, Intent, Phase};
use crate::plan::scan_root;

/// Applies a plan without conflicts.
///
/// - Rename in place: moves every item in two phases. First each file is renamed to a temporary
///   name in its own folder, then each temporary file is renamed to its target (creating
///   folders). The two phases let files swap names and change only the case of their names. A
///   file whose target cannot be taken goes back to its original name.
/// - Copy into a folder: copies each file to a temporary name next to its target, checks the
///   copy's size, then renames it to the target. Originals are never changed.
/// - Export list: writes the CSV ([`export_csv`]); no files change.
///
/// Every move and copy uses a rename that fails when the target exists, so nothing is
/// overwritten even if another program creates a file after the plan was built. Every operation
/// is recorded in `journal` before it happens. When an item has `heard_subtitles_to` and
/// `heard_subtitles` returns text for its file, a new `.srt` file is written there (never over an
/// existing file) after the video is in place; [`crate::heard_srt`] formats it.
///
/// A file whose size is not the scanned size ([`RenameItem::size_bytes`]) is left alone and
/// reported as failed: the name may now belong to a different file.
///
/// Failed operations are reported in [`RenameOutcome::failed`] and the rest continue. An error is
/// returned only when the plan has conflicts or the journal cannot be written. When the journal
/// fails during a rename, files already moved to temporary names are moved back to their
/// original names first (the error says how many), so no file is left under a hidden name; the
/// operations already done are in the journal.
pub fn apply_plan(
    plan: &RenamePlan,
    show_name: &str,
    journal: &Journal,
    heard_subtitles: &dyn Fn(&FileId) -> Option<String>,
) -> crate::Result<RenameOutcome> {
    if !plan.conflicts.is_empty() {
        return Err(crate::RenameError::Conflicts(plan.conflicts.len()));
    }
    let kind = match &plan.mode {
        SaveMode::ExportList {
            destination,
            replace,
        } => return export_csv(plan, destination, *replace),
        SaveMode::RenameInPlace { .. } => SaveModeKind::RenameInPlace,
        SaveMode::CopyToFolder { .. } => SaveModeKind::CopyToFolder,
    };
    let folder = scanned_folder(plan).unwrap_or_default();
    let (_guard, mut writer) = journal.start(show_name, &folder, kind)?;
    let mut failed = Vec::new();
    let mut placed: Vec<&RenameItem> = Vec::new();

    match kind {
        SaveModeKind::RenameInPlace => {
            let mut moves = Vec::new();
            for (i, item) in plan.items.iter().enumerate() {
                match fs::metadata(&item.from) {
                    Ok(meta) if meta.is_file() && meta.len() != item.size_bytes => {
                        failed.push(failure(item, &item.from, CHANGED.into()))
                    }
                    Ok(meta) if meta.is_file() => {
                        if item.from == item.to {
                            placed.push(item);
                        } else {
                            moves.push(PlannedMove {
                                item: i as u32,
                                file_id: item.file_id.clone(),
                                from: item.from.clone(),
                                to: item.to.clone(),
                                size: meta.len(),
                                modified_ns: modified_of(&meta),
                            });
                        }
                    }
                    Ok(_) => failed.push(failure(item, &item.from, "it is not a file".into())),
                    Err(e) => failed.push(failure(item, &item.from, describe(&e))),
                }
            }
            let moved = move_set(&mut writer, Phase::Apply, moves, &mut failed)?;
            for m in moved {
                placed.push(&plan.items[m.item as usize]);
            }
        }
        SaveModeKind::CopyToFolder => {
            for (i, item) in plan.items.iter().enumerate() {
                if copy_one(&mut writer, i as u32, item, &mut failed)? {
                    placed.push(item);
                }
            }
        }
        SaveModeKind::ExportList => unreachable!("handled above"),
    }

    let completed = placed.len() as u32;
    placed.sort_by(|a, b| a.to.cmp(&b.to));
    for item in placed {
        write_subtitles(&mut writer, item, heard_subtitles, &mut failed)?;
    }
    Ok(RenameOutcome {
        history_id: writer
            .has_begun()
            .then(|| mi_types::HistoryId(writer.entry().to_owned())),
        completed,
        failed,
    })
}

/// Writes the plan as CSV to `destination`.
///
/// The CSV is written to a temporary file in the same folder and then renamed to `destination`,
/// so a crash never leaves a half-written list. With `replace`, the rename replaces an existing
/// file (the user confirmed that in the save dialog); without it, an existing file is never
/// replaced and the export fails with `AlreadyExists`.
///
/// Columns: `file` (the file's path relative to the scanned folder), `status` (`episode`,
/// `play-all`, `extra` or `skipped`), `season`, `episode`, `title`, and `new_name` (the target
/// relative to the save root, with `/` between folders). One row per scanned file, sorted by
/// `file`. The file is UTF-8 with a byte order mark so Excel on Windows reads accents correctly.
/// A cell that begins with `=`, `+`, `-`, `@`, a tab or a carriage return gets a leading `'`, so
/// a spreadsheet shows a title such as `=Hello` as text instead of running it as a formula.
pub fn export_csv(
    plan: &RenamePlan,
    destination: &Path,
    replace: bool,
) -> crate::Result<RenameOutcome> {
    let root = match &plan.mode {
        SaveMode::RenameInPlace { root } => Some(root.clone()),
        SaveMode::CopyToFolder { destination } => Some(destination.clone()),
        SaveMode::ExportList { .. } => None,
    };
    let mut rows: Vec<[String; 6]> = Vec::new();
    for item in &plan.items {
        let base = root
            .clone()
            .unwrap_or_else(|| scan_root(&item.from, &item.file_id));
        let new_name = match item.to.strip_prefix(&base) {
            Ok(rel) => rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/"),
            Err(_) => item.to.to_string_lossy().into_owned(),
        };
        rows.push([
            item.file_id.0.clone(),
            "episode".into(),
            item.episode.season.to_string(),
            item.episode.number.to_string(),
            item.title.clone(),
            new_name,
        ]);
    }
    for file in &plan.untouched {
        let status = match file.reason {
            UntouchedReason::PlayAll => "play-all",
            UntouchedReason::Extra => "extra",
            UntouchedReason::Skipped => "skipped",
        };
        rows.push([
            file.file_id.0.clone(),
            status.into(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]);
    }
    rows.sort_by(|a, b| a[0].cmp(&b[0]));

    let mut out = csv::Writer::from_writer(Vec::from("\u{feff}".as_bytes()));
    out.write_record(["file", "status", "season", "episode", "title", "new_name"])?;
    for row in &rows {
        out.write_record(row.iter().map(|cell| as_text(cell)))?;
    }
    let bytes = out
        .into_inner()
        .map_err(|e| crate::RenameError::Io(e.into_error()))?;
    let folder = match destination.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let temp = free_temp_name(folder, &format!("list-{}", std::process::id()), "part");
    let written = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        if replace {
            fs::rename(&temp, destination)
        } else {
            rename_no_replace(&temp, destination)
        }
    })();
    if let Err(e) = written {
        let _ = fs::remove_file(&temp);
        return Err(e.into());
    }
    Ok(RenameOutcome {
        history_id: None,
        completed: plan.items.len() as u32,
        failed: Vec::new(),
    })
}

fn as_text(cell: &str) -> String {
    if cell.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{cell}")
    } else {
        cell.to_owned()
    }
}

fn scanned_folder(plan: &RenamePlan) -> Option<PathBuf> {
    plan.items
        .first()
        .map(|i| scan_root(&i.from, &i.file_id))
        .or_else(|| {
            plan.untouched
                .first()
                .map(|u| scan_root(&u.path, &u.file_id))
        })
}

fn failure(item: &RenameItem, path: &Path, message: String) -> OperationFailure {
    OperationFailure {
        file_id: item.file_id.clone(),
        path: path.to_path_buf(),
        message,
    }
}

/// Why a file whose size differs from the scan is left alone.
const CHANGED: &str = "it changed since it was identified (a different file may have this name now), so it was left as it is";

/// One file to move, with the size and modification time recorded for verification.
#[derive(Debug, Clone)]
pub(crate) struct PlannedMove {
    pub item: u32,
    pub file_id: FileId,
    pub from: PathBuf,
    pub to: PathBuf,
    pub size: u64,
    pub modified_ns: Option<i64>,
}

/// Moves a set of files in two phases (all to temporary names, then all to their targets),
/// journaling every step. A file that cannot reach its target is moved back to where it started.
/// Returns the moves that completed; failures are appended to `failed`.
///
/// When the journal cannot be written, the files still under temporary names are moved back to
/// where they started before the error is returned, because nothing else would find them: the
/// temporary names are hidden and are not video files.
pub(crate) fn move_set(
    writer: &mut EntryWriter,
    phase: Phase,
    moves: Vec<PlannedMove>,
    failed: &mut Vec<OperationFailure>,
) -> crate::Result<Vec<PlannedMove>> {
    let mut pending: Vec<(PlannedMove, PathBuf)> = Vec::new();
    let mut moved = Vec::new();
    match move_set_steps(writer, phase, moves, failed, &mut pending, &mut moved) {
        Ok(()) => Ok(moved),
        Err(e) => Err(put_back(writer, phase, pending, e)),
    }
}

/// The two phases of [`move_set`]. `pending` holds the files under temporary names at every
/// point where an error can be returned.
fn move_set_steps(
    writer: &mut EntryWriter,
    phase: Phase,
    moves: Vec<PlannedMove>,
    failed: &mut Vec<OperationFailure>,
    pending: &mut Vec<(PlannedMove, PathBuf)>,
    moved: &mut Vec<PlannedMove>,
) -> crate::Result<()> {
    let tag = match phase {
        Phase::Apply => "a",
        Phase::Undo => "u",
    };
    for m in moves {
        let dir = m.from.parent().unwrap_or(Path::new("."));
        let temp = free_temp_name(dir, &format!("{}-{tag}{}", writer.entry(), m.item), "tmp");
        let op = writer.intent(move_intent(phase, &m, &m.from, &temp))?;
        match rename_no_replace(&m.from, &temp) {
            Ok(()) => {
                pending.push((m, temp));
                writer.done(op)?;
            }
            Err(e) => {
                writer.failed(op)?;
                failed.push(OperationFailure {
                    file_id: m.file_id.clone(),
                    path: m.from.clone(),
                    message: describe(&e),
                });
            }
        }
    }

    while let Some((m, temp)) = pending.first().cloned() {
        let reached = match m.to.parent() {
            Some(parent) => ensure_dir(writer, phase, parent),
            None => Ok(()),
        };
        let reached = match reached {
            Ok(()) => {
                let op = writer.intent(move_intent(phase, &m, &temp, &m.to))?;
                match rename_no_replace(&temp, &m.to) {
                    Ok(()) => {
                        pending.remove(0);
                        writer.done(op)?;
                        Ok(())
                    }
                    Err(e) => {
                        writer.failed(op)?;
                        Err(e)
                    }
                }
            }
            Err(e)
                if e.get_ref()
                    .is_some_and(|inner| inner.is::<crate::RenameError>()) =>
            {
                // ensure_dir could not write the journal.
                return Err(crate::RenameError::Journal(e.to_string()));
            }
            Err(e) => Err(e),
        };
        match reached {
            Ok(()) => moved.push(m),
            Err(e) => {
                let reason = describe(&e);
                let op = writer.intent(move_intent(phase, &m, &temp, &m.from))?;
                pending.remove(0);
                let message = match rename_no_replace(&temp, &m.from) {
                    Ok(()) => {
                        writer.done(op)?;
                        format!("could not move it to {}: {reason}", m.to.display())
                    }
                    Err(back) => {
                        writer.failed(op)?;
                        format!(
                            "could not move it to {}: {reason}; it is now named {} ({}), and History can undo it",
                            m.to.display(),
                            temp.display(),
                            describe(&back)
                        )
                    }
                };
                failed.push(OperationFailure {
                    file_id: m.file_id.clone(),
                    path: m.from.clone(),
                    message,
                });
            }
        }
    }
    Ok(())
}

/// After a journal error, moves the files in `pending` back from their temporary names to where
/// they started (journaling it while the journal accepts records), and returns the error with
/// what happened to them.
fn put_back(
    writer: &mut EntryWriter,
    phase: Phase,
    pending: Vec<(PlannedMove, PathBuf)>,
    error: crate::RenameError,
) -> crate::RenameError {
    let reason = match error {
        crate::RenameError::Journal(message) => message,
        other => other.to_string(),
    };
    if pending.is_empty() {
        return crate::RenameError::Journal(reason);
    }
    let mut journal_ok = true;
    let mut restored = 0;
    let mut stranded = Vec::new();
    for (m, temp) in pending {
        let op = if journal_ok {
            writer
                .intent(move_intent(phase, &m, &temp, &m.from))
                .inspect_err(|_| journal_ok = false)
                .ok()
        } else {
            None
        };
        let result = rename_no_replace(&temp, &m.from);
        if let Some(op) = op {
            let recorded = match &result {
                Ok(()) => writer.done(op),
                Err(_) => writer.failed(op),
            };
            journal_ok &= recorded.is_ok();
        }
        match result {
            Ok(()) => restored += 1,
            Err(_) => stranded.push(temp.display().to_string()),
        }
    }
    let mut message = format!("{reason}; {restored} files were moved back to their original names");
    if !stranded.is_empty() {
        message.push_str(&format!(
            "; these could not be moved back and keep a temporary name: {}",
            stranded.join(", ")
        ));
    }
    crate::RenameError::Journal(message)
}

fn move_intent(phase: Phase, m: &PlannedMove, from: &Path, to: &Path) -> Intent {
    let mut intent = Intent::new(Action::Move, phase, to);
    intent.item = Some(m.item);
    intent.file_id = Some(m.file_id.clone());
    intent.from = Some(from.to_path_buf());
    intent.size_bytes = Some(m.size);
    intent.modified_ns = m.modified_ns;
    intent
}

/// Creates `dir` and any missing parents, journaling each folder created so undo can remove it.
pub(crate) fn ensure_dir(writer: &mut EntryWriter, phase: Phase, dir: &Path) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut cursor = Some(dir);
    while let Some(d) = cursor {
        if d.as_os_str().is_empty() || exists(d) {
            break;
        }
        missing.push(d.to_path_buf());
        cursor = d.parent();
    }
    for d in missing.into_iter().rev() {
        let op = writer
            .intent(Intent::new(Action::Mkdir, phase, &d))
            .map_err(io::Error::other)?;
        match fs::create_dir(&d) {
            Ok(()) => writer.done(op).map_err(io::Error::other)?,
            Err(e) => {
                writer.failed(op).map_err(io::Error::other)?;
                if e.kind() != io::ErrorKind::AlreadyExists {
                    return Err(e);
                }
            }
        }
    }
    Ok(())
}

/// Copies one item; returns whether the copy is in place.
fn copy_one(
    writer: &mut EntryWriter,
    index: u32,
    item: &RenameItem,
    failed: &mut Vec<OperationFailure>,
) -> crate::Result<bool> {
    let size = match fs::metadata(&item.from) {
        Ok(meta) if meta.is_file() && meta.len() != item.size_bytes => {
            failed.push(failure(item, &item.from, CHANGED.into()));
            return Ok(false);
        }
        Ok(meta) if meta.is_file() => meta.len(),
        Ok(_) => {
            failed.push(failure(item, &item.from, "it is not a file".into()));
            return Ok(false);
        }
        Err(e) => {
            failed.push(failure(item, &item.from, describe(&e)));
            return Ok(false);
        }
    };
    let Some(parent) = item.to.parent() else {
        failed.push(failure(item, &item.to, "the target has no folder".into()));
        return Ok(false);
    };
    if let Err(e) = ensure_dir(writer, Phase::Apply, parent) {
        failed.push(failure(item, &item.to, describe(&e)));
        return Ok(false);
    }
    let temp = free_temp_name(parent, &format!("{}-c{index}", writer.entry()), "part");
    let mut intent = Intent::new(Action::Copy, Phase::Apply, &item.to);
    intent.item = Some(index);
    intent.file_id = Some(item.file_id.clone());
    intent.from = Some(item.from.clone());
    intent.temp = Some(temp.clone());
    intent.size_bytes = Some(size);
    let op = writer.intent(intent)?;

    let result = (|| -> io::Result<()> {
        let copied = fs::copy(&item.from, &temp)?;
        let on_disk = fs::metadata(&temp)?.len();
        if copied != size || on_disk != size {
            return Err(io::Error::other(
                "the copy's size does not match the original",
            ));
        }
        rename_no_replace(&temp, &item.to)
    })();
    match result {
        Ok(()) => {
            writer.done_at(op, modified_ns(&item.to))?;
            Ok(true)
        }
        Err(e) => {
            let _ = fs::remove_file(&temp);
            writer.failed(op)?;
            failed.push(failure(item, &item.to, describe(&e)));
            Ok(false)
        }
    }
}

fn write_subtitles(
    writer: &mut EntryWriter,
    item: &RenameItem,
    heard_subtitles: &dyn Fn(&FileId) -> Option<String>,
    failed: &mut Vec<OperationFailure>,
) -> crate::Result<()> {
    let Some(path) = &item.heard_subtitles_to else {
        return Ok(());
    };
    let Some(text) = heard_subtitles(&item.file_id) else {
        return Ok(());
    };
    let mut intent = Intent::new(Action::Create, Phase::Apply, path);
    intent.item = None;
    intent.file_id = Some(item.file_id.clone());
    intent.size_bytes = Some(text.len() as u64);
    let op = writer.intent(intent)?;
    let mut created = false;
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        created = true;
        file.write_all(text.as_bytes())?;
        file.sync_all()
    })();
    match result {
        Ok(()) => writer.done_at(op, modified_ns(path))?,
        Err(e) => {
            if created {
                let _ = fs::remove_file(path);
            }
            writer.failed(op)?;
            failed.push(failure(
                item,
                path,
                format!("the subtitle file was not saved: {}", describe(&e)),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three files to rename in one folder, and a journal that fails after `writes` records.
    fn rename_with_failing_journal(writes: u32) -> (tempfile::TempDir, crate::Result<()>) {
        let dir = tempfile::tempdir().unwrap();
        let mut moves = Vec::new();
        for i in 0..3u32 {
            let from = dir.path().join(format!("title_t0{i}.mkv"));
            fs::write(&from, format!("file {i}")).unwrap();
            moves.push(PlannedMove {
                item: i,
                file_id: FileId(format!("title_t0{i}.mkv")),
                from,
                to: dir.path().join("Season 01").join(format!("E0{i}.mkv")),
                size: 6,
                modified_ns: None,
            });
        }
        let journal = Journal::new(dir.path().join("data").join("history.jsonl"));
        let (_guard, mut writer) = journal
            .start("Show", dir.path(), SaveModeKind::RenameInPlace)
            .unwrap();
        writer.writes_left = Some(writes);
        let mut failed = Vec::new();
        let result = move_set(&mut writer, Phase::Apply, moves, &mut failed).map(|_| ());
        (dir, result)
    }

    #[test]
    fn a_journal_failure_moves_staged_files_back() {
        // Every cut-off point from the first record to the last.
        for writes in 0..20 {
            let (dir, result) = rename_with_failing_journal(writes);
            let mut names: Vec<String> = fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with(".mi-"))
                .collect();
            names.sort();
            assert!(
                names.is_empty(),
                "after {writes} records: {names:?} {result:?}"
            );
            // Every file is either at its original name or at its target.
            let mut at_original = 0;
            for i in 0..3 {
                let original = dir.path().join(format!("title_t0{i}.mkv"));
                let target = dir.path().join("Season 01").join(format!("E0{i}.mkv"));
                assert!(
                    original.exists() != target.exists(),
                    "after {writes} records, file {i}"
                );
                at_original += usize::from(original.exists());
            }
            match result {
                // Before the first rename, and after the last, nothing needed moving back.
                Err(e) if writes >= 2 && at_original > 0 => {
                    assert!(e.to_string().contains("moved back"), "{writes}: {e}")
                }
                Err(e) => assert!(matches!(e, crate::RenameError::Journal(_)), "{e}"),
                Ok(()) => {}
            }
        }
    }
}
