# Glossary

## General

**Candidate file**: a video file the app tries to identify as an episode. Files the scan marks as
the play-all or ignores (unreadable, no audio, shorter than 20 seconds) are not candidates.

**Play-all**: a disc title that plays every episode back to back; its duration is close to the
sum of the candidate files. It is used as an answer key for disc order and never renamed.

**Extra**: a candidate file that matches no episode well, such as a bonus feature. Extras are left
where they are.

**Job**: one identification run over one folder for one confirmed show.

**Sidecar**: a helper executable shipped next to the app (ffmpeg and ffprobe).

## Listening

<!-- owner: transcribe module -->

**Window**: a time range of a file that is transcribed. A file of six minutes or less is one
window covering the whole file.

## Sources

<!-- owner: sources module -->

**Reference text**: text known to belong to an episode (subtitles, lyrics, or its summary),
normalised to plain dialogue lines.

## Matching

<!-- owner: match module -->

**Signal**: one independent comparison between a file and an episode (dialogue, title hook,
length, disc order), scored from 0 to 1.

**Title hook**: the episode title occurring in what was heard.

**Margin**: the score of a file's assigned option minus the score of its runner-up.

**Confident / Check**: a suggestion whose margin is at least / below the confidence threshold.

## Saving and releases

<!-- owner: release module -->

**History entry**: one save (rename in place or copy) as recorded in the History journal
(`history.jsonl`); undoing it moves renamed files back and deletes unchanged copies. A CSV export
changes no files and makes no entry.

**Two-phase rename**: renaming every file first to a temporary name in its own folder, then to its
target, so files can swap names or change only the case of their names.

**Sidecar build**: the minimal LGPL ffmpeg and ffprobe programs built by
`scripts/build-ffmpeg.sh`, as opposed to an ffmpeg installed on the computer.

**Updater artifacts**: the signed files the app's updater downloads (`.app.tar.gz` on macOS, the
NSIS `-setup.exe` on Windows, each with a `.sig`), listed with the version and notes in
`latest.json`.

**Ad-hoc signature**: a code signature that identifies no developer; Apple Silicon requires one to
run any code, and it does not let an app skip the first-launch confirmation.
