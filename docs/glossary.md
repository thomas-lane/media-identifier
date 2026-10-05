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

<!-- owner: match module -->

**Signal**: one independent comparison between a file and an episode (dialogue, title hook,
length, disc order), scored from 0 to 1.

**Title hook**: the episode title occurring in what was heard.

**Margin**: the score of a file's assigned option minus the score of its runner-up.

**Confident / Check**: a suggestion whose margin is at least / below the confidence threshold.
