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

Matching lives in `crates/mi-match`. It reads no files and makes no network requests: it
receives transcripts, reference texts, durations and audio fingerprints as plain values, so each
rule below is checked by tests with constructed inputs. The entry point is
`mi_match::match_files`; its thresholds and weights are the fields of `mi_match::MatchConfig`,
and the values given here are its defaults.

Each file/episode pair gets up to four signals, each between 0 and 1: **dialogue**, **title
hook**, **length** and **disc order**. A signal that cannot be measured (no reference text, no
listed runtime, no usable play-all) is left out of the combined score rather than counted as
zero, so a file is never penalised for data a provider lacks.

### What is compared

The *heard text* of a file is the dialogue of its own embedded text subtitle stream when it has
one, and its transcript otherwise. Embedded subtitles are preferred because they are the file's
exact dialogue; the title hook searches both, because a song's title may be sung but missing
from its subtitles.

An episode's reference text is its subtitles and lyrics, joined. An episode with neither is
compared through its summary (a summary reference text, or else the summary from the episode
list), which is weaker evidence and is scored differently (see [Summaries](#summaries)). An
episode with none of these has no dialogue signal.

### Normalisation

Both sides are first split into word tokens (`crates/mi-match/src/normalize.rs`), so differences
in writing never count as differences in what was said:

- letters are lower-cased and common Latin accents removed ("Café" becomes `cafe`);
- apostrophes inside words are dropped ("what's" becomes `whats`), and every other character
  that is not a letter or digit separates words ("seventy-three" becomes `seventy three`);
- numbers are spelled as words, because Whisper writes "3" where subtitles write "three":
  four-digit years are read in pairs ("1973" becomes `nineteen seventy three`), ordinals are
  spelled ("21st" becomes `twenty first`), decimals and thousands separators are read
  ("3.5", "1,000");
- `&`, `%`, "Mr", "Mrs", "Dr", "OK" and "vs" are spelled out.

Each token remembers the characters of the original text it came from, so quotes on the Review
screen show the original words.

### Dialogue

Speech recognition makes three kinds of mistakes, and the dialogue signal combines one measure
for each:

| Mistake | Example | Measure that survives it |
|---|---|---|
| A word replaced by one that sounds the same | "their" for "there", "knight" for "night" | phonetic TF-IDF cosine |
| A word misspelt, split or joined | "tonite", "every body" for "everybody" | phrase coverage |
| Some words wrong, the rest right | most sentences | word TF-IDF cosine, phrase coverage |

**Word TF-IDF cosine.** TF-IDF represents a text as a vector with one entry per *term*, here
every word and every pair of adjacent words. Each entry is the term's count, damped as
`1 + ln(count)` so that a chorus sung ten times does not outweigh everything else, multiplied by
the term's inverse document frequency (IDF): how few of the candidate episodes' reference texts
contain it, as `ln(1 + (N − df + 0.5) / (df + 0.5))` for `N` texts of which `df` contain the term.
This weight is close to zero for terms every episode contains, so the theme song, the credits and
recurring character names do not make every episode look alike. A heard term that no episode
contains (usually a misheard word) is weighted like a term that one episode contains, so a
transcript full of mistakes is not dominated by them. The cosine of the angle between the heard
vector and an episode's vector is 1 when both use the same terms in the same proportions and 0
when they share none.

**Phonetic TF-IDF cosine.** The same measure over Double Metaphone codes instead of words.
Double Metaphone (provided by the `rphonetic` crate) maps a word to a short code of its consonant
sounds, so words that sound alike get the same code: "there" and "their", "write" and "right",
"Smith" and "Smyth". Codes are kept up to six characters, because the usual four would merge long words that begin
alike ("transportation" and "transformation" both give `TRNS`).

**Phrase coverage.** The heard tokens are cut into consecutive phrases of six words (a remainder
of one or two words joins the last phrase). Each phrase is compared, character by character, with
stretches of the reference of the same number of words, and its best *character similarity*
counts: twice the length of the longest common subsequence of characters divided by the total
length of both strings (RapidFuzz's `ratio`). A phrase with similarity 0.88
or more counts as found; 0.6 or less (what unrelated phrases of equal length reach by chance)
counts as not found; values between count in proportion. Each phrase is weighted by the sum of
its words' IDF, so a phrase of common words counts little. Coverage is the weighted share of
phrases found. Comparing every phrase with every stretch of an episode would be slow, so a
phrase is compared only where one of its rarer words occurs in the reference (same phonetic code,
at most 12 positions, rarest words first), with the stretch shifted by one word either way to
absorb a dropped or inserted word.

**Combination.** Dialogue similarity is `0.5 × coverage + 0.25 × word + 0.25 × phonetic`, where
each cosine is divided by 0.45 and capped at 1. Coverage weighs most because it measures how much
of what was heard occurs in the episode, which does not depend on how much of the episode was
heard: four sample windows of a 22-minute episode cover about a third of it, which lowers both
cosines against the whole episode even for the right one. The 0.45 cap reflects that: a cosine
of 0.45 already means the same dialogue.

Matching uses these measures rather than exact sentence comparison because a transcript with one
word wrong in four would share almost no exact sentences with its subtitles, and it uses no
language model or online service so that everything heard stays on the computer.

**Too little heard.** When the heard text has fewer than 8 content words (words other than
common function words such as "the", and sung or hesitation sounds such as "la", "oh", "mm"),
the dialogue signal is not measured: the absence of matches in a few words says nothing about
which episode a file is.

### Summaries

A summary describes an episode rather than quoting it, so only its distinctive words are
compared: words of three or more letters that are not common function words, each weighted by
IDF among the summaries. A summary word counts as heard when the same word, or a word with the
same phonetic code of at least three characters, was heard.
Similarity is `(coverage − 0.1) / 0.5`, clipped to 0..1 and multiplied by 0.8, so a summary can
never count as much as matching dialogue.

### Title hook

Songs usually sing their own title, and many episodes say theirs. The title hook
(`crates/mi-match/src/title_hook.rs`) looks for the episode title inside the heard text. Text in
parentheses or brackets ("Pilot (Part 1)") is removed from the title first, because providers add
it inconsistently and it is rarely spoken. The title is compared with stretches of the heard text
of one word fewer to one word more than the title, wherever one of the title's words occurs (by
phonetic code). Two similarities are taken and the larger counts: the character similarity of
the words, and the word-level phonetic agreement (twice the number of matching codes in order,
divided by the number of words on both sides, times 0.95 because codes are coarser than
spelling). Whole codes are compared rather than their characters, because codes are one to six
characters long and unrelated short codes share characters by chance. Similarity 0.7 or less
scores 0, 0.95 or more scores 1, linear between.

The result is multiplied by the title's *specificity*: the letters in its words other than
common function words, divided by 10 and kept between 0.25 and 1 (0.15 for a title made only of
function words). "Conjunction Junction" is fully specific; "Pilot" (0.5) and "The End" (0.3) are
weighted down because they are heard by chance. When too little was heard for the dialogue signal,
the title hook is measured only if the title was found, for the same reason.

### Length

Listed runtimes are rounded, often to whole minutes and sometimes to the broadcast slot, so the
length signal (`crates/mi-match/src/duration.rs`) is forgiving:

- a file within 60 seconds or 10% of the listed runtime (whichever is larger) scores 1;
- a listed runtime that is a whole number of half hours is often the slot rather than the
  programme, so a file shorter than it but at least 60% of it scores 0.9 (a 22-minute episode
  listed at 30 minutes);
- beyond that the score falls off as `exp(−x²)`, where `x` is the difference beyond the tolerance
  divided by 20% of the runtime plus 30 seconds. A 2½-minute song listed at 4 minutes scores
  about 0.86; a 22-minute file listed at 3 minutes scores 0.

### Combined score

The combined score of a pair is the weighted mean of its measured signals, with weights dialogue
0.55, title hook 0.15, length 0.10 and disc order 0.20. For a file the transcriber marked as
mostly music the title hook weight is doubled, because sung words are transcribed less reliably
than a title repeated in a chorus.

Length and disc order can say which of several episodes a file fits, but not whether it is an
episode at all: a 22-minute bonus feature fits a 22-minute runtime perfectly. So when an episode
has dialogue text, the dialogue was measured, and neither the dialogue reaches 0.3 nor the title
hook 0.5, the mean is multiplied by the dialogue score divided by 0.3. A file whose dialogue
resembles no episode then scores low against all of them and becomes an extra. Files located
inside a play-all that is used are exempt (see
[the play-all as an answer key](#matching-the-play-all-as-an-answer-key)), and so are
comparisons with summaries, which are too weak to rule an episode out.

### Evidence shown on the Review screen

Each pair carries its signals and the evidence the Review screen shows:

- **Quotes.** "Heard" and "reference" excerpts around the best-matching phrase, with six words of
  context on each side, shown when that phrase's character similarity is at least 0.75 (weaker
  overlaps are chance agreement on common words). Words are highlighted where a word-by-word
  alignment (longest common subsequence) pairs them with a word on the other side that is the
  same, has the same phonetic code, or has character similarity 0.8 or more. For a summary, the
  quotes highlight the summary words that were heard; with no dialogue text, the heard quote
  highlights where the title was heard.
- **Notes**, most important first:

| Note | Shown when |
|---|---|
| Disc order agrees (with chapter) | disc order signal 0.5 or more |
| Disc order disagrees | disc order signal below 0.5 |
| Play-all ignored | a play-all was found but not used |
| Title heard | title hook 0.5 or more |
| Length mismatch | length signal below 0.3 |
| No speech | nothing was heard |
| Mostly music | the transcriber marked the file as mostly music |
| No reference text | the episode has no dialogue text and no summary |
| Sampled (windows) | only sample windows of the file were transcribed |

## Matching: the play-all as an answer key

<!-- owner: match module -->

A disc's play-all title contains its short titles back to back, usually as the very same audio.
Finding where each short file sits inside it gives the files' order on the disc, which helps
when dialogue alone cannot tell (a song whose lyrics are unavailable, a file heard as "la la
la"). That order is evidence of order only: it says which file comes before which, not which
episode a file is, so it is turned into episode evidence only through files the dialogue already
identifies.

### Fingerprints

`mi_match::align::FingerprintBuilder` computes a fingerprint from 16 kHz mono audio, chunk by
chunk, so a two-hour play-all is never held in memory. The audio is cut into frames of 128 ms
every 32 ms. Each frame's spectrum is summed into 24 bands spaced evenly in pitch from 150 Hz to
4 kHz, and each band energy is taken as a natural logarithm. Energies more than 50 dB below the
frame's loudest band, or 90 dB below full scale, are raised to that floor: they are mostly
leakage and codec noise, which differ between encodes. A fingerprint frame stores, per band, how
much the log energy changed since the previous frame, in steps of 1/16 (about 0.54 dB).

Changes of log energy are used because they cancel what differs between copies of the same
audio: a volume change adds the same constant to every log energy, and an encoder's fixed tone
colouring adds a constant per band; both vanish in the change from one frame to the next. Steady
sounds give changes near zero, so what is compared is where sounds start, stop and move.

Binary fingerprints in the style of Chromaprint (one bit per band saying whether an energy
difference grew) are not used: during a steady sound the difference barely changes, the bit's
sign is decided by noise, and two encodes of the same audio disagree on it.

### Locating a file

`mi_match::locate` slides a file's fingerprint along the play-all's and measures, at each offset,
the normalised correlation of the two sequences of changes (the cosine of the angle between
them): about 1 for the same audio and about 0 for unrelated audio. To keep this fast, a first
pass compares 64 evenly spaced positions of the file, each summed over four frames (the change
over 128 ms, which varies slowly enough to try only every second offset); the eight best offsets,
at least nine frames apart, are then compared frame by frame at every offset within four frames
of each. The best correlation is the alignment's *strength*. A file may run past either end of
the play-all by up to 10% of its length, because encoders trim or pad title boundaries. Files
shorter than about 3 seconds are not aligned.

### Disc order

`mi_match::derive_disc_order` turns the alignments into an order. A file whose strength is below
0.35 is not located. When two files land on the same range (one covers more than half of the
other), they are duplicates of one title: the stronger keeps the position and the other counts as
not located. Each position records its rank, its start and end, and the play-all chapter that
contains the point one second after its start. The order is *trustworthy* when all of these
hold:

1. at least two files, and at least half of the files, are located;
2. no two located ranges overlap by more than 2 seconds, since back-to-back titles cannot;
3. when the play-all has two or more chapters, at least half of the located files start within
   3 seconds (or 2% of their length, if larger) of a chapter start.

An untrustworthy order is ignored and every file gets the note "Play-all ignored".

### Checking the order against the dialogue

A play-all may hold its titles in an order unrelated to the episode numbers (a shuffled or
best-of disc). Matching therefore checks a trustworthy order before using it. It first assigns
files using the content signals only (dialogue, title hook and length). The located files whose
assignment leads its runner-up by at least the confident margin (0.15) become *anchors*. Along
the play-all, the anchors' episodes should rise. The largest number of anchors whose episodes
rise in play-all order (a longest increasing subsequence), divided by the number of anchors,
must be at least 0.75; otherwise the play-all is shuffled and ignored. With
fewer than two anchors there is nothing to contradict, and the order is used.

### The disc order signal

For a located file, the nearest anchors before and after it along the play-all (excluding the
file itself) bound which episodes it can be:

- an episode outside that range scores 0 (it would break the order);
- the episode that continues an anchor's sequence scores 1: the episode as many places after the
  earlier anchor's episode as the file is after that anchor in the play-all, or as many places
  before the later anchor's episode as the file is before it;
- any other episode inside the range scores 0.6.

With no anchors the signal is not measured, because the order alone favours no episode.

## Matching: assignment and confidence

<!-- owner: match module -->

### Assignment

Scoring files one at a time would let two files claim one episode. The assignment instead
maximises the total combined score over all files at once (`crates/mi-match/src/assign.rs`), so a
file whose best episode fits another file better moves to its own next-best episode or becomes
an extra.

- **Without a usable disc order**, files are matched by Hungarian assignment (the Kuhn-Munkres
  algorithm from the `pathfinding` crate), which finds the one-to-one assignment with the largest
  total. Each file also has its own "no episode" option scored 0.25, so a file is left unmatched
  rather than forced onto a poor episode.
- **With a usable disc order**, the located files, in play-all order, are matched to the episodes,
  in episode-list order, by dynamic programming that keeps both orders rising and maximises the
  total of `score − 0.25`. Skipping an episode is free, because a disc rarely holds every
  episode; leaving a file unmatched costs 0.10. A file found in the play-all is part of the
  disc's main sequence, which holds episodes, so it can be matched with a score as low as 0.15
  instead of 0.25. The files that were not located are then matched by Hungarian assignment to
  the episodes left over.

### Confidence

A file's *margin* is the score of its assigned option minus its *runner-up*: the best other
episode or the "no episode" option, whichever is higher.

| Verdict | Rule | What the app does |
|---|---|---|
| Confident | margin at least 0.15, and the dialogue reaches 0.3 or the title hook 0.5 | pre-approved |
| Check | an episode is assigned with a smaller margin, or without that dialogue or title support | the user reviews it |
| Check | "no episode" was chosen although an episode scores 0.25 or more (another file took it, as with a duplicate rip) | the user reviews it |
| Extra | "no episode" was chosen and every episode scores below 0.25 | left where it is |

A suggestion supported only by length and disc order is never Confident: those signals place a
file among episodes but cannot recognise it.

Each result lists up to five candidate episodes, best first, always including the suggested one,
for the Review screen's dropdown.

### Listening to more of a file

When only sample windows of a long file were transcribed and the result is uncertain, hearing more
can settle it. `mi_match::needs_more_listening` lists every Check file and every Extra whose best
episode came within the confident margin of the "no episode" level; the job transcribes more of
those files and matches again.

### Settings

| `MatchConfig` field | Default | Meaning |
|---|---|---|
| `weights` | 0.55 / 0.15 / 0.10 / 0.20 | dialogue / title hook / length / disc order |
| `confident_margin` | 0.15 | lead over the runner-up for Confident |
| `no_episode_score` | 0.25 | score of the "no episode" option |
| `skip_file_penalty` | 0.10 | cost of leaving a located file unmatched |
| `max_candidates` | 5 | candidates listed per file |
| `identity_floor` | 0.3 | dialogue below which the combined score is scaled down |
| `min_heard_words` | 8 | content words needed to measure dialogue |
| `music_title_boost` | 2.0 | title hook weight factor for mostly-music files |
| `min_order_agreement` | 0.75 | share of anchors in episode order needed to use a play-all |
