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

**Escalation**: transcribing more windows of a file after matching left it with a low margin.

**Segment**: one piece of recognised text with its start and end time in the file, as whisper.cpp
returns it; usually a sentence or a line of a song.

**Invented text** (hallucination): text the speech model writes although nobody said it, typically
over music or silence ("Thank you.", "Subtitles by ..."). Segments judged to be invented are
marked and left out of matching but still shown.

**Voice activity detection (VAD)**: finding the parts of the audio that contain speech, so only
those are decoded. Used for files longer than six minutes that are not music-heavy.

**Compression ratio**: the length of a segment's text divided by its length after zlib
compression. Text that repeats itself compresses well, so a high ratio marks looping output.

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
