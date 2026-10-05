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

## Scanning

<!-- owner: media module -->

**Chapter match**: a candidate file whose length equals the length of one of the play-all's
chapters, within 2 seconds or 1% of the file's length. Each chapter matches at most one file.
Many chapter matches show that the play-all has one chapter per title.

**Disc structure folder**: a folder copied from a disc as it is, `VIDEO_TS` (DVD) or `BDMV`
(Blu-ray), rather than ripped into one file per title. The scan reports it and skips its files.

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
