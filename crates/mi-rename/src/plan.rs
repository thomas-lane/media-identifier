//! Building a rename plan (the preview).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use mi_types::{
    Episode, FileId, FileMatch, FileRole, MediaFile, PlanConflict, RenameItem, RenamePlan,
    RenamePlanRequest, ReviewDecision, SaveMode, Show, UntouchedFile, UntouchedReason, Verdict,
};

use unicode_normalization::UnicodeNormalization;

use crate::{RenameError, render_relative_path};

/// What a plan is built from (supplied by `mi-core` from the job's results).
#[derive(Debug, Clone, Copy)]
pub struct PlanContext<'a> {
    /// The show.
    pub show: &'a Show,
    /// All candidate episodes.
    pub episodes: &'a [Episode],
    /// The scanned files.
    pub files: &'a [MediaFile],
    /// The job's match results (identifies the play-all).
    pub matches: &'a [FileMatch],
}

/// What [`build_plan`] reads from the disk (the real file system in the app, sets in tests).
#[derive(Clone, Copy)]
pub struct DiskView<'a> {
    /// Whether anything exists at a path.
    pub exists: &'a dyn Fn(&Path) -> bool,
    /// The size of the file at a path; `None` when there is no file there.
    pub file_size: &'a dyn Fn(&Path) -> Option<u64>,
}

impl std::fmt::Debug for DiskView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DiskView")
    }
}

impl DiskView<'static> {
    /// The real file system.
    pub fn real() -> Self {
        fn exists(path: &Path) -> bool {
            std::fs::symlink_metadata(path).is_ok()
        }
        fn file_size(path: &Path) -> Option<u64> {
            std::fs::metadata(path)
                .ok()
                .filter(std::fs::Metadata::is_file)
                .map(|m| m.len())
        }
        Self {
            exists: &exists,
            file_size: &file_size,
        }
    }
}

/// Builds the plan for `request`, reading the disk only through `disk`.
///
/// Each scanned file is placed by the first rule that applies:
/// 1. The play-all (scan role `PlayAll`, or match verdict `PlayAll`) is untouched, whatever the
///    decision says, because it is the answer key and not an episode.
/// 2. A file the scan ignored (role `Ignored`: unreadable, no audio or too short) is untouched as
///    skipped, because it was never identified.
/// 3. `Approved` gets a [`RenameItem`] whose target is the episode's name under the save root:
///    `root` for rename in place, `destination` for copy, and the scanned folder for a CSV export
///    (the CSV lists the names a rename would give).
/// 4. `NotAnEpisode` is untouched as an extra; `Skip` and `Pending` are untouched as skipped.
/// 5. A file without a decision is untouched: as an extra when its verdict is `Extra`, otherwise
///    as skipped. Files are renamed only on an explicit approval.
///
/// With `save_heard_subtitles`, each item also gets `<target stem>.srt`.
///
/// Conflicts (for a CSV export, only [`PlanConflict::ListExists`]):
/// - `SourceChanged`: the file at an item's scanned path is missing or has a size other than the
///   scanned one, so the name may now belong to a different file (a later rip, or an earlier
///   save of this job).
/// - `DuplicateTarget`: two items whose targets are equal ignoring letter case and Unicode
///   normalization, because macOS and Windows file systems usually treat such names as the same
///   file.
/// - `TargetExists`: a target that exists. For rename in place, a target that is the current path
///   of another item in the plan is not a conflict, because that file moves away first (applying
///   uses temporary names); an item whose target is its own current path is left as is.
/// - `ListExists`: the CSV file exists and the request does not say to replace it.
///
/// Items are sorted by target path. An approval naming an episode missing from
/// `context.episodes` is an error ([`RenameError::UnknownEpisode`]), as is an invalid custom
/// template ([`RenameError::BadTemplate`]).
pub fn build_plan(
    context: PlanContext<'_>,
    request: &RenamePlanRequest,
    disk: DiskView<'_>,
) -> crate::Result<RenamePlan> {
    let decisions: HashMap<&FileId, &ReviewDecision> = request
        .decisions
        .iter()
        .map(|d| (&d.file_id, &d.decision))
        .collect();
    let verdicts: HashMap<&FileId, Verdict> = context
        .matches
        .iter()
        .map(|m| (&m.file_id, m.confidence.verdict))
        .collect();

    let mut items = Vec::new();
    let mut untouched = Vec::new();
    for file in context.files {
        let verdict = verdicts.get(&file.id).copied();
        let leave = |reason| UntouchedFile {
            file_id: file.id.clone(),
            path: file.path.clone(),
            reason,
        };
        if file.role == FileRole::PlayAll || verdict == Some(Verdict::PlayAll) {
            untouched.push(leave(UntouchedReason::PlayAll));
            continue;
        }
        if file.role == FileRole::Ignored {
            untouched.push(leave(UntouchedReason::Skipped));
            continue;
        }
        match decisions.get(&file.id) {
            Some(ReviewDecision::Approved { episode }) => {
                let ep = context
                    .episodes
                    .iter()
                    .find(|e| e.key == *episode)
                    .ok_or_else(|| {
                        RenameError::UnknownEpisode(format!(
                            "S{:02}E{:02}",
                            episode.season, episode.number
                        ))
                    })?;
                let ext = file
                    .path
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let relative = render_relative_path(&request.naming, context.show, ep, &ext)?;
                let root = match &request.mode {
                    SaveMode::RenameInPlace { root } => root.clone(),
                    SaveMode::CopyToFolder { destination } => destination.clone(),
                    SaveMode::ExportList { .. } => scan_root(&file.path, &file.id),
                };
                let to = root.join(relative);
                let heard_subtitles_to = request
                    .save_heard_subtitles
                    .then(|| to.with_extension("srt"));
                items.push(RenameItem {
                    file_id: file.id.clone(),
                    from: file.path.clone(),
                    to,
                    episode: ep.key,
                    title: ep.title.clone(),
                    heard_subtitles_to,
                    size_bytes: file.size_bytes,
                });
            }
            Some(ReviewDecision::NotAnEpisode) => untouched.push(leave(UntouchedReason::Extra)),
            Some(ReviewDecision::Skip | ReviewDecision::Pending) => {
                untouched.push(leave(UntouchedReason::Skipped))
            }
            None if verdict == Some(Verdict::Extra) => {
                untouched.push(leave(UntouchedReason::Extra))
            }
            None => untouched.push(leave(UntouchedReason::Skipped)),
        }
    }
    items.sort_by(|a, b| a.to.cmp(&b.to));

    let conflicts = match &request.mode {
        SaveMode::ExportList {
            destination,
            replace,
        } => {
            if !replace && (disk.exists)(destination) {
                vec![PlanConflict::ListExists {
                    path: destination.clone(),
                }]
            } else {
                Vec::new()
            }
        }
        SaveMode::RenameInPlace { .. } => find_conflicts(&items, disk, true),
        SaveMode::CopyToFolder { .. } => find_conflicts(&items, disk, false),
    };

    Ok(RenamePlan {
        job_id: request.job_id.clone(),
        request: request.clone(),
        mode: request.mode.clone(),
        items,
        untouched,
        conflicts,
    })
}

/// The folder that was scanned: the file's path with its id's components removed.
pub(crate) fn scan_root(path: &Path, id: &FileId) -> PathBuf {
    let depth = id.0.split('/').filter(|p| !p.is_empty()).count();
    path.ancestors()
        .nth(depth)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| path.parent().map(Path::to_path_buf).unwrap_or_default())
}

/// A comparison key that treats names differing only in letter case or in Unicode
/// normalization as equal. APFS, HFS+ and NTFS look names up that way: `Café` written with a
/// composed `é` (one code point, NFC) and with `e` plus a combining accent (NFD, which HFS+
/// stores) name the same file.
pub(crate) fn fold(path: &Path) -> String {
    path.to_string_lossy()
        .nfc()
        .collect::<String>()
        .to_lowercase()
}

fn find_conflicts(
    items: &[RenameItem],
    disk: DiskView<'_>,
    sources_move_away: bool,
) -> Vec<PlanConflict> {
    let mut conflicts = Vec::new();
    let exists = disk.exists;

    for item in items {
        if (disk.file_size)(&item.from) != Some(item.size_bytes) {
            conflicts.push(PlanConflict::SourceChanged {
                file_id: item.file_id.clone(),
                path: item.from.clone(),
            });
        }
    }

    let mut by_target: HashMap<String, Vec<&RenameItem>> = HashMap::new();
    for item in items {
        by_target.entry(fold(&item.to)).or_default().push(item);
    }
    let mut duplicated: HashSet<String> = HashSet::new();
    let mut groups: Vec<_> = by_target.into_iter().filter(|(_, v)| v.len() > 1).collect();
    groups.sort_by(|a, b| a.1[0].to.cmp(&b.1[0].to));
    for (key, group) in groups {
        conflicts.push(PlanConflict::DuplicateTarget {
            file_ids: group.iter().map(|i| i.file_id.clone()).collect(),
            path: group[0].to.clone(),
        });
        duplicated.insert(key);
    }

    let sources: HashSet<String> = if sources_move_away {
        items.iter().map(|i| fold(&i.from)).collect()
    } else {
        HashSet::new()
    };
    for item in items {
        if fold(&item.from) == fold(&item.to) || duplicated.contains(&fold(&item.to)) {
            // Already named correctly, or already reported.
        } else if exists(&item.to) && !sources.contains(&fold(&item.to)) {
            conflicts.push(PlanConflict::TargetExists {
                file_id: item.file_id.clone(),
                path: item.to.clone(),
            });
        }
        if let Some(srt) = &item.heard_subtitles_to
            && exists(srt)
        {
            conflicts.push(PlanConflict::TargetExists {
                file_id: item.file_id.clone(),
                path: srt.clone(),
            });
        }
    }
    conflicts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{episode, file_match, media_file, show};
    use mi_types::{EpisodeKey, FileDecision, JobId, NamingScheme};

    fn request(mode: SaveMode, decisions: Vec<(&str, ReviewDecision)>) -> RenamePlanRequest {
        RenamePlanRequest {
            job_id: JobId("job".into()),
            decisions: decisions
                .into_iter()
                .map(|(id, decision)| FileDecision {
                    file_id: FileId(id.into()),
                    decision,
                })
                .collect(),
            mode,
            naming: NamingScheme::JellyfinPlex,
            save_heard_subtitles: false,
        }
    }

    fn approve(season: u32, number: u32) -> ReviewDecision {
        ReviewDecision::Approved {
            episode: EpisodeKey { season, number },
        }
    }

    struct Fixture {
        root: PathBuf,
        show: Show,
        episodes: Vec<Episode>,
        files: Vec<MediaFile>,
        matches: Vec<FileMatch>,
    }

    impl Fixture {
        fn new() -> Self {
            let root = PathBuf::from("/disc");
            let files = vec![
                media_file(&root, "title_t00.mkv", FileRole::PlayAll),
                media_file(&root, "title_t01.mkv", FileRole::Candidate),
                media_file(&root, "title_t02.mkv", FileRole::Candidate),
                media_file(&root, "title_t03.mkv", FileRole::Candidate),
                media_file(&root, "title_t04.mkv", FileRole::Candidate),
                media_file(&root, "sub/title_t05.mkv", FileRole::Candidate),
            ];
            let matches = vec![
                file_match("title_t00.mkv", Verdict::PlayAll),
                file_match("title_t01.mkv", Verdict::Confident),
                file_match("title_t02.mkv", Verdict::Check),
                file_match("title_t03.mkv", Verdict::Extra),
                file_match("title_t04.mkv", Verdict::Extra),
            ];
            Self {
                root,
                show: show("Show", Some(1973)),
                episodes: vec![
                    episode(1, 1, "One"),
                    episode(1, 2, "Two"),
                    episode(1, 3, "Three"),
                ],
                files,
                matches,
            }
        }

        fn context(&self) -> PlanContext<'_> {
            PlanContext {
                show: &self.show,
                episodes: &self.episodes,
                files: &self.files,
                matches: &self.matches,
            }
        }

        fn in_place(&self) -> SaveMode {
            SaveMode::RenameInPlace {
                root: self.root.clone(),
            }
        }
    }

    fn nothing_exists(_: &Path) -> bool {
        false
    }

    /// Every scanned file is still there with its scanned size (0 in these fixtures).
    fn scanned_size(_: &Path) -> Option<u64> {
        Some(0)
    }

    fn disk<'a>(exists: &'a dyn Fn(&Path) -> bool) -> DiskView<'a> {
        DiskView {
            exists,
            file_size: &scanned_size,
        }
    }

    fn target(root: &Path, number: u32, title: &str) -> PathBuf {
        root.join("Show (1973)")
            .join("Season 01")
            .join(format!("Show (1973) - S01E{number:02} - {title}.mkv"))
    }

    #[test]
    fn approved_files_get_targets_and_the_rest_stay() {
        let f = Fixture::new();
        let req = request(
            f.in_place(),
            vec![
                ("title_t00.mkv", approve(1, 3)), // the play-all is never renamed
                ("title_t01.mkv", approve(1, 2)),
                ("title_t02.mkv", ReviewDecision::Pending),
                ("title_t03.mkv", ReviewDecision::NotAnEpisode),
                ("sub/title_t05.mkv", approve(1, 1)),
            ],
        );
        let plan = build_plan(f.context(), &req, disk(&nothing_exists)).unwrap();
        let targets: Vec<_> = plan
            .items
            .iter()
            .map(|i| (i.file_id.0.as_str(), i.to.clone()))
            .collect();
        assert_eq!(
            targets,
            vec![
                ("sub/title_t05.mkv", target(&f.root, 1, "One")),
                ("title_t01.mkv", target(&f.root, 2, "Two")),
            ]
        );
        let reasons: Vec<_> = plan
            .untouched
            .iter()
            .map(|u| (u.file_id.0.as_str(), u.reason))
            .collect();
        assert_eq!(
            reasons,
            vec![
                ("title_t00.mkv", UntouchedReason::PlayAll),
                ("title_t02.mkv", UntouchedReason::Skipped),
                ("title_t03.mkv", UntouchedReason::Extra),
                ("title_t04.mkv", UntouchedReason::Extra), // no decision, verdict Extra
            ]
        );
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn a_play_all_found_only_by_matching_is_untouched() {
        let mut f = Fixture::new();
        f.files[0].role = FileRole::Candidate;
        let req = request(f.in_place(), vec![("title_t00.mkv", approve(1, 1))]);
        let plan = build_plan(f.context(), &req, disk(&nothing_exists)).unwrap();
        assert!(plan.items.is_empty());
        assert_eq!(plan.untouched[0].reason, UntouchedReason::PlayAll);
    }

    #[test]
    fn unknown_episode_is_an_error() {
        let f = Fixture::new();
        let req = request(f.in_place(), vec![("title_t01.mkv", approve(9, 9))]);
        let err = build_plan(f.context(), &req, disk(&nothing_exists)).unwrap_err();
        assert!(matches!(err, RenameError::UnknownEpisode(s) if s == "S09E09"));
    }

    #[test]
    fn duplicate_targets_ignore_letter_case() {
        let mut f = Fixture::new();
        let mut req = request(
            f.in_place(),
            vec![
                ("title_t01.mkv", approve(1, 2)),
                ("title_t02.mkv", approve(1, 2)),
            ],
        );
        let plan = build_plan(f.context(), &req, disk(&nothing_exists)).unwrap();
        assert_eq!(
            plan.conflicts,
            vec![PlanConflict::DuplicateTarget {
                file_ids: vec![
                    FileId("title_t01.mkv".into()),
                    FileId("title_t02.mkv".into())
                ],
                path: target(&f.root, 2, "Two"),
            }]
        );

        // Names that differ only in letter case are one file on macOS and Windows.
        f.episodes.push(episode(1, 5, "ONE"));
        req.naming = NamingScheme::Custom {
            template: "{title}.{ext}".into(),
        };
        req.decisions[0].decision = approve(1, 1);
        req.decisions[1].decision = approve(1, 5);
        let plan = build_plan(f.context(), &req, disk(&nothing_exists)).unwrap();
        assert_eq!(plan.conflicts.len(), 1, "{:?}", plan.conflicts);
    }

    #[test]
    fn existing_targets_conflict_unless_they_move_away() {
        let f = Fixture::new();
        // title_t02 is approved as the episode whose name title_t01's target has; and the
        // target of title_t01 is title_t02's current name.
        let mut req = request(
            f.in_place(),
            vec![
                ("title_t01.mkv", approve(1, 1)),
                ("title_t02.mkv", approve(1, 2)),
            ],
        );
        req.naming = NamingScheme::Custom {
            template: "title_t0{episode}.{ext}".into(),
        };
        let on_disk: HashSet<PathBuf> =
            [f.root.join("title_t01.mkv"), f.root.join("title_t02.mkv")]
                .into_iter()
                .collect();
        let exists = |p: &Path| on_disk.contains(p);

        // t01 -> title_t01.mkv (itself), t02 -> title_t02.mkv (itself): no conflicts.
        let plan = build_plan(f.context(), &req, disk(&exists)).unwrap();
        assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);

        // Swap: t01 -> title_t02.mkv, t02 -> title_t01.mkv. Both targets exist but move away.
        req.decisions[0].decision = approve(1, 2);
        req.decisions[1].decision = approve(1, 1);
        let plan = build_plan(f.context(), &req, disk(&exists)).unwrap();
        assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);

        // t01 -> title_t03.mkv, which exists and is not part of the plan.
        req.decisions = vec![FileDecision {
            file_id: FileId("title_t01.mkv".into()),
            decision: approve(1, 3),
        }];
        let on_disk: HashSet<PathBuf> = [f.root.join("title_t03.mkv")].into_iter().collect();
        let exists = |p: &Path| on_disk.contains(p);
        let plan = build_plan(f.context(), &req, disk(&exists)).unwrap();
        assert_eq!(
            plan.conflicts,
            vec![PlanConflict::TargetExists {
                file_id: FileId("title_t01.mkv".into()),
                path: f.root.join("title_t03.mkv"),
            }]
        );
    }

    #[test]
    fn copies_conflict_with_any_existing_target() {
        let f = Fixture::new();
        let dest = PathBuf::from("/library");
        let req = request(
            SaveMode::CopyToFolder {
                destination: dest.clone(),
            },
            vec![("title_t01.mkv", approve(1, 1))],
        );
        let plan = build_plan(
            f.context(),
            &req,
            disk(&|p: &Path| p == target(&dest, 1, "One")),
        )
        .unwrap();
        assert_eq!(plan.items[0].to, target(&dest, 1, "One"));
        assert_eq!(plan.conflicts.len(), 1);
    }

    #[test]
    fn heard_subtitles_sit_next_to_the_target_and_must_not_exist() {
        let f = Fixture::new();
        let mut req = request(f.in_place(), vec![("title_t01.mkv", approve(1, 1))]);
        req.save_heard_subtitles = true;
        let srt = target(&f.root, 1, "One").with_extension("srt");
        let plan = build_plan(f.context(), &req, disk(&|p: &Path| p == srt)).unwrap();
        assert_eq!(plan.items[0].heard_subtitles_to.as_ref(), Some(&srt));
        assert_eq!(
            plan.conflicts,
            vec![PlanConflict::TargetExists {
                file_id: FileId("title_t01.mkv".into()),
                path: srt,
            }]
        );
    }

    #[test]
    fn export_lists_names_under_the_scanned_folder_without_conflicts() {
        let f = Fixture::new();
        let req = request(
            SaveMode::ExportList {
                destination: PathBuf::from("/out/list.csv"),
                replace: false,
            },
            vec![("sub/title_t05.mkv", approve(1, 1))],
        );
        let plan = build_plan(
            f.context(),
            &req,
            disk(&|p: &Path| p != Path::new("/out/list.csv")),
        )
        .unwrap();
        assert_eq!(plan.items[0].to, target(&f.root, 1, "One"));
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn an_existing_list_is_replaced_only_when_asked() {
        let f = Fixture::new();
        let mut req = request(
            SaveMode::ExportList {
                destination: PathBuf::from("/out/list.csv"),
                replace: false,
            },
            vec![("title_t01.mkv", approve(1, 1))],
        );
        let plan = build_plan(f.context(), &req, disk(&|_: &Path| true)).unwrap();
        assert_eq!(
            plan.conflicts,
            vec![PlanConflict::ListExists {
                path: PathBuf::from("/out/list.csv")
            }]
        );
        req.mode = SaveMode::ExportList {
            destination: PathBuf::from("/out/list.csv"),
            replace: true,
        };
        let plan = build_plan(f.context(), &req, disk(&|_: &Path| true)).unwrap();
        assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);
    }

    #[test]
    fn a_file_that_changed_since_the_scan_is_a_conflict() {
        let f = Fixture::new();
        let req = request(
            f.in_place(),
            vec![
                ("title_t01.mkv", approve(1, 1)),
                ("title_t02.mkv", approve(1, 2)),
            ],
        );
        // title_t01 now holds a different rip; title_t02 is gone.
        let sizes = |p: &Path| (p == f.root.join("title_t01.mkv")).then_some(1234);
        let view = DiskView {
            exists: &nothing_exists,
            file_size: &sizes,
        };
        let plan = build_plan(f.context(), &req, view).unwrap();
        assert_eq!(
            plan.conflicts,
            vec![
                PlanConflict::SourceChanged {
                    file_id: FileId("title_t01.mkv".into()),
                    path: f.root.join("title_t01.mkv"),
                },
                PlanConflict::SourceChanged {
                    file_id: FileId("title_t02.mkv".into()),
                    path: f.root.join("title_t02.mkv"),
                },
            ]
        );
    }

    #[test]
    fn ignored_files_are_never_renamed() {
        let mut f = Fixture::new();
        f.files[1].role = FileRole::Ignored;
        let req = request(f.in_place(), vec![("title_t01.mkv", approve(1, 1))]);
        let plan = build_plan(f.context(), &req, disk(&nothing_exists)).unwrap();
        assert!(plan.items.is_empty());
        assert_eq!(plan.untouched[1].reason, UntouchedReason::Skipped);
    }

    #[test]
    fn a_decomposed_name_is_its_own_composed_target() {
        // HFS+ stores "é" as "e" + U+0301; the catalog's title has the composed U+00E9.
        let mut f = Fixture::new();
        f.files[1].path = f.root.join("Cafe\u{301}.mkv");
        f.episodes[0].title = "Caf\u{e9}".into();
        let mut req = request(f.in_place(), vec![("title_t01.mkv", approve(1, 1))]);
        req.naming = NamingScheme::Custom {
            template: "{title}.{ext}".into(),
        };
        let current = f.files[1].path.clone();
        // The file system finds the file under either form.
        let exists = |p: &Path| fold(p) == fold(&current);
        let plan = build_plan(f.context(), &req, disk(&exists)).unwrap();
        assert_eq!(plan.items[0].to, f.root.join("Caf\u{e9}.mkv"));
        assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);
    }

    #[test]
    fn scan_root_strips_the_id_components() {
        assert_eq!(
            scan_root(
                Path::new("/a/b/VIDEO_TS/VTS_01_1.VOB"),
                &FileId("VIDEO_TS/VTS_01_1.VOB".into())
            ),
            PathBuf::from("/a/b")
        );
        assert_eq!(
            scan_root(Path::new("/a/b/t.mkv"), &FileId("t.mkv".into())),
            PathBuf::from("/a/b")
        );
    }
}
