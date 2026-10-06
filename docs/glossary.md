# Glossary

## General

**Candidate file**: a video file the app tries to identify as an episode. Files the scan marks as
the play-all or ignores (unreadable, no audio, shorter than 20 seconds) are not candidates.

**Play-all**: a disc title that plays every episode back to back; its duration is close to the
sum of the candidate files. It is used as an answer key for disc order and never renamed.

**Extra**: a candidate file that matches no episode well, such as a bonus feature. Extras are left
where they are.

**Job**: one identification run over one folder for one confirmed show.

**Job record**: everything kept about one job: its results, the scanned files and what was heard
in each file. It is filled while the job runs and saved to `<app data>/jobs/` when the job
finishes, or when it fails or is cancelled with at least one file result, so Recent and Review
work after the app restarts. A job that stopped before any file was matched has nothing to
review and is not saved.

**Sidecar**: a helper executable shipped next to the app (ffmpeg and ffprobe).

## Scanning

**Chapter match**: a candidate file whose length equals the length of one of the play-all's
chapters, within 2 seconds or 1% of the file's length. Each chapter matches at most one file.
Many chapter matches show that the play-all has one chapter per title.

**Disc structure folder**: a folder copied from a disc as it is, `VIDEO_TS` (DVD) or `BDMV`
(Blu-ray), rather than ripped into one file per title. The scan reports it and skips its files.

## Listening

**Window**: a time range of a file that is transcribed. A file of six minutes or less is one
window covering the whole file.

**Escalation**: transcribing more windows of a file after matching left it with a low margin.

**Segment**: one piece of recognised text with its start and end time in the file, as whisper.cpp
returns it; usually a sentence or a line of a song.

**Invented text** (hallucination): text the speech model writes although nobody said it, typically
over music or silence ("Thank you.", "Subtitles by ..."). Segments judged to be invented are
marked and left out of matching but still shown.

**Voice activity detection (VAD)**: finding the parts of the audio that contain speech, so only
those are decoded. Used for files longer than six minutes that are not music-heavy: files of a
musical show, and files found to be mostly music when they are heard further (see
[mostly music](identification.md#mostly-music)).

**Compression ratio**: the length of a segment's text divided by its length after zlib
compression. Text that repeats itself compresses well, so a high ratio marks looping output.

## Sources

**Reference text**: text known to belong to an episode (subtitles, lyrics, or its summary),
normalised to plain dialogue lines. Text from a file's own embedded subtitle stream is not
reference text: it describes that file, not an episode.

**Dialogue text**: reference text that quotes the episode (subtitles or lyrics), as opposed to a
summary, which only describes it.

**Season pack**: one subtitle archive covering many or all episodes of a season, downloaded once.

**Aired order / DVD order**: two numberings of the same episodes. Aired order is the broadcast
numbering (TVmaze's main list, TMDb's default); DVD order is the order on the disc set (a TVmaze
alternate list or TMDb episode group of type DVD). An episode keeps its provider episode id in
both.

**Provider**: an online service or local origin of data, named by `mi_types::ProviderId`.

## Matching

**Signal**: one independent comparison between a file and an episode (dialogue, title hook,
length, disc order), scored from 0 to 1. A signal that cannot be measured is left out of the
combined score, not counted as 0.

**Heard text**: what matching compares with reference texts: the dialogue of a file's embedded
text subtitle stream when it has a usable one (in the job's language or untagged, with at least
20 words), otherwise its transcript.

**Mostly music**: a file whose show is musical (at least half of the episodes have lyrics) or
more than half of whose recognised segments are music notes or non-speech. Its title hook counts
double. See [mostly music](identification.md#mostly-music).

**Special**: an episode of season 0. Specials take no part in the disc order, because episode
lists sort them before season 1.

**Content word**: a heard word other than a common function word ("the", "and") or a sung or
hesitation sound ("la", "oh", "mm"). Dialogue is measured only when at least eight were heard.

**Phonetic code**: the Double Metaphone code of a word, a short string of its consonant sounds;
words that sound alike ("there", "their") share one.

**Phrase coverage**: the share of heard six-word phrases that occur, allowing misspellings, in an
episode's reference text, weighted by how specific each phrase's words are, and scaled down when
the heard phrases are much less specific than the reference texts' own (shared theme lines).

**Title hook**: the episode title occurring in what was heard.

**No episode option**: the choice of leaving a file unmatched, scored 0.25; a file is an extra
when it wins.

**Runner-up**: a file's best option other than the assigned one: another episode or the
no episode option, whichever scores higher.

**Margin**: the score of a file's assigned option minus the score of its runner-up.

**Confident / Check**: a suggestion whose margin is at least / below the confidence threshold
(0.15). A suggestion without dialogue or title support is Check whatever its margin.

**Fingerprint**: a compact description of audio, per 32 ms, of how the energy in 24 pitch bands
changed; it is the same for different encodes of the same audio.

**Alignment strength**: how well a file's fingerprint matches the play-all at its best offset,
from 0 (unrelated) to 1 (identical). Below 0.35 the file counts as not found in the play-all.

**Disc order**: the order of the files located inside the play-all. It is *trustworthy* when
enough files were located without overlaps and they start at the play-all's chapters, and
*used* when it also agrees with the anchors.

**Anchor**: a file found in the play-all that the dialogue, title and length alone identify
with a confident margin as a regular (not special) episode; anchors check the disc order and
place the files between them.

## Saving and releases

**History entry**: one save (rename in place or copy) as recorded in the History journal
(`history.jsonl`); undoing it moves renamed files back and deletes unchanged copies. A CSV export
changes no files and makes no entry. See [saving](saving.md).

**Changed source**: a file that, when a rename plan is built or applied, is missing from its
scanned path or has a different size than when it was scanned. It is never renamed, because the
name may now belong to a different file.

**Two-phase rename**: renaming every file first to a temporary name in its own folder, then to its
target, so files can swap names or change only the case of their names.

**Sidecar build**: the minimal LGPL ffmpeg and ffprobe programs built by
`scripts/build-ffmpeg.sh`, as opposed to an ffmpeg installed on the computer.

**Updater artifacts**: the signed files the app's updater downloads (`.app.tar.gz` on macOS, the
NSIS `-setup.exe` on Windows, each with a `.sig`), listed with the version and notes in
`latest.json`.

**Ad-hoc signature**: a code signature that identifies no developer; Apple Silicon requires one to
run any code, and it does not let an app skip the first-launch confirmation.
