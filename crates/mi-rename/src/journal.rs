//! The History journal, which makes renames and copies undoable.
//!
//! The journal is an append-only JSON Lines file (`<app data>/history.jsonl`). Each save is an
//! *entry*; each file system change in it is an *operation* written as two records: an `intent`
//! (what is about to happen) before the change, flushed to disk, and `done` or `failed` after it.
//! An intent with neither, left by a crash or power cut, is resolved by looking at the disk (for a
//! move: is the source gone and the target there?). The next save or undo settles each such
//! intent once, by appending the `done` or `failed` it lacks, so a later change on disk (another
//! save writing the same target) cannot change what the interrupted save is said to have done.
//! Replaying the records therefore tells where every file is now, which is what History shows and
//! what undo starts from, even when a save was interrupted half way.
//!
//! A record cut short (the disk filled up, or the power failed during the write) is left as a
//! partial last line. Before appending, the writer ends such a line with a newline, so the next
//! record starts on a line of its own and only the damaged record is lost.
//!
//! Record kinds (`"kind"`): `begin` (entry id, time, show, folder, mode), `intent` (operation
//! number, action, phase, paths, size), `done`, `failed`, `undone`. Actions: `move`, `copy`,
//! `create` (a subtitle file), `mkdir`, `remove`, `removeDir`. Phase `apply` is the save itself,
//! `undo` the undo, so History can show what a save did even after it was undone.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use mi_types::{
    FileId, HistoryEntry, HistoryId, HistoryItem, OperationFailure, SaveModeKind, UndoOutcome,
};
use serde::{Deserialize, Serialize};

use crate::RenameError;
use crate::apply::{PlannedMove, move_set};
use crate::fsops::{describe, exists, modified_ns};

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
    /// Modification time of that file, nanoseconds since the Unix epoch, also used to verify it
    /// on undo: editors that rewrite tags in place often keep a file's size. For a move it is
    /// recorded in the intent (a rename keeps it); for a copy or created file in the `done`
    /// record, once the file exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_ns: Option<i64>,
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
            modified_ns: None,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modified_ns: Option<i64>,
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

/// Ends a partial last line (a record cut short) with a newline, so the next record is readable.
fn end_partial_line(file: &mut File) -> io::Result<()> {
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(());
    }
    let mut last = [0u8; 1];
    file.seek(SeekFrom::Start(len - 1))?;
    file.read_exact(&mut last)?;
    if last[0] != b'\n' {
        // Appends go to the end whatever the read position.
        file.write_all(b"\n")?;
        file.sync_data()?;
    }
    Ok(())
}

/// Writes one entry's records. The `begin` record is written just before the first operation,
/// so a save that changes nothing leaves no entry.
pub(crate) struct EntryWriter {
    path: PathBuf,
    file: Option<File>,
    entry: String,
    next_op: u32,
    pending_begin: Option<Record>,
    /// Tests: the number of records written before every write fails, as on a full disk.
    #[cfg(test)]
    pub(crate) writes_left: Option<u32>,
}

impl EntryWriter {
    fn new(path: &Path, entry: String, next_op: u32, pending_begin: Option<Record>) -> Self {
        Self {
            path: path.to_path_buf(),
            file: None,
            entry,
            next_op,
            pending_begin,
            #[cfg(test)]
            writes_left: None,
        }
    }

    /// The entry's id.
    pub fn entry(&self) -> &str {
        &self.entry
    }

    /// Whether anything has been written for this entry.
    pub fn has_begun(&self) -> bool {
        self.pending_begin.is_none()
    }

    fn write(&mut self, record: &Record) -> crate::Result<()> {
        #[cfg(test)]
        if let Some(left) = self.writes_left.as_mut() {
            if *left == 0 {
                return Err(journal_error("No space left on device"));
            }
            *left -= 1;
        }
        if self.file.is_none() {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent).map_err(journal_error)?;
            }
            let mut file = OpenOptions::new()
                .create(true)
                .read(true)
                .append(true)
                .open(&self.path)
                .map_err(journal_error)?;
            end_partial_line(&mut file).map_err(journal_error)?;
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
        self.done_at(op, None)
    }

    /// Marks an operation done, recording the modification time of the file it produced.
    pub fn done_at(&mut self, op: u32, modified_ns: Option<i64>) -> crate::Result<()> {
        let record = Record::Done {
            entry: self.entry.clone(),
            op,
            modified_ns,
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
    modified_ns: Option<i64>,
    current: PathBuf,
    after_save: PathBuf,
}

impl MoveTrack {
    /// Whether the file is back under its original name although the journal does not say so:
    /// its current path is gone and the original holds a file of the recorded size. This is what
    /// a rename cut short by a journal error leaves after moving its files back.
    fn is_back_home(&self) -> bool {
        self.current != self.original
            && !exists(&self.current)
            && check_unchanged(&self.original, self.size, self.modified_ns).is_ok()
    }
}

/// A file the save created (a copy or a subtitle file).
#[derive(Debug, Clone)]
struct CreatedTrack {
    file_id: FileId,
    source: Option<PathBuf>,
    path: PathBuf,
    size: Option<u64>,
    modified_ns: Option<i64>,
    is_copy: bool,
    made_by_save: bool,
    present: bool,
}

/// A temporary copy file left by an interrupted or failed copy.
#[derive(Debug, Clone)]
struct Leftover {
    source: Option<PathBuf>,
    temp: PathBuf,
}

/// The effect of an entry's operations.
#[derive(Debug, Default)]
struct Effects {
    moves: BTreeMap<u32, MoveTrack>,
    created: Vec<CreatedTrack>,
    /// Folders the save created that still exist, in creation order.
    dirs: Vec<PathBuf>,
    /// Every folder the save created, including ones since removed.
    made_dirs: HashSet<PathBuf>,
    /// Temporary copy files still on disk.
    leftovers: Vec<Leftover>,
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
                // A copy is renamed from its temporary file as its last step, so while the
                // temporary file exists the copy was not finished.
                Action::Copy => {
                    exists(&intent.to) && intent.temp.as_deref().is_none_or(|t| !exists(t))
                }
                Action::Create | Action::Mkdir => exists(&intent.to),
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
                        modified_ns: intent.modified_ns,
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
                            modified_ns: intent.modified_ns,
                            is_copy: intent.action == Action::Copy,
                            made_by_save: intent.phase == Phase::Apply,
                            present: true,
                        });
                    }
                    if let Some(temp) = &intent.temp
                        && exists(temp)
                    {
                        fx.leftovers.push(Leftover {
                            source: intent.from.clone(),
                            temp: temp.clone(),
                        });
                    }
                }
                Action::Remove if happened => {
                    for c in fx.created.iter_mut().filter(|c| c.path == intent.to) {
                        c.present = false;
                    }
                    fx.leftovers.retain(|l| l.temp != intent.to);
                }
                Action::Mkdir if happened && intent.phase == Phase::Apply => {
                    fx.dirs.push(intent.to.clone());
                    fx.made_dirs.insert(intent.to.clone());
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
                let moved = *at != t.original && (undone || !t.is_back_home());
                moved.then(|| HistoryItem {
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
        // A copy cut short leaves its temporary file; listing it lets undo remove it.
        if !undone {
            items.extend(fx.leftovers.iter().map(|l| HistoryItem {
                from: l.source.clone().unwrap_or_default(),
                to: l.temp.clone(),
            }));
        }
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

    /// Takes the process-wide lock, settles unfinished operations of earlier entries, and
    /// prepares a new entry; nothing is written for it until the first operation.
    pub(crate) fn start(
        &self,
        show_name: &str,
        folder: &Path,
        mode: SaveModeKind,
    ) -> crate::Result<(MutexGuard<'static, ()>, EntryWriter)> {
        let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        self.settle()?;
        let entry = new_entry_id();
        let begin = Record::Begin {
            entry: entry.clone(),
            created_at_ms: now_ms(),
            show_name: show_name.to_owned(),
            folder: folder.to_path_buf(),
            mode,
        };
        let writer = EntryWriter::new(&self.path, entry, 0, Some(begin));
        Ok((guard, writer))
    }

    /// Reads every entry and appends a `done` or `failed` record for each operation that has
    /// neither (the app stopped during it), decided from the disk now. Call with the lock held.
    ///
    /// Settling before anything else changes the disk keeps an interrupted copy from later being
    /// credited with a file another save wrote at the same path. If another process is still
    /// running that operation, its own `done` or `failed` record comes later and replaces this
    /// one, because replay keeps the last status recorded for an operation.
    fn settle(&self) -> crate::Result<Vec<EntryState>> {
        let mut entries = self.read()?;
        for entry in &mut entries {
            let mut writer: Option<EntryWriter> = None;
            for (op, intent, status) in &mut entry.ops {
                if *status != Status::Unknown {
                    continue;
                }
                let happened = EntryState::happened(intent, Status::Unknown);
                let w = writer
                    .get_or_insert_with(|| EntryWriter::new(&self.path, entry.id.clone(), 0, None));
                if happened {
                    let modified = match intent.action {
                        Action::Copy | Action::Create => modified_ns(&intent.to),
                        _ => None,
                    };
                    w.done_at(*op, modified)?;
                    if modified.is_some() {
                        intent.modified_ns = modified;
                    }
                    *status = Status::Done;
                } else {
                    w.failed(*op)?;
                    *status = Status::Failed;
                }
            }
        }
        Ok(entries)
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
    /// - Copies and subtitle files the save created are deleted only when their size and
    ///   modification time are unchanged. One that is gone is skipped as deleted by the user when
    ///   the folder it was saved in is still reachable; when that folder is missing (a drive or
    ///   network share that is not connected), it is reported as failed, so undo can be retried
    ///   once the drive is back. Originals are never touched by undoing a copy.
    /// - Temporary files left by an interrupted copy are deleted.
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
            .settle()?
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
        let next_op = state.ops.iter().map(|(op, ..)| op + 1).max().unwrap_or(0);
        let mut writer = EntryWriter::new(&self.path, state.id.clone(), next_op, None);
        let mut failed = Vec::new();
        let mut restored = 0u32;

        // Renamed files go back.
        let mut moves = Vec::new();
        for (item, track) in &fx.moves {
            if track.current == track.original || track.is_back_home() {
                continue;
            }
            match check_unchanged(&track.current, track.size, track.modified_ns) {
                Ok((size, modified_ns)) => moves.push(PlannedMove {
                    item: *item,
                    file_id: track.file_id.clone(),
                    from: track.current.clone(),
                    to: track.original.clone(),
                    size,
                    modified_ns,
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
        for created in fx.created.iter().filter(|c| c.present) {
            if !exists(&created.path) {
                if !folder_reachable(&created.path, &fx.made_dirs) {
                    failed.push(OperationFailure {
                        file_id: created.file_id.clone(),
                        path: created.path.clone(),
                        message: "the drive or folder it was saved to is not available; connect it and undo again"
                            .to_owned(),
                    });
                }
                // Otherwise the user already deleted it, which needs nothing.
                continue;
            }
            match check_unchanged(&created.path, created.size, created.modified_ns) {
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
        for leftover in &fx.leftovers {
            let temp = &leftover.temp;
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
                Record::Done {
                    entry,
                    op,
                    modified_ns,
                } => {
                    set_status(&mut entries, &index, &entry, op, Status::Done);
                    if let Some(m) = modified_ns
                        && let Some(&i) = index.get(&entry)
                        && let Some(slot) = entries[i].ops.iter_mut().find(|(o, ..)| *o == op)
                    {
                        slot.1.modified_ns = Some(m);
                    }
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

/// Checks that `path` is a file of the recorded size and modification time (each when
/// recorded); returns its size and modification time.
fn check_unchanged(
    path: &Path,
    size: Option<u64>,
    modified: Option<i64>,
) -> Result<(u64, Option<i64>), String> {
    let changed = || "it changed since it was saved, so it was left as it is".to_owned();
    match fs::metadata(path) {
        Ok(meta) if !meta.is_file() => Err("it is no longer a file".to_owned()),
        Ok(meta) => {
            if size.is_some_and(|expected| expected != meta.len()) {
                return Err(changed());
            }
            let now = crate::fsops::modified_of(&meta);
            if modified.is_some() && now != modified {
                return Err(changed());
            }
            Ok((meta.len(), now))
        }
        Err(e) => Err(describe(&e)),
    }
}

/// Whether the folder a created file was saved in can be reached: the nearest folder above it
/// that the save did not create exists. When that folder is missing too, the drive or share is
/// most likely not connected, rather than the file deleted.
fn folder_reachable(path: &Path, made_dirs: &HashSet<PathBuf>) -> bool {
    path.ancestors()
        .skip(1)
        .find(|d| !made_dirs.contains(*d))
        .is_some_and(Path::is_dir)
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
