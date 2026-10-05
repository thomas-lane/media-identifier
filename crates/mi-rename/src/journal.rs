//! The History journal, which makes renames and copies undoable.
//!
//! The journal is an append-only JSON Lines file (`<app data>/history.jsonl`). Each save is an
//! *entry*; each file system change in it is an *operation* written as two records: an `intent`
//! (what is about to happen) before the change, flushed to disk, and `done` or `failed` after it.
//! An intent with neither, left by a crash or power cut, is resolved by looking at the disk (for a
//! move: is the source gone and the target there?). Replaying the records therefore tells where
//! every file is now, which is what History shows and what undo starts from, even when a save
//! was interrupted half way.
//!
//! Record kinds (`"kind"`): `begin` (entry id, time, show, folder, mode), `intent` (operation
//! number, action, phase, paths, size), `done`, `failed`, `undone`. Actions: `move`, `copy`,
//! `create` (a subtitle file), `mkdir`, `remove`, `removeDir`. Phase `apply` is the save itself,
//! `undo` the undo, so History can show what a save did even after it was undone.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use mi_types::{
    FileId, HistoryEntry, HistoryId, HistoryItem, OperationFailure, SaveModeKind, UndoOutcome,
};
use serde::{Deserialize, Serialize};

use crate::RenameError;
use crate::apply::{PlannedMove, move_set};
use crate::fsops::{describe, exists};

/// Serialises all journal writers in this process: one save or undo at a time.
static LOCK: Mutex<()> = Mutex::new(());

/// Names of files an operating system adds to folders on its own. A folder the app created that
/// holds only these counts as empty when undo removes it.
const OS_CLUTTER: &[&str] = &[".DS_Store", "Thumbs.db"];

/// An append-only JSON Lines file (`<app data>/history.jsonl`) recording every save.
#[derive(Debug, Clone)]
pub struct Journal {
    path: PathBuf,
}

/// What an operation does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Action {
    /// Rename `from` to `to`.
    Move,
    /// Copy `from` to `temp`, then rename `temp` to `to`.
    Copy,
    /// Create `to` (a subtitle file).
    Create,
    /// Create the folder `to`.
    Mkdir,
    /// Delete the file `to` (a copy or created file, during undo).
    Remove,
    /// Delete the empty folder `to` (during undo).
    RemoveDir,
}

/// Whether an operation belongs to the save or to its undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Phase {
    /// The save.
    Apply,
    /// The undo.
    Undo,
}

/// One operation, as recorded before it happens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Intent {
    pub action: Action,
    pub phase: Phase,
    /// Index of the plan item the operation belongs to; `None` for folders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_id: Option<FileId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<PathBuf>,
    pub to: PathBuf,
    /// The temporary file a copy is written to before it is renamed to `to`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp: Option<PathBuf>,
    /// Size of the file moved, copied or created, used to verify it on undo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

impl Intent {
    pub fn new(action: Action, phase: Phase, to: impl Into<PathBuf>) -> Self {
        Self {
            action,
            phase,
            item: None,
            file_id: None,
            from: None,
            to: to.into(),
            temp: None,
            size_bytes: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Record {
    Begin {
        entry: String,
        created_at_ms: i64,
        show_name: String,
        folder: PathBuf,
        mode: SaveModeKind,
    },
    Intent {
        entry: String,
        op: u32,
        #[serde(flatten)]
        intent: Intent,
    },
    Done {
        entry: String,
        op: u32,
    },
    Failed {
        entry: String,
        op: u32,
    },
    Undone {
        entry: String,
        at_ms: i64,
    },
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

fn new_entry_id() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    format!(
        "{}-{}-{}",
        now_ms(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn journal_error(e: impl std::fmt::Display) -> RenameError {
    RenameError::Journal(e.to_string())
}

/// Writes one entry's records. The `begin` record is written just before the first operation,
/// so a save that changes nothing leaves no entry.
pub(crate) struct EntryWriter {
    path: PathBuf,
    file: Option<File>,
    entry: String,
    next_op: u32,
    pending_begin: Option<Record>,
}

impl EntryWriter {
    /// The entry's id.
    pub fn entry(&self) -> &str {
        &self.entry
    }

    /// Whether anything has been written for this entry.
    pub fn has_begun(&self) -> bool {
        self.pending_begin.is_none()
    }

    fn write(&mut self, record: &Record) -> crate::Result<()> {
        if self.file.is_none() {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent).map_err(journal_error)?;
            }
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .map_err(journal_error)?;
            self.file = Some(file);
        }
        let file = self.file.as_mut().expect("opened above");
        let mut line = serde_json::to_string(record).map_err(journal_error)?;
        line.push('\n');
        file.write_all(line.as_bytes()).map_err(journal_error)?;
        // The record must be on disk before the operation it describes happens.
        file.sync_data().map_err(journal_error)
    }

    /// Records an intended operation and returns its number.
    pub fn intent(&mut self, intent: Intent) -> crate::Result<u32> {
        if let Some(begin) = self.pending_begin.take() {
            self.write(&begin)?;
        }
        let op = self.next_op;
        self.next_op += 1;
        let record = Record::Intent {
            entry: self.entry.clone(),
            op,
            intent,
        };
        self.write(&record)?;
        Ok(op)
    }

    /// Marks an operation done.
    pub fn done(&mut self, op: u32) -> crate::Result<()> {
        let record = Record::Done {
            entry: self.entry.clone(),
            op,
        };
        self.write(&record)
    }

    /// Marks an operation as not having happened.
    pub fn failed(&mut self, op: u32) -> crate::Result<()> {
        let record = Record::Failed {
            entry: self.entry.clone(),
            op,
        };
        self.write(&record)
    }

    fn undone(&mut self) -> crate::Result<()> {
        let record = Record::Undone {
            entry: self.entry.clone(),
            at_ms: now_ms(),
        };
        self.write(&record)
    }
}

/// How an operation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Done,
    Failed,
    /// Neither recorded: the app stopped during the operation.
    Unknown,
}

/// One entry, replayed from the records.
#[derive(Debug, Clone)]
struct EntryState {
    id: String,
    created_at_ms: i64,
    show_name: String,
    folder: PathBuf,
    mode: SaveModeKind,
    ops: Vec<(u32, Intent, Status)>,
    undone_at_ms: Option<i64>,
}

/// Where one moved file started, is now, and was after the save.
#[derive(Debug, Clone)]
struct MoveTrack {
    file_id: FileId,
    original: PathBuf,
    size: Option<u64>,
    current: PathBuf,
    after_save: PathBuf,
}

/// A file the save created (a copy or a subtitle file).
#[derive(Debug, Clone)]
struct CreatedTrack {
    file_id: FileId,
    source: Option<PathBuf>,
    path: PathBuf,
    size: Option<u64>,
    is_copy: bool,
    made_by_save: bool,
    present: bool,
}

/// The effect of an entry's operations.
#[derive(Debug, Default)]
struct Effects {
    moves: BTreeMap<u32, MoveTrack>,
    created: Vec<CreatedTrack>,
    /// Folders the save created that still exist, in creation order.
    dirs: Vec<PathBuf>,
    /// Temporary copy files left by an interrupted or failed copy.
    leftovers: Vec<PathBuf>,
}

impl EntryState {
    /// Whether an operation took effect, consulting the disk for unfinished ones.
    fn happened(intent: &Intent, status: Status) -> bool {
        match status {
            Status::Done => true,
            Status::Failed => false,
            Status::Unknown => match intent.action {
                Action::Move => {
                    intent.from.as_deref().is_some_and(|f| !exists(f)) && exists(&intent.to)
                }
                Action::Copy | Action::Create | Action::Mkdir => exists(&intent.to),
                Action::Remove | Action::RemoveDir => !exists(&intent.to),
            },
        }
    }

    fn effects(&self) -> Effects {
        let mut fx = Effects::default();
        for (_, intent, status) in &self.ops {
            let happened = Self::happened(intent, *status);
            match intent.action {
                Action::Move if happened => {
                    let item = intent.item.unwrap_or(u32::MAX);
                    let from = intent.from.clone().unwrap_or_default();
                    let track = fx.moves.entry(item).or_insert_with(|| MoveTrack {
                        file_id: intent.file_id.clone().unwrap_or(FileId(String::new())),
                        original: from.clone(),
                        size: intent.size_bytes,
                        current: from.clone(),
                        after_save: from,
                    });
                    track.current = intent.to.clone();
                    if intent.phase == Phase::Apply {
                        track.after_save = intent.to.clone();
                    }
                }
                Action::Copy | Action::Create => {
                    if happened {
                        fx.created.push(CreatedTrack {
                            file_id: intent.file_id.clone().unwrap_or(FileId(String::new())),
                            source: intent.from.clone(),
                            path: intent.to.clone(),
                            size: intent.size_bytes,
                            is_copy: intent.action == Action::Copy,
                            made_by_save: intent.phase == Phase::Apply,
                            present: true,
                        });
                    }
                    if let Some(temp) = &intent.temp
                        && exists(temp)
                    {
                        fx.leftovers.push(temp.clone());
                    }
                }
                Action::Remove if happened => {
                    for c in fx.created.iter_mut().filter(|c| c.path == intent.to) {
                        c.present = false;
                    }
                    fx.leftovers.retain(|p| *p != intent.to);
                }
                Action::Mkdir if happened && intent.phase == Phase::Apply => {
                    fx.dirs.push(intent.to.clone())
                }
                Action::RemoveDir if happened => fx.dirs.retain(|d| *d != intent.to),
                _ => {}
            }
        }
        fx
    }

    fn history_entry(&self) -> HistoryEntry {
        let fx = self.effects();
        let undone = self.undone_at_ms.is_some();
        let mut items: Vec<HistoryItem> = fx
            .moves
            .values()
            .filter_map(|t| {
                let at = if undone { &t.after_save } else { &t.current };
                (*at != t.original).then(|| HistoryItem {
                    from: t.original.clone(),
                    to: at.clone(),
                })
            })
            .collect();
        items.extend(
            fx.created
                .iter()
                .filter(|c| c.is_copy && c.made_by_save && (undone || c.present))
                .map(|c| HistoryItem {
                    from: c.source.clone().unwrap_or_default(),
                    to: c.path.clone(),
                }),
        );
        HistoryEntry {
            id: HistoryId(self.id.clone()),
            created_at_ms: self.created_at_ms,
            show_name: self.show_name.clone(),
            folder: self.folder.clone(),
            mode: self.mode,
            items,
            undone_at_ms: self.undone_at_ms,
        }
    }
}

impl Journal {
    /// Uses the journal file at `path` (created on first write).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The journal file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Takes the process-wide lock and prepares a new entry; nothing is written until the first
    /// operation.
    pub(crate) fn start(
        &self,
        show_name: &str,
        folder: &Path,
        mode: SaveModeKind,
    ) -> (MutexGuard<'static, ()>, EntryWriter) {
        let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = new_entry_id();
        let writer = EntryWriter {
            path: self.path.clone(),
            file: None,
            entry: entry.clone(),
            next_op: 0,
            pending_begin: Some(Record::Begin {
                entry,
                created_at_ms: now_ms(),
                show_name: show_name.to_owned(),
                folder: folder.to_path_buf(),
                mode,
            }),
        };
        (guard, writer)
    }

    /// All entries that changed something, newest first.
    ///
    /// Each entry lists its files as original path and current path; an undone entry lists them
    /// as they were after the save, with `undone_at_ms` set. An interrupted save lists exactly
    /// the moves that happened, including a file left under a temporary name.
    pub fn list(&self) -> crate::Result<Vec<HistoryEntry>> {
        let mut entries: Vec<HistoryEntry> = self
            .read()?
            .iter()
            .map(EntryState::history_entry)
            .filter(|e| !e.items.is_empty() || e.undone_at_ms.is_some())
            .collect();
        entries.reverse();
        entries.sort_by_key(|e| std::cmp::Reverse(e.created_at_ms));
        Ok(entries)
    }

    /// Undoes an entry.
    ///
    /// - Renamed files move back to their original names, through a temporary name (so names
    ///   that differ only in case, or files that swapped names, are restored too) and never over
    ///   an existing file. A file is moved back only when it is still where the save put it and
    ///   still has the size it had then; afterwards the original name is checked to hold a file
    ///   of that size.
    /// - Copies and subtitle files the save created are deleted only when their size is
    ///   unchanged; ones already deleted are skipped. Originals are never touched by undoing a
    ///   copy.
    /// - Folders the save created are deleted when they are empty, or hold only files the
    ///   operating system adds by itself (`.DS_Store`, `Thumbs.db`).
    ///
    /// Files that changed since, or whose original name is now taken, are left alone and listed
    /// in [`UndoOutcome::failed`]; the entry is marked undone only when nothing failed, so undo
    /// can be retried after the user resolves them. Undoing an entry that is already undone does
    /// nothing.
    pub fn undo(&self, id: &HistoryId) -> crate::Result<UndoOutcome> {
        let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let state = self
            .read()?
            .into_iter()
            .find(|e| e.id == id.0)
            .ok_or_else(|| RenameError::NotFound(id.0.clone()))?;
        if state.undone_at_ms.is_some() {
            return Ok(UndoOutcome {
                restored: 0,
                failed: Vec::new(),
            });
        }
        let fx = state.effects();
        let mut writer = EntryWriter {
            path: self.path.clone(),
            file: None,
            entry: state.id.clone(),
            next_op: state.ops.iter().map(|(op, ..)| op + 1).max().unwrap_or(0),
            pending_begin: None,
        };
        let mut failed = Vec::new();
        let mut restored = 0u32;

        // Renamed files go back.
        let mut moves = Vec::new();
        for (item, track) in &fx.moves {
            if track.current == track.original {
                continue;
            }
            match check_unchanged(&track.current, track.size) {
                Ok(size) => moves.push(PlannedMove {
                    item: *item,
                    file_id: track.file_id.clone(),
                    from: track.current.clone(),
                    to: track.original.clone(),
                    size,
                }),
                Err(message) => failed.push(OperationFailure {
                    file_id: track.file_id.clone(),
                    path: track.current.clone(),
                    message,
                }),
            }
        }
        for m in move_set(&mut writer, Phase::Undo, moves, &mut failed)? {
            match fs::metadata(&m.to) {
                Ok(meta) if meta.is_file() && meta.len() == m.size => restored += 1,
                _ => failed.push(OperationFailure {
                    file_id: m.file_id.clone(),
                    path: m.to.clone(),
                    message:
                        "the file could not be found under its original name after moving it back"
                            .to_owned(),
                }),
            }
        }

        // Copies and subtitle files are deleted.
        // A created file the user already deleted needs nothing.
        for created in fx.created.iter().filter(|c| c.present && exists(&c.path)) {
            match check_unchanged(&created.path, created.size) {
                Ok(_) => {
                    let mut intent = Intent::new(Action::Remove, Phase::Undo, &created.path);
                    intent.file_id = Some(created.file_id.clone());
                    let op = writer.intent(intent)?;
                    match fs::remove_file(&created.path) {
                        Ok(()) => {
                            writer.done(op)?;
                            if created.is_copy {
                                restored += 1;
                            }
                        }
                        Err(e) => {
                            writer.failed(op)?;
                            failed.push(OperationFailure {
                                file_id: created.file_id.clone(),
                                path: created.path.clone(),
                                message: describe(&e),
                            });
                        }
                    }
                }
                Err(message) => failed.push(OperationFailure {
                    file_id: created.file_id.clone(),
                    path: created.path.clone(),
                    message,
                }),
            }
        }
        for temp in &fx.leftovers {
            let op = writer.intent(Intent::new(Action::Remove, Phase::Undo, temp))?;
            match fs::remove_file(temp) {
                Ok(()) => writer.done(op)?,
                Err(_) => writer.failed(op)?,
            }
        }

        // Folders the save created go when empty, newest first.
        for dir in fx.dirs.iter().rev() {
            if !is_effectively_empty(dir) {
                continue;
            }
            let op = writer.intent(Intent::new(Action::RemoveDir, Phase::Undo, dir))?;
            for clutter in OS_CLUTTER {
                let _ = fs::remove_file(dir.join(clutter));
            }
            match fs::remove_dir(dir) {
                Ok(()) => writer.done(op)?,
                Err(_) => writer.failed(op)?,
            }
        }

        if failed.is_empty() {
            writer.undone()?;
        }
        Ok(UndoOutcome { restored, failed })
    }

    /// Reads and replays every entry, in file order. A line that cannot be parsed (for example
    /// one cut short by a crash) is skipped.
    fn read(&self) -> crate::Result<Vec<EntryState>> {
        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(journal_error(e)),
        };
        let mut entries: Vec<EntryState> = Vec::new();
        let mut index: BTreeMap<String, usize> = BTreeMap::new();
        for (n, line) in BufReader::new(file).lines().enumerate() {
            let line = line.map_err(journal_error)?;
            if line.trim().is_empty() {
                continue;
            }
            let record: Record = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("history journal line {} skipped: {e}", n + 1);
                    continue;
                }
            };
            match record {
                Record::Begin {
                    entry,
                    created_at_ms,
                    show_name,
                    folder,
                    mode,
                } => {
                    index.insert(entry.clone(), entries.len());
                    entries.push(EntryState {
                        id: entry,
                        created_at_ms,
                        show_name,
                        folder,
                        mode,
                        ops: Vec::new(),
                        undone_at_ms: None,
                    });
                }
                Record::Intent { entry, op, intent } => {
                    if let Some(&i) = index.get(&entry) {
                        entries[i].ops.push((op, intent, Status::Unknown));
                    }
                }
                Record::Done { entry, op } => {
                    set_status(&mut entries, &index, &entry, op, Status::Done)
                }
                Record::Failed { entry, op } => {
                    set_status(&mut entries, &index, &entry, op, Status::Failed)
                }
                Record::Undone { entry, at_ms } => {
                    if let Some(&i) = index.get(&entry) {
                        entries[i].undone_at_ms = Some(at_ms);
                    }
                }
            }
        }
        Ok(entries)
    }
}

fn set_status(
    entries: &mut [EntryState],
    index: &BTreeMap<String, usize>,
    entry: &str,
    op: u32,
    status: Status,
) {
    if let Some(&i) = index.get(entry)
        && let Some(slot) = entries[i].ops.iter_mut().find(|(o, ..)| *o == op)
    {
        slot.2 = status;
    }
}

/// Checks that `path` is a file of the recorded size; returns its size.
fn check_unchanged(path: &Path, size: Option<u64>) -> Result<u64, String> {
    match fs::metadata(path) {
        Ok(meta) if !meta.is_file() => Err("it is no longer a file".to_owned()),
        Ok(meta) => match size {
            Some(expected) if expected != meta.len() => {
                Err("it changed since it was saved, so it was left as it is".to_owned())
            }
            _ => Ok(meta.len()),
        },
        Err(e) => Err(describe(&e)),
    }
}

fn is_effectively_empty(dir: &Path) -> bool {
    match fs::read_dir(dir) {
        Ok(entries) => entries.flatten().all(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|name| OS_CLUTTER.contains(&name))
        }),
        Err(_) => false,
    }
}
