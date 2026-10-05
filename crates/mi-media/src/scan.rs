//! Finding and classifying the video files in a folder.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use mi_types::{
    CancelFlag, DiscFolderFormat, FileId, FileRole, MediaFile, Probe, ScanSummary, ScanWarning,
};

use crate::{MediaError, PlayAllThresholds, Sidecars};

/// File extensions (lowercase, without the dot) treated as video files.
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "mov", "avi", "ts", "m2ts", "mts", "mpg", "mpeg", "vob",
];

/// How many files are probed at the same time. Probing mostly waits on the disk or network share,
/// so a few in parallel shorten a scan of a NAS folder without flooding it.
const PROBE_THREADS: usize = 4;

/// Scan options.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Descend into subfolders. Disc structure folders (`VIDEO_TS`, `BDMV`) are never entered.
    pub recursive: bool,
    /// Files shorter than this many seconds get [`mi_types::FileRole::Ignored`] (menus, logos).
    pub min_duration_s: f64,
    /// Play-all detection tolerances.
    pub play_all: PlayAllThresholds,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            recursive: false,
            min_duration_s: 20.0,
            play_all: PlayAllThresholds::default(),
        }
    }
}

/// Scans `folder`: lists video files, probes each one, marks the play-all title, guesses the show
/// name from the folder or disc name, and records warnings.
///
/// Contract:
/// - Files are found by extension ([`VIDEO_EXTENSIONS`], any case). Names starting with `.` are
///   skipped: they are hidden files, including the `._name.mkv` metadata files macOS writes on
///   network shares, which have a video extension but no video.
/// - A disc structure folder (`VIDEO_TS` or `BDMV`, or the scanned folder itself when it holds
///   `VIDEO_TS.IFO` or `index.bdmv`) adds an `UnsupportedDiscFolder` warning and its files are not
///   listed; see [`ScanWarning::UnsupportedDiscFolder`] for why.
/// - `files` are sorted by `FileId` (relative path with `/`), so order is stable across runs.
/// - Roles: unprobeable files are kept with `probe: None`, role `Ignored`, and an `Unreadable`
///   warning; files without audio are `Ignored` with a `NoAudio` warning; files shorter than
///   `min_duration_s` are `Ignored` silently; the rest are `Candidate`.
/// - The play-all is chosen among the candidates by [`crate::detect_play_all`]; its role becomes
///   `PlayAll` and `candidate_count` excludes it.
/// - `MissingShortTitles` is added when the play-all has more chapters than there are candidates
///   and at least half of the candidates match a chapter's length. The second condition means the
///   chapters mark one title each; a disc whose play-all has several chapters per episode would
///   otherwise always trigger the warning.
/// - `show_guess` is [`guess_show`] of the folder.
///
/// Errors: `Cancelled` when `cancel` is set; `Io` when the folder cannot be listed. Problems with
/// single files become warnings instead.
pub fn scan_folder(
    sidecars: &Sidecars,
    folder: &Path,
    options: &ScanOptions,
    cancel: &CancelFlag,
) -> crate::Result<ScanSummary> {
    crate::check_cancel(cancel)?;
    let listing = list_video_files(folder, options.recursive)?;
    let probes = probe_all(sidecars, &listing.files, cancel)?;

    let mut warnings = listing.warnings;
    let mut files = Vec::with_capacity(listing.files.len());
    for (found, probe) in listing.files.into_iter().zip(probes) {
        let (probe, role) = match probe {
            Ok(probe) => {
                let role = if probe.audio_streams.is_empty() {
                    warnings.push(ScanWarning::NoAudio {
                        file_id: found.id.clone(),
                    });
                    FileRole::Ignored
                } else if probe.duration_s < options.min_duration_s {
                    FileRole::Ignored
                } else {
                    FileRole::Candidate
                };
                (Some(probe), role)
            }
            Err(error) => {
                warnings.push(ScanWarning::Unreadable {
                    file_id: found.id.clone(),
                    reason: plain_reason(&error),
                });
                (None, FileRole::Ignored)
            }
        };
        files.push(MediaFile {
            id: found.id,
            file_name: found
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: found.path,
            size_bytes: found.size_bytes,
            probe,
            role,
        });
    }
    let summary = classify(folder, files, warnings, &options.play_all);
    Ok(summary)
}

/// Marks the play-all, counts candidates, adds the missing-titles warning and guesses the show.
/// Split from [`scan_folder`] so it can be tested without ffprobe.
fn classify(
    folder: &Path,
    mut files: Vec<MediaFile>,
    mut warnings: Vec<ScanWarning>,
    thresholds: &PlayAllThresholds,
) -> ScanSummary {
    let play_all = crate::detect_play_all(&files, thresholds);
    if let Some(info) = &play_all
        && let Some(file) = files.iter_mut().find(|f| f.id == info.file_id)
    {
        file.role = FileRole::PlayAll;
    }
    let candidate_count = files
        .iter()
        .filter(|f| f.role == FileRole::Candidate)
        .count() as u32;
    if let Some(info) = &play_all
        && info.chapter_count > candidate_count
        && info.chapters_matched * 2 >= candidate_count
    {
        warnings.push(ScanWarning::MissingShortTitles {
            chapters: info.chapter_count,
            short_files: candidate_count,
        });
    }
    ScanSummary {
        folder: folder.to_path_buf(),
        files,
        play_all,
        candidate_count,
        show_guess: guess_show(folder),
        warnings,
    }
}

/// A video file found on disk, before probing.
#[derive(Debug, Clone, PartialEq)]
struct Found {
    id: FileId,
    path: PathBuf,
    size_bytes: u64,
}

#[derive(Debug, Default)]
struct Listing {
    files: Vec<Found>,
    warnings: Vec<ScanWarning>,
}

/// Lists video files under `folder`, sorted by id, and reports disc structure folders.
fn list_video_files(folder: &Path, recursive: bool) -> crate::Result<Listing> {
    let mut listing = Listing::default();
    if let Some(format) = disc_folder_format(folder) {
        listing.warnings.push(ScanWarning::UnsupportedDiscFolder {
            folder: String::new(),
            format,
        });
        return Ok(listing);
    }
    walk(folder, "", recursive, &mut listing)?;
    listing.files.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(listing)
}

fn walk(dir: &Path, prefix: &str, recursive: bool, listing: &mut Listing) -> crate::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let path = entry.path();
        // `file_type` does not follow symbolic links, so a link to a parent folder cannot loop.
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if let Some(format) = disc_folder_format(&path) {
                listing.warnings.push(ScanWarning::UnsupportedDiscFolder {
                    folder: relative,
                    format,
                });
            } else if recursive {
                walk(&path, &relative, recursive, listing)?;
            }
            continue;
        }
        if !has_video_extension(&name) {
            continue;
        }
        // Follows a link to a file; a broken link is skipped.
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if metadata.is_file() {
            listing.files.push(Found {
                id: FileId(relative),
                path,
                size_bytes: metadata.len(),
            });
        }
    }
    Ok(())
}

fn has_video_extension(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Recognises a disc structure folder by its name (`VIDEO_TS`, `BDMV`) or its index file
/// (`VIDEO_TS.IFO`, `index.bdmv`), in any case.
fn disc_folder_format(dir: &Path) -> Option<DiscFolderFormat> {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_uppercase());
    match name.as_deref() {
        Some("VIDEO_TS") => return Some(DiscFolderFormat::Dvd),
        Some("BDMV") => return Some(DiscFolderFormat::BluRay),
        _ => {}
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        match entry
            .file_name()
            .to_string_lossy()
            .to_ascii_uppercase()
            .as_str()
        {
            "VIDEO_TS.IFO" => return Some(DiscFolderFormat::Dvd),
            "INDEX.BDMV" => return Some(DiscFolderFormat::BluRay),
            _ => {}
        }
    }
    None
}

/// Probes every file, [`PROBE_THREADS`] at a time, keeping the input order. Per-file errors are
/// returned in place; cancellation aborts the whole scan.
fn probe_all(
    sidecars: &Sidecars,
    files: &[Found],
    cancel: &CancelFlag,
) -> crate::Result<Vec<crate::Result<Probe>>> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<crate::Result<Probe>>>> =
        Mutex::new((0..files.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..PROBE_THREADS.min(files.len()) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= files.len() || cancel.is_cancelled() {
                        break;
                    }
                    let result = crate::probe(sidecars, &files[i].path, cancel);
                    results.lock().expect("probe results lock")[i] = Some(result);
                }
            });
        }
    });
    crate::check_cancel(cancel)?;
    let results = results.into_inner().expect("probe results lock");
    Ok(results
        .into_iter()
        .map(|r| r.unwrap_or(Err(MediaError::Cancelled)))
        .collect())
}

/// A short explanation of a probe failure for the Confirm show screen.
fn plain_reason(error: &MediaError) -> String {
    match error {
        MediaError::ToolFailed { message, .. } => {
            // ffprobe's last stderr line is the most specific, e.g.
            // "/x/t.mkv: Invalid data found when processing input".
            let last = message.lines().last().unwrap_or(message).trim();
            let detail = last.rsplit(": ").next().unwrap_or(last);
            format!("The file could not be read ({detail}).")
        }
        MediaError::BadProbe { .. } => "The file's length could not be determined.".to_owned(),
        MediaError::Io(e) => format!("The file could not be opened ({e})."),
        other => other.to_string(),
    }
}

/// Words removed from folder names because they describe the disc or rip, not the show.
fn is_marker(token: &str) -> bool {
    let t = token.to_ascii_lowercase();
    let digits_after = |prefix: &str| {
        t.strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
    };
    let season_disc = || {
        // S1D2, S03D1
        t.strip_prefix('s')
            .and_then(|rest| rest.split_once('d'))
            .is_some_and(|(s, d)| {
                !s.is_empty()
                    && !d.is_empty()
                    && s.chars().all(|c| c.is_ascii_digit())
                    && d.chars().all(|c| c.is_ascii_digit())
            })
    };
    matches!(
        t.as_str(),
        "dvd" | "bd" | "bluray" | "blu-ray" | "makemkv" | "rip" | "disc" | "disk" | "season"
    ) || digits_after("d")
        || digits_after("s")
        || digits_after("disc")
        || digits_after("disk")
        || digits_after("dvd")
        || digits_after("bd")
        || digits_after("season")
        || season_disc()
        || is_bracketed_year(&t)
}

fn is_bracketed_year(t: &str) -> bool {
    let inner = t
        .strip_prefix('(')
        .and_then(|r| r.strip_suffix(')'))
        .or_else(|| t.strip_prefix('[').and_then(|r| r.strip_suffix(']')));
    inner.is_some_and(|y| y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()))
}

/// Guesses the show name from a folder path, for the initial search on the Confirm show screen.
///
/// The folder name is split into words at spaces, `_` and `.`; disc and rip markers are dropped
/// (`D1`, `Disc 2`, `DISC_2`, `S03`, `Season 3`, `S1D2`, `DVD`, `BD`, `Blu-ray`, a bracketed year
/// such as `(1973)`), as is a number that follows `Disc` or `Season`. An all-capitals name (disc
/// volume labels such as `SCHOOLHOUSE_ROCK_D1`) is changed to title case. When nothing is left
/// (`Season 2`, `Disc 1`), the parent folder is tried, up to three levels.
pub fn guess_show(folder: &Path) -> Option<String> {
    folder
        .ancestors()
        .take(3)
        .filter_map(|dir| dir.file_name())
        .find_map(|name| clean_name(&name.to_string_lossy()))
}

fn clean_name(name: &str) -> Option<String> {
    let spaced: String = name
        .chars()
        .map(|c| if c == '_' || c == '.' { ' ' } else { c })
        .collect();
    let tokens: Vec<&str> = spaced.split_whitespace().collect();
    let mut kept = Vec::new();
    let mut skip_number = false;
    for token in tokens {
        if skip_number && token.chars().all(|c| c.is_ascii_digit()) {
            skip_number = false;
            continue;
        }
        let lower = token.to_ascii_lowercase();
        skip_number = matches!(
            lower.as_str(),
            "disc" | "disk" | "season" | "volume" | "vol"
        );
        if matches!(lower.as_str(), "volume" | "vol") {
            // "Volume 2" is usually part of a release's title; keep it.
            kept.push(token);
            skip_number = false;
            continue;
        }
        if lower == "-" || is_marker(token) {
            continue;
        }
        kept.push(token);
    }
    if kept.is_empty() {
        return None;
    }
    let joined = kept.join(" ");
    let has_lower = joined.chars().any(|c| c.is_lowercase());
    let has_upper = joined.chars().any(|c| c.is_uppercase());
    Some(if has_upper && !has_lower {
        title_case(&kept)
    } else {
        joined
    })
}

fn title_case(words: &[&str]) -> String {
    const SMALL: &[&str] = &[
        "a", "an", "and", "at", "for", "in", "of", "on", "or", "the", "to",
    ];
    words
        .iter()
        .enumerate()
        .map(|(i, word)| {
            let lower = word.to_lowercase();
            if i > 0 && SMALL.contains(&lower.as_str()) {
                return lower;
            }
            let mut chars = lower.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play_all::tests::file;

    #[test]
    fn guesses_show_from_disc_labels_and_folders() {
        let cases = [
            ("/rips/SCHOOLHOUSE_ROCK_D1", Some("Schoolhouse Rock")),
            ("/rips/FRIENDS_S1_D2", Some("Friends")),
            ("/rips/LORD_OF_THE_RINGS_DISC_2", Some("Lord of the Rings")),
            ("/rips/The Office Season 3 Disc 2", Some("The Office")),
            ("/rips/The Office/Season 3/Disc 2", Some("The Office")),
            ("/rips/Fawlty.Towers.S01D1", Some("Fawlty Towers")),
            (
                "/rips/Schoolhouse Rock! (1973) - DVD",
                Some("Schoolhouse Rock!"),
            ),
            (
                "/rips/Schoolhouse Rock Volume 2",
                Some("Schoolhouse Rock Volume 2"),
            ),
            ("/rips/Mixed Case D1", Some("Mixed Case")),
            ("D1", None),
        ];
        for (path, expected) in cases {
            assert_eq!(guess_show(Path::new(path)).as_deref(), expected, "{path}");
        }
    }

    #[test]
    fn lists_videos_sorted_skipping_hidden_files_and_other_extensions() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "title_t02.mkv",
            "title_t01.MKV",
            "._title_t01.mkv",
            ".hidden.mp4",
            "notes.txt",
            "cover.jpg",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        std::fs::create_dir(dir.path().join("Extras")).unwrap();
        std::fs::write(dir.path().join("Extras/bonus.m4v"), b"xy").unwrap();

        let flat = list_video_files(dir.path(), false).unwrap();
        let ids: Vec<&str> = flat.files.iter().map(|f| f.id.0.as_str()).collect();
        assert_eq!(ids, ["title_t01.MKV", "title_t02.mkv"]);
        assert!(flat.warnings.is_empty());

        let deep = list_video_files(dir.path(), true).unwrap();
        let ids: Vec<&str> = deep.files.iter().map(|f| f.id.0.as_str()).collect();
        assert_eq!(ids, ["Extras/bonus.m4v", "title_t01.MKV", "title_t02.mkv"]);
        assert_eq!(deep.files[0].size_bytes, 2);
    }

    #[test]
    fn reports_disc_folders_instead_of_listing_their_files() {
        let dir = tempfile::tempdir().unwrap();
        let video_ts = dir.path().join("VIDEO_TS");
        std::fs::create_dir(&video_ts).unwrap();
        std::fs::write(video_ts.join("VIDEO_TS.IFO"), b"x").unwrap();
        std::fs::write(video_ts.join("VTS_01_1.VOB"), b"x").unwrap();
        std::fs::create_dir_all(dir.path().join("BDMV/STREAM")).unwrap();
        std::fs::write(dir.path().join("BDMV/STREAM/00001.m2ts"), b"x").unwrap();
        std::fs::write(dir.path().join("title_t00.mkv"), b"x").unwrap();

        let listing = list_video_files(dir.path(), true).unwrap();
        let ids: Vec<&str> = listing.files.iter().map(|f| f.id.0.as_str()).collect();
        assert_eq!(ids, ["title_t00.mkv"]);
        assert_eq!(
            listing.warnings,
            [
                ScanWarning::UnsupportedDiscFolder {
                    folder: "BDMV".into(),
                    format: DiscFolderFormat::BluRay
                },
                ScanWarning::UnsupportedDiscFolder {
                    folder: "VIDEO_TS".into(),
                    format: DiscFolderFormat::Dvd
                },
            ]
        );

        // Choosing the VIDEO_TS folder itself, or a renamed copy of it, is reported the same way.
        let itself = list_video_files(&video_ts, false).unwrap();
        assert!(itself.files.is_empty());
        assert_eq!(
            itself.warnings,
            [ScanWarning::UnsupportedDiscFolder {
                folder: String::new(),
                format: DiscFolderFormat::Dvd
            }]
        );
        let renamed = dir.path().join("MY_DISC");
        std::fs::rename(&video_ts, &renamed).unwrap();
        assert_eq!(
            disc_folder_format(&renamed),
            Some(DiscFolderFormat::Dvd),
            "detected by VIDEO_TS.IFO"
        );
    }

    fn summary_for(files: Vec<MediaFile>) -> ScanSummary {
        classify(
            Path::new("/rips/SCHOOLHOUSE_ROCK_D1"),
            files,
            Vec::new(),
            &PlayAllThresholds::default(),
        )
    }

    #[test]
    fn marks_play_all_and_warns_about_missing_short_titles() {
        // The play-all has 6 one-title chapters; only 4 titles were ripped.
        let chapters = [190.0, 100.0, 185.0, 110.0, 178.0, 201.0];
        let mut files = vec![file("title_t00.mkv", chapters.iter().sum(), &chapters)];
        for (i, d) in [190.0, 185.0, 178.0, 201.0].iter().enumerate() {
            files.push(file(&format!("title_t0{}.mkv", i + 1), *d, &[]));
        }
        let s = summary_for(files);
        assert_eq!(s.files[0].role, FileRole::PlayAll);
        assert_eq!(s.candidate_count, 4);
        assert_eq!(s.show_guess.as_deref(), Some("Schoolhouse Rock"));
        assert_eq!(
            s.warnings,
            [ScanWarning::MissingShortTitles {
                chapters: 6,
                short_files: 4
            }]
        );
    }

    #[test]
    fn several_chapters_per_episode_do_not_warn() {
        let mut files = vec![file("all.mkv", 5280.0, &[220.0; 24])];
        for i in 0..4 {
            files.push(file(&format!("e{i}.mkv"), 1320.0, &[]));
        }
        let s = summary_for(files);
        assert!(s.play_all.is_some());
        assert_eq!(s.candidate_count, 4);
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    }

    #[test]
    fn plain_reason_keeps_ffprobes_last_message() {
        let e = MediaError::ToolFailed {
            tool: crate::Tool::Ffprobe,
            path: "/x/t.mkv".into(),
            message:
                "exited with exit status: 1: /x/t.mkv: Invalid data found when processing input"
                    .into(),
        };
        assert_eq!(
            plain_reason(&e),
            "The file could not be read (Invalid data found when processing input)."
        );
    }
}
