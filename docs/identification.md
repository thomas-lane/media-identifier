# How identification works

Media Identifier names a file by comparing what is said in it with what is said in each
candidate episode, and by checking the file's length and its position on the disc. Speech
recognition mishears words, subtitles paraphrase, and songs repeat lines, so no single
comparison is trusted on its own and none requires an exact match. Every file is scored against
every candidate episode, and the best overall assignment of files to episodes wins.

Terms such as *play-all*, *window*, *margin* and *title hook* are defined in
[the glossary](glossary.md). Which online services supply episode lists and reference text is
described in [sources.md](sources.md).

## Scanning and play-all detection

<!-- owner: media module -->

A scan (`mi_media::scan_folder`) lists the video files in the chosen folder by extension (`mkv`,
`mp4`, `m4v`, `mov`, `avi`, `ts`, `m2ts`, `mts`, `mpg`, `mpeg`, `vob`, in any case). Subfolders
are included only when the scan is asked to be recursive. Names starting with `.` are skipped:
besides hidden files, these are the `._title_t00.mkv` files macOS writes on network shares,
which have a video extension but contain only file metadata.

Each file is probed with ffprobe for its duration, audio streams, subtitle streams and chapters,
four files at a time because probing a network share mostly waits on the network. The file then
gets one of three roles:

| Role | When |
|---|---|
| Ignored, with an "unreadable" warning | ffprobe fails or reports no duration |
| Ignored, with a "no audio" warning | the file has no audio stream, so there is nothing to listen to |
| Ignored, silently | shorter than 20 seconds, such as menus and logos |
| Candidate | everything else |

**Disc folders.** A folder copied straight from a disc (`VIDEO_TS` for DVD, `BDMV` for Blu-ray,
recognised by its name or by its `VIDEO_TS.IFO` or `index.bdmv` file) is reported with a warning
and its files are not listed. On a disc, one title is split across several 1 GB `.VOB` files,
and one `.VOB` file can hold parts of several titles, so these files do not correspond to
episodes. Ripping the disc with MakeMKV first produces one file per title.

**The play-all.** Among the candidates, the longest file is compared with all the others
together. Three measurements decide whether it is the play-all:

- *Duration fit*: its duration is within 25% of the sum of the other candidates. Rips trim a few
  seconds from each title and some titles may be missing, so an exact sum is not expected.
- *Chapter fit*: at least half of the other candidates (and at least two) have the length of one
  of its chapters, within 2 seconds or 1% of the file's length, each chapter matched to at most
  one file. A play-all usually has one chapter per title, so its chapter lengths are the titles'
  lengths.
- *Ratio*: its duration divided by the next longest file's.

The longest file is the play-all when the ratio is at least 3 and either fit holds, or when the
ratio is at least 1.5 and both fits hold. The ratio keeps a double-length episode among normal
episodes from being taken for a play-all; with both fits, a lower ratio is accepted so that a
disc of two or three episodes still has its play-all found. A play-all whose chapters fit but
whose duration does not is accepted because MakeMKV may have dropped many short titles, leaving
the play-all much longer than the files found. A play-all needs at least two other candidates.

The play-all gets a confidence from 0 to 1 and a plain-language reason that the Confirm show
screen can display. With `d` = 1 minus the duration difference divided by the allowed 25%
(clamped to 0..1) and `c` = the fraction of candidates matching a chapter, the confidence is
`0.6 × max(d, c) + 0.4 × min(d, c)`; without chapters it is `0.7 × d`, because duration alone
is weaker evidence. These durations only nominate the play-all. Whether its order can be trusted
is decided later by audio alignment (see "Matching: the play-all as an answer key").

**Missing short titles.** When the play-all has more chapters than there are candidates, and at
least half of the candidates match a chapter, the scan warns that titles are probably missing:
MakeMKV skips titles shorter than its minimum length (120 seconds by default), which drops short
episodes such as two-minute songs. The second condition checks that the chapters mark one title
each; a play-all with several chapters per episode always has more chapters than episodes, and
would otherwise always raise the warning.

**Show name guess.** The folder name becomes the initial search text: it is split into words at
spaces, `_` and `.`; disc and rip markers are removed (`D1`, `Disc 2`, `S03`, `Season 3`, `S1D2`,
`DVD`, `BD`, `Blu-ray`, a bracketed year such as `(1973)`); and an all-capitals disc label is
changed to title case, so `SCHOOLHOUSE_ROCK_D1` becomes "Schoolhouse Rock". When nothing is left,
as for a folder named `Season 2`, the parent folder is used.

**Audio for listening and alignment.** `mi_media::extract_audio` and `stream_audio` decode one
audio stream to 16 kHz mono 32-bit float samples, the format whisper.cpp expects; every channel,
surround included, is mixed down to mono. When the caller does not name a stream, the file is
probed and the stream chosen in this order: one in the requested language (two-letter codes such
as `en` match the three-letter `eng` that containers use), then the one the file marks as default,
then the first. A time window
is decoded by seeking before opening the input, so ffmpeg jumps straight to the window through
the file's index instead of decoding everything before it, and still starts at the exact time
asked for.

**Embedded subtitles.** A text subtitle stream (SubRip, ASS/SSA, WebVTT, MP4 text) is converted
by ffmpeg to SubRip and then to dialogue lines by `mi-sources`. DVD and Blu-ray subtitles are
pictures of text, which would need character recognition, so they are not used.

## Listening

<!-- owner: transcribe module -->

Speech is recognised locally with whisper.cpp. Two models are offered:

| Setting | Model file | Size |
|---|---|---|
| Accurate (default) | `ggml-large-v3-turbo-q5_0.bin` | 547 MiB |
| Fast | `ggml-small.en-q5_1.bin` | 181 MiB |

Both are downloaded on first use from Hugging Face (`ggerganov/whisper.cpp`, pinned to one
repository revision) and accepted only when size and SHA-256 match the values pinned in
`crates/mi-transcribe/src/catalog.rs`.

**Windows.** Files of six minutes or less are transcribed whole: short musical clips need every
second. Longer files are sampled in four windows of about 105 seconds centred at 15%, 40%, 65%
and 85% of the runtime, which skips opening and closing credits shared by every episode. When the
match margin after sampling is low, more windows (or the whole file) are transcribed. The
"Listen to a sample of each file" setting turns sampling off.

**Decoding.** English by default; previous text is not fed back as a prompt (`no_context`), so one
misheard line cannot steer the rest; non-speech tokens are suppressed; a failed decode is retried
at higher temperatures. Voice activity detection is off for files of six minutes or less and for
music-heavy content, where it cuts sung words.

**Invented text.** Over music and silence the model writes phrases that were never said
("Thank you.", "Subtitles by ...") or repeats one line. Such segments are marked and left out of
matching: known phrases, back-to-back repeats, text whose gzip compression ratio exceeds 2.4, and
segments the model rates as probably not speech.

## Matching: signals

<!-- owner: match module -->

Each file/episode pair gets up to four signals, each between 0 and 1:

- **Dialogue**: fuzzy similarity of the transcript to the episode's reference text, combining
  TF-IDF cosine over word n-grams, phonetic shingles (Double Metaphone codes, so "Sampson" and
  "Samson" agree), and fuzzy partial matching at the character level.
- **Title hook**: how well the episode title occurs in the transcript. Songs usually sing their
  title.
- **Length**: how well the file's duration fits the episode's listed runtime.
- **Disc order**: how well the file's position in the play-all fits the episode's place in the
  episode order.

A signal that cannot be measured (no reference text, no runtime, no play-all) is left out of the
combined score rather than counted as zero, so a file is never penalised for data a provider
lacks.

## Matching: the play-all as an answer key

<!-- owner: match module -->

A disc's play-all title contains the short titles back to back. Each short file is located inside
it by audio alignment (fingerprints computed from decoded audio), which gives the disc order.
That order is evidence of order only: it says which file comes before which, not which episode a
file is. The order is ignored when it is not trustworthy: located ranges overlap, too few files
are located, or the play-all appears shuffled.

## Matching: assignment and confidence

<!-- owner: match module -->

With a trustworthy disc order, files and episodes are matched by order-preserving dynamic
programming: both orders stay monotonic, a file may be skipped as an extra at a small penalty, and
episodes may be skipped freely because a disc rarely holds every episode. Otherwise files are
matched by Hungarian assignment, with one "no episode" option per file so that a file is left
unmatched rather than forced onto a poor episode.

Confidence is the margin between a file's assigned option and its runner-up (another episode or
"no episode"). A clear margin is **Confident** and pre-approved; a small margin is **Check** and
needs the user's review; a file whose best option is "no episode" is an **Extra** (probably a
bonus feature) and is left where it is.
