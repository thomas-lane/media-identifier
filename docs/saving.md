# Saving and undo

After Review, the Rename screen saves the approved results in one of three ways. This document
describes how a save is planned, carried out, recorded and undone. The code is in
`crates/mi-rename/` (naming, planning, applying, the History journal) and
`Engine::plan_rename` / `Engine::apply_rename` in `crates/mi-core/src/engine.rs`.

## Ways to save

| Mode | What it does | History entry |
|---|---|---|
| Rename in place (default) | Moves each approved file to its new name under a root folder (the scanned folder unless the user picks another), creating the show and season folders | yes |
| Copy into a new folder | Copies each approved file to its new name under a destination folder; originals are untouched | yes |
| Only export a list | Writes a CSV of file to episode; no file changes | no |

Only files the user approved are saved. The play-all, files marked "Not an episode", skipped or
unchecked files, and files the scan ignored (unreadable, no audio, too short) stay where they
are and are listed under "Not renamed" in the preview. Subtitle, artwork and metadata files that
sit next to a video (`Episode 3.en.srt`, `Episode 3.nfo`) are not moved with it: MakeMKV output,
the app's main input, has none, and the app does not look for them. Move them yourself when a
folder has them, or players will no longer pair them with the renamed video.

## Names

The new path of each file comes from a template (`crates/mi-rename/src/naming.rs`):

| Scheme | Template |
|---|---|
| Jellyfin / Plex | `{show} ({year})/Season {season:02}/{show} ({year}) - S{season:02}E{episode:02} - {title}.{ext}` |
| Kodi | `{show} ({year})/Season {season:02}/{show} S{season:02}E{episode:02} - {title}.{ext}` |
| Custom | the user's template |

Placeholders are `{show}`, `{year}`, `{season}`, `{episode}`, `{title}` and `{ext}`;
`{season:02}` and `{episode:02}` pad with zeros to the given width (1 to 9). `/` (or `\`) starts
a new folder, and the last part must contain `{ext}`, so every file keeps its extension. A
template that breaks these rules, or contains `..` or an absolute path, is refused.

Rendering rules:

- When the show's year is unknown, `({year})` and `[{year}]` are removed with their brackets.
- Specials (season 0) go to `Season 00`; an episode without a title is named `Episode <number>`.
- Each folder and file name is made valid on both macOS and Windows: `: ` becomes ` - `, `/`, `\`
  and other `:` become `-`, `"` becomes `'`, `<>|?*` are removed, leading dots (which hide a file
  on macOS) and trailing dots and spaces (which Windows drops silently) are removed, and Windows
  device names such as `CON` get `_` appended. The rules do not depend on the operating system,
  so a library moved between the two keeps its names.
- A name is at most 200 bytes of UTF-8. Both file systems allow 255 characters; the margin
  leaves room for macOS storing accented letters decomposed and keeps full paths short for
  players that still assume 260-character paths. A long file name is shortened at the end of its
  stem, so the extension survives.

With "Also save subtitles (.srt) from what was heard", each saved file also gets
`<new name>.srt`, written from the segments the speech model heard (without the ones the
hallucination filter dropped; see [identification](identification.md#listening)). A sampled
file therefore has subtitles only for its sampled windows. The `.srt` is written after the video
is in place and never over an existing file.

## Planning: the preview

`Engine::plan_rename` builds a `RenamePlan` from the job's saved record (scanned files, episode
list, match results) and the request the window sends (decisions, mode, naming, the subtitle
option). `mi_rename::build_plan` computes every target path and every conflict; the only disk
access is checking whether paths exist and reading the size of each source file. The Rename
screen shows the plan as a folder tree and disables saving while a newer preview is still being
built, so the button always saves what is shown.

A plan with conflicts cannot be applied. The conflicts are:

| Conflict | Meaning |
|---|---|
| `SourceChanged` | A file is no longer at its scanned path, or has a different size than when it was scanned. The name may now belong to a different file: MakeMKV names every disc's titles `title_t00.mkv`, `title_t01.mkv` and so on, so a later rip into the same folder reuses the names, and an earlier save of the same job moved the files. |
| `DuplicateTarget` | Two files would get the same new name, compared ignoring letter case and Unicode normalization, because macOS and Windows file systems treat such names as one file. |
| `TargetExists` | A new name is already taken. For rename in place, a name held by another file in the same plan is not a conflict, because that file moves away first. |
| `ListExists` | The CSV file already exists and was typed rather than picked in the save dialog. |

Unicode normalization matters because a name such as `Café` can be stored as one code point
for `é` (NFC, what the catalogs send) or as `e` plus a combining accent (NFD, what HFS+ stores);
APFS, HFS+ and NTFS look both up as the same file. Names are compared after converting to NFC,
so a file already named correctly in the other form is not reported as taking its own name.

## Applying

`Engine::apply_rename` receives the plan the window shows, including the request it was built
from. It builds the plan again from that request and the job's record, with the disk as it is
now, and applies the rebuilt plan. The window's plan must equal it: same mode, items and
untouched files. This keeps the window from changing what a save does, for example by adding the
play-all or an arbitrary target, and refuses a plan built for settings the window has since
changed. A conflict that appeared since the preview (a target created by another program) refuses
the save as a conflict. Every target must also be an absolute path without `.` or `..` parts.

`mi_rename::apply_plan` then carries out the plan:

- **Rename in place** moves every file in two phases: first each file to a temporary name in its
  own folder (`.mi-<entry>-a<n>.tmp`, hidden on macOS), then each temporary file to its target,
  creating folders as needed. Two phases let files swap names and change only the case of their
  names. A file that cannot reach its target goes back to its original name.
- **Copy** copies each file to a temporary name next to its target (`.mi-<entry>-c<n>.part`),
  checks that the copy has the original's size, then renames it to the target.
- **Export** writes the CSV to a temporary file in the same folder and renames it to the chosen
  name, so a crash never leaves a half-written list. It replaces an existing file only when the
  user picked that file in the save dialog, which asked before replacing; a typed path never
  replaces a file.

Just before moving or copying a file, its size is compared with the scanned size again; a file
that differs is reported as failed and left alone.

**Nothing is overwritten.** A plain rename silently replaces an existing target on macOS and
Windows, and checking first leaves a moment in which another program can create the target. So
every move uses the operating system's own rename that fails when the target exists:
`renamex_np` with `RENAME_EXCL` on macOS and `MoveFileExW` without
`MOVEFILE_REPLACE_EXISTING` on Windows (`crates/mi-rename/src/fsops.rs`). A volume that does not
support the exclusive rename (some network shares return `ENOTSUP` on macOS) falls back to
checking and then renaming. Copies reach their target through the same rename.

A failed file is listed in the result with a plain reason and the others continue. The save
stops with an error only when the History journal cannot be written (for example, the disk that
holds the app's data is full). Files already moved to temporary names are then moved back to
their original names before the error is returned, and the error says how many, because nothing
else would find them under their hidden temporary names.

## The History journal

Every save that changes files is recorded in `<app data>/history.jsonl`, an append-only file
with one JSON record per line. Each save is an *entry*; each file system change in it is an
*operation* written as two records: an `intent` (what is about to happen), written and flushed to
disk before the change, and `done` or `failed` after it. Record kinds are `begin`, `intent`,
`done`, `failed` and `undone`; operations are `move`, `copy`, `create` (a subtitle file),
`mkdir`, `remove` and `removeDir`, each in the `apply` phase (the save) or the `undo` phase.
Intents record each file's size and modification time, which undo uses to tell whether a file
changed.

Replaying the records tells where every file is now, which is what History shows and what undo
starts from. Interruptions are handled as follows:

- **An operation with neither `done` nor `failed`** (the app stopped during it) is decided from
  the disk: a move happened when its source is gone and its target exists; a copy happened when
  its target exists and its temporary file does not. The next save or undo decides each such
  operation once and appends the missing record, so a later change on disk, such as another save
  writing the same target, cannot change what the interrupted save is said to have done.
- **A record cut short** (the disk filled up, or the power failed during the write) is skipped.
  Before appending, the journal ends such a partial line with a newline, so the next record starts
  on a line of its own and only the damaged record is lost.
- **A copy cut short** leaves its `.part` file; History lists it, and undo deletes it.

## Undo

Undoing an entry from History reverses it, journaling each step in the `undo` phase:

- Renamed files move back to their original names in the same two phases, never over an
  existing file. A file moves back only when it is still where the save put it with the same
  size and modification time; afterwards the original name is checked to hold that file.
- Copies and subtitle files the save created are deleted only when their size and modification
  time are unchanged, because tag editors often rewrite a file in place without changing its
  size. A created file that is gone counts as deleted by the user when the folder it was saved
  into is still there; when that folder is missing too (an external drive or network share that
  is not connected), the file is reported as failed, so undo can be retried once the drive is
  back.
- Folders the save created are deleted when they are empty or hold only files the operating
  system adds by itself (`.DS_Store`, `Thumbs.db`).

Files that changed since, or whose original name is now taken, are left alone and listed as
failed. The entry is marked undone only when nothing failed, so undo can be repeated after the
user resolves them. Originals are never touched by undoing a copy.
