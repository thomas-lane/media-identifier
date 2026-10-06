<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/header-dark.svg">
    <img src="docs/images/header-light.svg" width="440" alt="Media Identifier">
  </picture>
</p>

# Media Identifier

Media Identifier is a desktop app for macOS (Apple Silicon) and Windows that names ripped TV
episode files. Point it at a folder of unlabeled files, such as the titles MakeMKV rips from a
disc, and it listens to each file on your computer, compares what it heard with each episode's
subtitles, lyrics and length, uses the disc's "play all" title to check the order, and suggests
a show, season, episode and title for every file with the evidence behind it. After you review
the suggestions it renames the files in place for Jellyfin, Plex or Kodi, and History can undo
the renames. Audio and video never leave your computer.

<p align="center"><img src="docs/images/identification.svg" width="720" alt="How one file is identified: the words heard in the file are compared with each episode's subtitles, the file is located inside the play-all title, and the signals are combined into the best overall assignment."></p>

<p align="center"><img src="docs/images/review.png" width="720" alt="The Review screen: the files on the left with their suggested episodes and confidence, and on the right the evidence for the selected file: signal scores, the words heard next to the episode's lyrics, and its place in the play-all."></p>

The Review screen, showing the app's built-in sample data.

**Install:** download the latest version from the
[Releases page](https://github.com/thomas-lane/media-identifier/releases/latest): the `.dmg` for
macOS or the `-setup.exe` for Windows. The apps are unsigned, so the first launch needs one extra
step; the [install guide](docs/install.md) shows it.

## Status

Version 0.1.0 is released. CI builds and tests every push on macOS and Windows, and the release
workflow builds both installers. The app itself has been run only on an Apple Silicon Mac; the
Windows build passes every automated test on GitHub's Windows runners but has not been run on a
Windows PC.

| Component | State |
|---|---|
| Folder scan, probing, play-all detection, audio decoding | Tested on generated files and on a synthetic disc; real MakeMKV rips untested |
| ffmpeg/ffprobe sidecars | Built by the release workflow for both systems; macOS sidecars tested on a Mac, Windows sidecars bundled but not run |
| Speech recognition (whisper.cpp on Metal) | Fast and Accurate models run on generated speech and real episodes |
| Model download | The release app downloaded and verified the Accurate model on first launch; resume tested against a local server |
| Episode lists and reference text | TVmaze and LRCLIB called live; SubDL and TMDb tested only against hand-written responses (no key yet) |
| Matching, disc order, confidence | Tested on simulated speech errors and synthetic audio; end-to-end results below |
| Identification pipeline | Tested with scripted services, and end to end on the files below |
| Rename, copy, CSV export, undo | Tested on temporary folders on macOS and Windows, including interrupted saves and journal write failures |
| UI | All screens tested in jsdom against the mock backend; the release app starts and shows the Start screen; a full identification through the window has not been run |
| macOS release | `.dmg` and signed update file built by the release workflow; unsigned (ad-hoc); the downloaded app starts |
| Windows release | Per-user installer and signed update file built by the release workflow; needs no Visual C++ runtime DLLs (checked in the workflow); not run on a Windows PC |
| Auto-update | The update feed and both update files download without an account and verify against the app's key; installing an update has not been exercised yet (it needs a second release) |
| Windows processor check | Untested on Windows |

End-to-end checks with the whole pipeline (command-line example, same engine as the app):

| Files | Model | Result |
|---|---|---|
| Synthetic disc: 8 short episodes spoken by macOS `say`, a play-all and an extra; 3 of 8 reference subtitles paraphrased | Fast | 10 of 10 files right (8 episodes Confident, play-all, extra) |
| 4 real one-hour episodes of one season, renamed `title_t00`–`t03`, with the season's subtitle files as reference text | Fast and Accurate | 4 of 4 right, all Confident |
| The same 4 episodes with no subtitles (episode titles and summaries only) | Fast | 3 of 4 right; 2 marked Check, including the wrong one |

These are small samples, not accuracy measurements.

## Documentation

- [Architecture](docs/architecture.md): crates, data flow, data folders
- [How identification works](docs/identification.md)
- [Saving and undo](docs/saving.md): naming, rename, copy, CSV, History
- [Online sources](docs/sources.md): services, keys, limits, attribution
- [Install](docs/install.md)
- [Development](docs/development.md): build, test, release
- [Glossary](docs/glossary.md)
- [Developer guide for agents](AGENTS.md)

Episode data comes from [TVmaze](https://www.tvmaze.com) (CC BY-SA 4.0). Licensed MIT; see
[LICENSE](LICENSE) and [THIRD_PARTY.md](THIRD_PARTY.md).
