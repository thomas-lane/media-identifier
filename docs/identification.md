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

Speech is recognised locally with whisper.cpp, an implementation of OpenAI's Whisper speech
model, compiled into the app. Two models are offered:

| Setting | Model file | Size |
|---|---|---|
| Accurate (default) | `ggml-large-v3-turbo-q5_0.bin` | 547 MiB |
| Fast | `ggml-small.en-q5_1.bin` (English only) | 181 MiB |

Either is downloaded on first use from Hugging Face (`ggerganov/whisper.cpp`), together with the
885 KB Silero voice activity detection model (`ggml-org/whisper-vad`). Each URL names a fixed
repository commit, so the file behind it cannot change, and a file is accepted only when its size
and SHA-256 match the values pinned in `crates/mi-transcribe/src/catalog.rs`. How downloads
resume and are verified is described in [architecture.md](architecture.md#transcription).

### Which parts of a file are transcribed

A *window* is a time range of a file that is transcribed (see the [glossary](glossary.md)).

- **Files of six minutes or less** are transcribed whole, as one window. Short clips such as
  two-minute songs need every second to be told apart, and decoding a few minutes is quick.
- **Longer files** get four windows of 105 seconds centred at 15%, 40%, 65% and 85% of the
  runtime. Opening and closing credits are the same in every episode, so the first and last
  minutes say little about which episode a file is. A window that would cross the start or end
  of the file is moved inside it, and overlapping windows are merged.
- **Escalation.** When matching leaves a file with a low margin, more of it is transcribed
  (`mi_transcribe::escalation_windows`): one window of up to 105 seconds in the middle of each
  untranscribed gap between two windows that is at least 20 seconds long. Repeating this fills
  the middle of the file progressively; only when no such gap is left are the gaps before the
  first and after the last window used, since they hold the credits. Asked for the whole file,
  it returns every untranscribed gap of at least one second. Escalation never returns a range
  that was already transcribed.
- The setting "Listen to a sample of each file" turned off makes every file one whole-file
  window.

### Decoding settings

Each window is decoded with these settings (`mi_transcribe::DecodeOptions`):

- **Language**: English by default. The Fast model only knows English.
- **No carried-over text.** Whisper normally feeds the text of the previous 30-second block back
  in as a prompt, which keeps spelling consistent but lets one misheard or invented line steer
  everything after it. The prompt is turned off entirely (`no_context` and a prompt budget of
  zero tokens), because a wrong guess repeated through a file would match the wrong episode
  with false confidence.
- **Non-speech tokens suppressed**, so music notes and sound descriptions are not written out.
- **Temperature fallback.** The first decode is greedy (temperature 0). When it fails Whisper's
  quality checks (average token log-probability below -1, or a token entropy below 2.4, which
  indicates looping), whisper.cpp decodes again at temperatures 0.2, 0.4, ... up to 1.0, keeping
  the most probable of five samples each time. This rescues passages where the greedy decode
  got stuck.

### Voice activity detection

Voice activity detection (VAD) finds the stretches of audio that contain speech, using the Silero
model. With VAD, only those stretches are decoded: they are joined into one shorter buffer with
0.2 seconds of silence between them (stretches less than 0.3 seconds apart are joined first, and
each is padded by 0.1 seconds so first and last syllables survive), the buffer is transcribed in
one call, and each segment's times are mapped back to the original file. A window with no speech
gives no segments. Skipping music beds and silence saves time and removes the main source of
invented text.

VAD is used only for files longer than six minutes that are not music-heavy
(`mi_transcribe::use_vad`; the caller decides what is music-heavy, for example a show whose
reference text is song lyrics). The detector treats singing as non-speech in places and cuts sung
words, and short files are mostly speech or song anyway.

whisper.cpp has its own VAD step, but it runs only through the context-level `whisper_full` call;
the per-state call that `whisper-rs` uses ignores the setting. The app therefore runs the detector
itself (`crates/mi-transcribe/src/vad.rs`).

### Invented text

Over music and silence the model writes phrases that were never said ("Thank you.", "Subtitles
by ...") or repeats one line. Such text would match every episode equally and hide the real
signal. `mi_transcribe::HallucinationFilter` marks these *segments* (a segment is one timed piece
of recognised text) with a reason; marked segments stay visible in the evidence panel but are left
out of matching. Text is compared after lowercasing, removing apostrophes and turning all other
punctuation into spaces. Each segment is checked for these reasons in order, and the first that
applies is recorded:

1. **Known phrase**: the whole segment is one of a list of phrases ("thank you", "thanks for
   watching", "please subscribe", "you", "bye", ...), possibly repeated; or it starts with a
   credit prefix ("subtitles by", "captions by", "transcribed by", "translated by", "amara org",
   ...). The full lists are in `HallucinationFilter::default` in
   `crates/mi-transcribe/src/filter.rs`.
2. **High compression ratio**: the segment's text, compressed with zlib, shrinks by more than a
   factor of 2.4. Ordinary sentences compress by less than 2; a line looped many times compresses
   far better. OpenAI's Whisper uses the same check and threshold.
3. **Not speech**: the segment is only a sound description ("[MUSIC]", "(laughs)", "♪♪"), or the
   model rated it as probably not speech (no-speech probability above 0.6) while also being
   unsure of the words (average log-probability below -1). Both are required because a
   confidently decoded line with a high no-speech probability is usually speech over music.
4. **Repeated**: the segment's text equals the previous segment's. The first line of such a run
   is kept, so a chorus sung twice still counts once.

The filter is applied again whenever an escalation window adds segments, after sorting all
segments by time, so a line repeated across the boundary of two windows is caught too
(`mi_transcribe::add_window`).

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
