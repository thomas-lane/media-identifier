# How identification works

Media Identifier names a file by comparing what is said in it with what is said in each
candidate episode, and by checking the file's length and its position on the disc. Speech
recognition mishears words, subtitles paraphrase, and songs repeat lines, so no single
comparison is trusted on its own and none requires an exact match. Every file is scored against
every candidate episode, and the best overall assignment of files to episodes wins.

![One file identified step by step: the words heard in the file, the same words in an episode's
subtitles, the file's chapter inside the play-all, the four signals, and the margin between the
best and second-best episode](images/identification.svg)

The figure uses sample values, not measured results.

Terms such as *play-all*, *window*, *margin* and *title hook* are defined in
[the glossary](glossary.md). Which online services supply episode lists and reference text is
described in [sources.md](sources.md).

## Scanning and play-all detection

<!-- owner: media module -->

A scan lists the video files in the chosen folder (`mkv`, `mp4`, `m4v`, `mov`, `avi`, `ts`,
`m2ts`, `mts`, `mpg`, `mpeg`, `vob`), probes each with ffprobe for duration, streams and
chapters, and classifies it as a candidate, the play-all, or ignored (unreadable, no audio, or
shorter than 20 seconds, such as menus and logos).

The play-all is the longest file when its duration is within 25% of the sum of the other files
and at least three times the next longest file. Its chapters usually mark where each short title
begins. When the play-all has more chapters than candidates were found, the scan warns that
titles are probably missing: MakeMKV skips titles shorter than its minimum length (120 seconds
by default), which drops short episodes such as two-minute songs.

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
