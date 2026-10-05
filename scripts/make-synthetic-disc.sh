#!/usr/bin/env bash
# Makes a synthetic "ripped disc" for the end-to-end check of the whole pipeline
# (crates/mi-core/tests/synthetic_disc.rs). macOS only: the speech is generated with `say`.
#
# Usage: scripts/make-synthetic-disc.sh <output folder>
#
# The show, "The Clockwork Garden", is eight short episodes written for this check. The output:
#   disc/title_t00.mkv        the play-all: all eight episodes in episode order, one chapter each
#   disc/title_t01..t08.mkv   the episodes as separate titles, numbered in a shuffled order
#   disc/title_t09.mkv        a bonus clip that is not an episode
#   refs/S01E0N.srt           reference subtitles; episodes 2, 5 and 7 are paraphrased, so their
#                             wording differs from what is spoken (like a real subtitle track)
#   show.json, episodes.json  the show and its episode list, in Media Identifier's JSON shapes
#   truth.json                the right answer for every file
# Every episode opens with the same eight-second instrumental theme, as real shows do. Episodes 3
# and 6 have a steady tone under the speech, and episode 8 a chord.
#
# Needs a full ffmpeg (Homebrew's) on PATH or in MI_TEST_FIXTURE_FFMPEG / MI_TEST_FIXTURE_FFPROBE:
# the app's own minimal build has no encoders. Results on these files say nothing about accuracy
# on real recordings.
set -euo pipefail

OUT="${1:?usage: scripts/make-synthetic-disc.sh <output folder>}"
FFMPEG="${MI_TEST_FIXTURE_FFMPEG:-ffmpeg}"
FFPROBE="${MI_TEST_FIXTURE_FFPROBE:-ffprobe}"
command -v say >/dev/null || { echo "this script needs macOS's say command" >&2; exit 2; }
command -v "${FFMPEG}" >/dev/null || { echo "ffmpeg not found (brew install ffmpeg)" >&2; exit 2; }

TITLES=(
  "The Rusty Gate"
  "Seeds in the Snow"
  "The Brass Beehive"
  "Lanterns for the Moths"
  "The Great Pumpkin Race"
  "A Visitor from the Pond"
  "The Broken Sundial"
  "Midsummer Music"
)

# What is spoken in each episode.
SPOKEN=(
  "Welcome to the Clockwork Garden. This morning the old garden gate will not open. Its hinges are orange with rust and it squeaks like a frightened mouse. Pip the robot gardener fetches a tiny can of oil. One drop, two drops, three drops, and the gate swings open without a sound."
  "Snow has covered every flower bed in a thick white blanket. Pip worries that the sleeping seeds are freezing in the ground. Grandma Fern explains that snow keeps the soil warm, like a woolly quilt. In spring the seeds will wake up hungry and ready to grow."
  "Behind the greenhouse stands a beehive made of polished brass. Its tiny mechanical bees have stopped buzzing. Pip opens the little door and finds a spring wound far too tight. With a careful turn of the key the bees whirr back to life and fly to the clover."
  "Every evening the moths gather around the garden lamps. Tonight the lamps have gone dark and the moths are lost. Pip and Grandma Fern hang paper lanterns along the path. The moths dance in the soft yellow light until the stars come out."
  "It is the day of the great pumpkin race. Each gardener rolls a pumpkin down the long grassy hill. Pip's pumpkin wobbles, bumps over a stone and wins by a whisker. The prize is a golden watering can and a slice of pumpkin pie."
  "A small green frog hops out of the pond and onto the garden bench. He has lost his way home after the rain. Pip builds him a path of flat stepping stones across the lawn. The frog croaks thank you and splashes happily back into the water."
  "The sundial in the middle of the lawn has cracked in two. Without it nobody knows when it is time for lunch. Pip glues the pieces together with honey and tree sap. By noon the shadow points to twelve and everyone sits down to eat."
  "On the longest day of the year the garden holds a concert. The crickets play violins and the bees hum the tune. Pip taps a rhythm on an upturned flower pot. Grandma Fern sings until the sun finally sets behind the hills."
)

# The reference subtitles. Episodes 2, 5 and 7 (indexes 1, 4, 6) say the same things in other
# words, as a subtitle track that was edited for reading would.
REFERENCE=(
  "${SPOKEN[0]}"
  "The flower beds are buried under deep white snow. Pip is afraid the seeds asleep in the earth will freeze. Grandma Fern says the snow is like a warm woolly blanket for the soil. When spring arrives the hungry seeds will wake and start to grow."
  "${SPOKEN[2]}"
  "${SPOKEN[3]}"
  "Today is the great pumpkin race! Every gardener sends a pumpkin rolling down the grassy hill. Pip's pumpkin wobbles and bounces off a stone, but it wins by a whisker. The winner gets a golden watering can and some pumpkin pie."
  "${SPOKEN[5]}"
  "The sundial on the lawn has split into two pieces. Now no one can tell when lunch time is. Pip sticks it back together using tree sap and honey. At midday the shadow points at twelve, and they all sit down for lunch."
  "${SPOKEN[7]}"
)

EXTRA="Hello, I am one of the people who made this show. In this short bonus clip we visit the workshop where the puppets and the little brass robots are built. Every robot takes about three weeks to finish, and each one has more than two hundred tiny parts. Next time we will show you how the garden sets are painted. Thank you for watching, and remember to water your plants."

# Disc titles: title_t0N holds episode FILE_EPISODE[N-1].
FILE_EPISODE=(5 1 7 2 8 3 6 4)

rm -rf "${OUT}"
mkdir -p "${OUT}/work" "${OUT}/disc" "${OUT}/refs"
WORK="${OUT}/work"

# The theme: eight seconds of a little four-note tune.
"${FFMPEG}" -nostdin -v error -y -f lavfi \
  -i "aevalsrc=0.25*sin(2*PI*(392+131*floor(mod(2*t\,4)))*t):s=48000:d=8" \
  -af "aformat=channel_layouts=stereo,afade=t=out:st=7.5:d=0.5" "${WORK}/theme.wav"

# Speech at 48 kHz stereo like a DVD, after the theme, with half a second of silence on each
# side and an optional background sound.
make_audio() { # <text> <background: none|tone|chord> <output wav> [no-theme]
  local text="$1" background="$2" out="$3" theme="${4:-theme}"
  say -r 140 -o "${WORK}/speech.aiff" "${text}"
  local pad="apad=pad_dur=0.5,adelay=500|500"
  case "${background}" in
    none)
      "${FFMPEG}" -nostdin -v error -y -i "${WORK}/speech.aiff" \
        -af "aresample=48000,aformat=channel_layouts=stereo,${pad}" "${out}"
      ;;
    tone)
      "${FFMPEG}" -nostdin -v error -y -i "${WORK}/speech.aiff" -f lavfi -i "sine=frequency=330:sample_rate=48000" \
        -filter_complex "[0:a]aresample=48000,aformat=channel_layouts=stereo,${pad}[s];[1:a]volume=0.08,aformat=channel_layouts=stereo[t];[s][t]amix=inputs=2:duration=first:normalize=0" \
        "${out}"
      ;;
    chord)
      "${FFMPEG}" -nostdin -v error -y -i "${WORK}/speech.aiff" \
        -f lavfi -i "sine=frequency=262:sample_rate=48000" -f lavfi -i "sine=frequency=330:sample_rate=48000" \
        -f lavfi -i "sine=frequency=392:sample_rate=48000" \
        -filter_complex "[0:a]aresample=48000,aformat=channel_layouts=stereo,${pad}[s];[1:a][2:a][3:a]amix=inputs=3,volume=0.12,aformat=channel_layouts=stereo[c];[s][c]amix=inputs=2:duration=first:normalize=0" \
        "${out}"
      ;;
  esac
  if [ "${theme}" = "theme" ]; then
    mv "${out}" "${WORK}/body.wav"
    "${FFMPEG}" -nostdin -v error -y -i "${WORK}/theme.wav" -i "${WORK}/body.wav" \
      -filter_complex "[0:a][1:a]concat=n=2:v=0:a=1" "${out}"
  fi
}

duration_of() {
  "${FFPROBE}" -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 "$1"
}

# Wraps audio in an MKV with a small black video stream and AC-3 audio, like a MakeMKV title.
make_mkv() { # <wav> <output mkv> [ffmetadata]
  local wav="$1" out="$2" meta="${3:-}"
  local args=(-nostdin -v error -y -f lavfi -i "color=c=black:s=160x120:r=10" -i "${wav}")
  if [ -n "${meta}" ]; then
    args+=(-i "${meta}" -map_metadata 2 -map_chapters 2)
  fi
  "${FFMPEG}" "${args[@]}" -map 0:v -map 1:a -shortest -c:v mpeg4 -q:v 20 -c:a ac3 -b:a 192k "${out}"
}

backgrounds=(none none tone none none tone none chord)
for i in "${!SPOKEN[@]}"; do
  echo "Episode $((i + 1)): ${TITLES[$i]}" >&2
  make_audio "${SPOKEN[$i]}" "${backgrounds[$i]}" "${WORK}/e$((i + 1)).wav"
done
make_audio "${EXTRA}" none "${WORK}/extra.wav" no-theme

# Separate titles.
for n in "${!FILE_EPISODE[@]}"; do
  e="${FILE_EPISODE[$n]}"
  make_mkv "${WORK}/e${e}.wav" "${OUT}/disc/title_t0$((n + 1)).mkv"
done
make_mkv "${WORK}/extra.wav" "${OUT}/disc/title_t09.mkv"

# The play-all: every episode in episode order, a chapter at each start.
{
  echo ";FFMETADATA1"
  start=0
  : > "${WORK}/concat.txt"
  for e in 1 2 3 4 5 6 7 8; do
    echo "file 'e${e}.wav'" >> "${WORK}/concat.txt"
    d="$(duration_of "${WORK}/e${e}.wav")"
    end="$(python3 -c "print(round(${start} + ${d} * 1000))")"
    printf '[CHAPTER]\nTIMEBASE=1/1000\nSTART=%s\nEND=%s\ntitle=Chapter %s\n' "${start}" "${end}" "${e}"
    start="${end}"
  done
} > "${WORK}/chapters.txt"
"${FFMPEG}" -nostdin -v error -y -f concat -safe 0 -i "${WORK}/concat.txt" -c copy "${WORK}/playall.wav"
make_mkv "${WORK}/playall.wav" "${OUT}/disc/title_t00.mkv" "${WORK}/chapters.txt"

# Reference subtitles: one cue per sentence, four seconds each.
for i in "${!REFERENCE[@]}"; do
  python3 - "${REFERENCE[$i]}" > "${OUT}/refs/S01E0$((i + 1)).srt" <<'PY'
import re, sys
sentences = [s for s in re.split(r"(?<=[.!?])\s+", sys.argv[1]) if s]
def ts(t):
    return "%02d:%02d:%02d,000" % (t // 3600, t // 60 % 60, t % 60)
for n, s in enumerate(sentences):
    print(n + 1)
    print("%s --> %s" % (ts(1 + n * 4), ts(4 + n * 4)))
    print(s)
    print()
PY
done

python3 - "${OUT}" "${TITLES[@]}" <<'PY'
import json, sys
out, titles = sys.argv[1], sys.argv[2:]
show_ref = {"provider": "local", "id": "clockwork-garden"}
show = {"showRef": show_ref, "name": "The Clockwork Garden", "year": 2026, "kind": "Animation",
        "seasonCount": 1, "episodeCount": len(titles), "url": None}
episodes = [{"showRef": show_ref, "ordering": "aired", "key": {"season": 1, "number": n + 1},
             "title": t, "runtimeS": None, "airdate": None, "summary": None,
             "providerEpisodeId": "e%d" % (n + 1)} for n, t in enumerate(titles)]
json.dump(show, open(out + "/show.json", "w"), indent=2)
json.dump(episodes, open(out + "/episodes.json", "w"), indent=2)
PY

{
  echo "{"
  echo '  "title_t00.mkv": "playAll",'
  for n in "${!FILE_EPISODE[@]}"; do
    echo "  \"title_t0$((n + 1)).mkv\": \"S01E0${FILE_EPISODE[$n]}\","
  done
  echo '  "title_t09.mkv": "extra"'
  echo "}"
} > "${OUT}/truth.json"

rm -rf "${WORK}"
echo "Synthetic disc written to ${OUT}" >&2
