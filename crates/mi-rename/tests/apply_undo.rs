//! Rename, copy, CSV export and undo against real temporary folders.

use std::fs;
use std::path::{Path, PathBuf};

use mi_rename::{Journal, PlanContext, apply_plan, build_plan};
use mi_types::{
    Confidence, Episode, EpisodeKey, EpisodeOrdering, FileDecision, FileId, FileMatch, FileRole,
    JobId, MediaFile, NamingScheme, PlanConflict, ProviderId, RenamePlan, RenamePlanRequest,
    ReviewDecision, SaveMode, Show, ShowRef, Suggestion, Verdict,
};
use tempfile::TempDir;

struct Disc {
    _dir: TempDir,
    root: PathBuf,
    data: PathBuf,
    show: Show,
    episodes: Vec<Episode>,
    files: Vec<MediaFile>,
    matches: Vec<FileMatch>,
}

fn show_ref() -> ShowRef {
    ShowRef {
        provider: ProviderId::Tvmaze,
        id: "1".into(),
    }
}

impl Disc {
    /// A disc folder with a play-all, three episodes' worth of files and one extra.
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("SCHOOLHOUSE_ROCK");
        let data = dir.path().join("appdata");
        fs::create_dir_all(&root).unwrap();
        let mut disc = Self {
            _dir: dir,
            root,
            data,
            show: Show {
                show_ref: show_ref(),
                name: "Schoolhouse Rock!".into(),
                year: Some(1973),
                kind: None,
                season_count: None,
                episode_count: None,
                url: None,
            },
            episodes: [
                (1, "Verb: That's What's Happening"),
                (2, "Unpack Your Adjectives"),
                (3, "Lolly, Lolly, Lolly"),
            ]
            .iter()
            .map(|(n, t)| Episode {
                show_ref: show_ref(),
                ordering: EpisodeOrdering::Aired,
                key: EpisodeKey {
                    season: 3,
                    number: *n,
                },
                title: (*t).into(),
                runtime_s: None,
                airdate: None,
                summary: None,
                provider_episode_id: n.to_string(),
            })
            .collect(),
            files: Vec::new(),
            matches: Vec::new(),
        };
        disc.add(
            "title_t00.mkv",
            "PLAY ALL",
            FileRole::PlayAll,
            Verdict::PlayAll,
        );
        disc.add(
            "title_t01.mkv",
            "one",
            FileRole::Candidate,
            Verdict::Confident,
        );
        disc.add(
            "title_t02.mkv",
            "two!",
            FileRole::Candidate,
            Verdict::Confident,
        );
        disc.add(
            "title_t03.mkv",
            "three",
            FileRole::Candidate,
            Verdict::Check,
        );
        disc.add(
            "title_t04.mkv",
            "bonus",
            FileRole::Candidate,
            Verdict::Extra,
        );
        disc
    }

    fn add(&mut self, id: &str, content: &str, role: FileRole, verdict: Verdict) {
        let path = self.root.join(id);
        fs::write(&path, content).unwrap();
        self.files.push(MediaFile {
            id: FileId(id.into()),
            path,
            file_name: id.into(),
            size_bytes: content.len() as u64,
            probe: None,
            role,
        });
        self.matches.push(FileMatch {
            file_id: FileId(id.into()),
            suggestion: Suggestion::NotAnEpisode,
            confidence: Confidence {
                score: 0.5,
                margin: 0.1,
                verdict,
            },
            candidates: vec![],
        });
    }

    fn journal(&self) -> Journal {
        Journal::new(self.data.join("history.jsonl"))
    }

    fn plan(&self, mode: SaveMode, approvals: &[(&str, u32)], naming: NamingScheme) -> RenamePlan {
        let request = RenamePlanRequest {
            job_id: JobId("job".into()),
            decisions: approvals
                .iter()
                .map(|(id, n)| FileDecision {
                    file_id: FileId((*id).into()),
                    decision: ReviewDecision::Approved {
                        episode: EpisodeKey {
                            season: 3,
                            number: *n,
                        },
                    },
                })
                .collect(),
            mode,
            naming,
            save_heard_subtitles: false,
        };
        let context = PlanContext {
            show: &self.show,
            episodes: &self.episodes,
            files: &self.files,
            matches: &self.matches,
        };
        build_plan(context, &request, &|p: &Path| p.exists()).unwrap()
    }

    fn in_place(&self) -> SaveMode {
        SaveMode::RenameInPlace {
            root: self.root.clone(),
        }
    }

    fn standard_plan(&self) -> RenamePlan {
        self.plan(
            self.in_place(),
            &[
                ("title_t01.mkv", 1),
                ("title_t02.mkv", 2),
                ("title_t03.mkv", 3),
            ],
            NamingScheme::JellyfinPlex,
        )
    }

    fn season(&self) -> PathBuf {
        self.root.join("Schoolhouse Rock! (1973)").join("Season 03")
    }

    fn target(&self, n: u32, title: &str) -> PathBuf {
        self.season().join(format!(
            "Schoolhouse Rock! (1973) - S03E{n:02} - {title}.mkv"
        ))
    }

    /// Every file under the disc folder, relative, sorted.
    fn listing(&self) -> Vec<String> {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, base, out);
                } else {
                    out.push(
                        path.strip_prefix(base)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &self.root, &mut out);
        out.sort();
        out
    }
}

fn no_subtitles(_: &FileId) -> Option<String> {
    None
}

fn original_listing() -> Vec<String> {
    [
        "title_t00.mkv",
        "title_t01.mkv",
        "title_t02.mkv",
        "title_t03.mkv",
        "title_t04.mkv",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[test]
fn rename_in_place_moves_episodes_and_leaves_play_all_and_extras() {
    let disc = Disc::new();
    let plan = disc.standard_plan();
    assert!(plan.conflicts.is_empty());
    let outcome = apply_plan(&plan, "Schoolhouse Rock!", &disc.journal(), &no_subtitles).unwrap();
    assert_eq!(outcome.completed, 3);
    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert_eq!(
        disc.listing(),
        vec![
            "Schoolhouse Rock! (1973)/Season 03/Schoolhouse Rock! (1973) - S03E01 - Verb - That's What's Happening.mkv",
            "Schoolhouse Rock! (1973)/Season 03/Schoolhouse Rock! (1973) - S03E02 - Unpack Your Adjectives.mkv",
            "Schoolhouse Rock! (1973)/Season 03/Schoolhouse Rock! (1973) - S03E03 - Lolly, Lolly, Lolly.mkv",
            "title_t00.mkv",
            "title_t04.mkv",
        ]
    );
    assert_eq!(
        fs::read_to_string(disc.target(2, "Unpack Your Adjectives")).unwrap(),
        "two!"
    );

    let history = disc.journal().list().unwrap();
    assert_eq!(history.len(), 1);
    let entry = &history[0];
    assert_eq!(Some(&entry.id), outcome.history_id.as_ref());
    assert_eq!(entry.show_name, "Schoolhouse Rock!");
    assert_eq!(entry.folder, disc.root);
    assert_eq!(entry.items.len(), 3);
    assert!(
        entry
            .items
            .iter()
            .any(|i| i.from == disc.root.join("title_t02.mkv")
                && i.to == disc.target(2, "Unpack Your Adjectives"))
    );
    assert_eq!(entry.undone_at_ms, None);
}

#[test]
fn undo_restores_names_and_removes_only_folders_it_created() {
    let disc = Disc::new();
    // The show folder exists already (another disc); the season folder does not.
    let show_dir = disc.root.join("Schoolhouse Rock! (1973)");
    fs::create_dir(&show_dir).unwrap();
    let journal = disc.journal();
    let outcome = apply_plan(&disc.standard_plan(), "S", &journal, &no_subtitles).unwrap();
    // Finder browsed the new folder.
    fs::write(disc.season().join(".DS_Store"), "x").unwrap();

    let id = outcome.history_id.unwrap();
    let undo = journal.undo(&id).unwrap();
    assert_eq!(undo.restored, 3);
    assert!(undo.failed.is_empty(), "{:?}", undo.failed);
    assert_eq!(disc.listing(), original_listing());
    assert_eq!(
        fs::read_to_string(disc.root.join("title_t02.mkv")).unwrap(),
        "two!"
    );
    assert!(show_dir.is_dir(), "a folder that existed before is kept");
    assert!(!disc.season().exists());

    let history = journal.list().unwrap();
    assert!(history[0].undone_at_ms.is_some());
    assert_eq!(
        history[0].items.len(),
        3,
        "an undone entry still shows what the save did"
    );

    // A second undo does nothing.
    let again = journal.undo(&id).unwrap();
    assert_eq!(again.restored, 0);
    assert!(again.failed.is_empty());
}

#[test]
fn files_can_swap_names_and_change_case() {
    let disc = Disc::new();
    let journal = disc.journal();
    // title_t01 <-> title_t02, and title_t03 -> TITLE_T03 (case only).
    let template = NamingScheme::Custom {
        template: "{title}.{ext}".into(),
    };
    let mut episodes = disc.episodes.clone();
    episodes[0].title = "title_t02".into();
    episodes[1].title = "title_t01".into();
    episodes[2].title = "TITLE_T03".into();
    let disc = Disc { episodes, ..disc };
    let plan = disc.plan(
        disc.in_place(),
        &[
            ("title_t01.mkv", 1),
            ("title_t02.mkv", 2),
            ("title_t03.mkv", 3),
        ],
        template,
    );
    assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);
    let outcome = apply_plan(&plan, "S", &journal, &no_subtitles).unwrap();
    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert_eq!(
        fs::read_to_string(disc.root.join("title_t01.mkv")).unwrap(),
        "two!"
    );
    assert_eq!(
        fs::read_to_string(disc.root.join("title_t02.mkv")).unwrap(),
        "one"
    );
    let names: Vec<String> = fs::read_dir(&disc.root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"TITLE_T03.mkv".to_owned()), "{names:?}");
    assert!(!names.contains(&"title_t03.mkv".to_owned()), "{names:?}");

    journal.undo(&outcome.history_id.unwrap()).unwrap();
    assert_eq!(
        fs::read_to_string(disc.root.join("title_t01.mkv")).unwrap(),
        "one"
    );
    assert_eq!(disc.listing(), original_listing());
}

#[test]
fn a_target_created_after_planning_is_never_overwritten() {
    let disc = Disc::new();
    let plan = disc.standard_plan();
    // Another program creates one target between preview and apply.
    let taken = disc.target(1, "Verb - That's What's Happening");
    fs::create_dir_all(taken.parent().unwrap()).unwrap();
    fs::write(&taken, "someone else's file").unwrap();

    let outcome = apply_plan(&plan, "S", &disc.journal(), &no_subtitles).unwrap();
    assert_eq!(outcome.completed, 2);
    assert_eq!(outcome.failed.len(), 1);
    assert_eq!(outcome.failed[0].file_id, FileId("title_t01.mkv".into()));
    assert_eq!(fs::read_to_string(&taken).unwrap(), "someone else's file");
    assert_eq!(
        fs::read_to_string(disc.root.join("title_t01.mkv")).unwrap(),
        "one"
    );
    let entry = &disc.journal().list().unwrap()[0];
    assert_eq!(entry.items.len(), 2, "History lists only what moved");
}

#[test]
fn a_plan_with_conflicts_is_refused_without_changes() {
    let disc = Disc::new();
    let plan = disc.plan(
        disc.in_place(),
        &[("title_t01.mkv", 1), ("title_t02.mkv", 1)],
        NamingScheme::JellyfinPlex,
    );
    assert!(matches!(
        plan.conflicts[0],
        PlanConflict::DuplicateTarget { .. }
    ));
    let err = apply_plan(&plan, "S", &disc.journal(), &no_subtitles).unwrap_err();
    assert!(matches!(err, mi_rename::RenameError::Conflicts(1)));
    assert_eq!(disc.listing(), original_listing());
    assert!(disc.journal().list().unwrap().is_empty());
}

#[test]
fn a_missing_source_fails_alone() {
    let disc = Disc::new();
    let plan = disc.standard_plan();
    fs::remove_file(disc.root.join("title_t03.mkv")).unwrap();
    let outcome = apply_plan(&plan, "S", &disc.journal(), &no_subtitles).unwrap();
    assert_eq!(outcome.completed, 2);
    assert_eq!(outcome.failed[0].file_id, FileId("title_t03.mkv".into()));
}

#[test]
fn copies_leave_originals_and_undo_deletes_unchanged_copies() {
    let disc = Disc::new();
    let library = disc.root.parent().unwrap().join("Library");
    let plan = disc.plan(
        SaveMode::CopyToFolder {
            destination: library.clone(),
        },
        &[("title_t01.mkv", 1), ("title_t02.mkv", 2)],
        NamingScheme::Kodi,
    );
    let journal = disc.journal();
    let outcome = apply_plan(&plan, "S", &journal, &no_subtitles).unwrap();
    assert_eq!(outcome.completed, 2);
    assert_eq!(disc.listing(), original_listing());
    let season = library.join("Schoolhouse Rock! (1973)").join("Season 03");
    let mut copies: Vec<String> = fs::read_dir(&season)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    copies.sort();
    assert_eq!(
        copies,
        vec![
            "Schoolhouse Rock! S03E01 - Verb - That's What's Happening.mkv",
            "Schoolhouse Rock! S03E02 - Unpack Your Adjectives.mkv",
        ],
        "no temporary files remain"
    );
    let copy2 = season.join("Schoolhouse Rock! S03E02 - Unpack Your Adjectives.mkv");
    assert_eq!(fs::read_to_string(&copy2).unwrap(), "two!");
    assert_eq!(journal.list().unwrap()[0].items.len(), 2);

    // The user edits one copy; undo deletes the other and reports this one.
    fs::write(&copy2, "edited, longer").unwrap();
    let id = outcome.history_id.unwrap();
    let undo = journal.undo(&id).unwrap();
    assert_eq!(undo.restored, 1);
    assert_eq!(undo.failed.len(), 1);
    assert_eq!(undo.failed[0].path, copy2);
    assert!(copy2.exists());
    assert_eq!(
        journal.list().unwrap()[0].undone_at_ms,
        None,
        "not fully undone"
    );

    // After the user deletes it, undo finishes and removes the folders it created.
    fs::remove_file(&copy2).unwrap();
    let undo = journal.undo(&id).unwrap();
    assert!(undo.failed.is_empty(), "{:?}", undo.failed);
    assert!(!library.exists());
    assert_eq!(disc.listing(), original_listing());
}

#[test]
fn a_copy_never_replaces_an_existing_file() {
    let disc = Disc::new();
    let library = disc.root.parent().unwrap().join("Library");
    let plan = disc.plan(
        SaveMode::CopyToFolder {
            destination: library.clone(),
        },
        &[("title_t01.mkv", 1)],
        NamingScheme::JellyfinPlex,
    );
    let target = plan.items[0].to.clone();
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "keep me").unwrap();
    let outcome = apply_plan(&plan, "S", &disc.journal(), &no_subtitles).unwrap();
    assert_eq!(outcome.completed, 0);
    assert_eq!(outcome.failed.len(), 1);
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep me");
    let leftovers: Vec<_> = fs::read_dir(target.parent().unwrap())
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(leftovers.len(), 1, "the temporary copy was removed");
}

#[test]
fn undo_leaves_changed_files_and_never_overwrites() {
    let disc = Disc::new();
    let journal = disc.journal();
    let outcome = apply_plan(&disc.standard_plan(), "S", &journal, &no_subtitles).unwrap();
    let id = outcome.history_id.unwrap();

    // One renamed file was changed; another's original name is now taken.
    let changed = disc.target(1, "Verb - That's What's Happening");
    fs::write(&changed, "re-encoded, different size").unwrap();
    fs::write(disc.root.join("title_t02.mkv"), "a new rip").unwrap();

    let undo = journal.undo(&id).unwrap();
    assert_eq!(undo.restored, 1);
    let mut failed: Vec<_> = undo.failed.iter().map(|f| f.file_id.0.clone()).collect();
    failed.sort();
    assert_eq!(failed, vec!["title_t01.mkv", "title_t02.mkv"]);
    assert_eq!(
        fs::read_to_string(&changed).unwrap(),
        "re-encoded, different size"
    );
    assert_eq!(
        fs::read_to_string(disc.root.join("title_t02.mkv")).unwrap(),
        "a new rip"
    );
    assert_eq!(
        fs::read_to_string(disc.target(2, "Unpack Your Adjectives")).unwrap(),
        "two!",
        "the file whose name was taken stays renamed"
    );
    assert_eq!(
        fs::read_to_string(disc.root.join("title_t03.mkv")).unwrap(),
        "three"
    );
    let entry = &journal.list().unwrap()[0];
    assert_eq!(entry.undone_at_ms, None);
    assert_eq!(
        entry.items.len(),
        2,
        "History shows the two files still renamed"
    );
}

#[test]
fn heard_subtitles_are_written_once_and_removed_by_undo() {
    let disc = Disc::new();
    let request_plan = {
        let mut plan = disc.plan(
            disc.in_place(),
            &[("title_t01.mkv", 1)],
            NamingScheme::JellyfinPlex,
        );
        let srt = plan.items[0].to.with_extension("srt");
        plan.items[0].heard_subtitles_to = Some(srt);
        plan
    };
    let journal = disc.journal();
    let outcome = apply_plan(&request_plan, "S", &journal, &|id: &FileId| {
        (id.0 == "title_t01.mkv").then(|| "1\n00:00:01,000 --> 00:00:02,000\nVerb!\n\n".to_owned())
    })
    .unwrap();
    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    let srt = request_plan.items[0].heard_subtitles_to.clone().unwrap();
    assert!(fs::read_to_string(&srt).unwrap().contains("Verb!"));

    journal.undo(&outcome.history_id.unwrap()).unwrap();
    assert!(!srt.exists());
    assert_eq!(disc.listing(), original_listing());
}

#[test]
fn an_interrupted_save_is_listed_as_it_is_on_disk_and_can_be_undone() {
    let disc = Disc::new();
    let journal = disc.journal();
    fs::create_dir_all(&disc.data).unwrap();
    let a = disc.root.join("title_t01.mkv");
    let b = disc.root.join("title_t02.mkv");
    let a_temp = disc.root.join(".mi-e1-a1.tmp");
    let b_temp = disc.root.join(".mi-e1-a2.tmp");
    let season = disc.season();
    let b_target = disc.target(2, "Unpack Your Adjectives");

    // What a crash leaves: both files reached their temporary names; the season folder and B's
    // final move were intended but neither marked done (B's move happened, the folder too).
    fs::rename(&a, &a_temp).unwrap();
    fs::create_dir_all(&season).unwrap();
    fs::rename(&b, &b_target).unwrap();
    let show_dir = season.parent().unwrap();
    let j = |v: serde_json::Value| v.to_string();
    let p = |p: &Path| p.to_string_lossy().into_owned();
    let lines = [
        j(
            serde_json::json!({"kind":"begin","entry":"e1","createdAtMs":1,"showName":"S","folder":p(&disc.root),"mode":"renameInPlace"}),
        ),
        j(
            serde_json::json!({"kind":"intent","entry":"e1","op":0,"action":"move","phase":"apply","item":1,"fileId":"title_t01.mkv","from":p(&a),"to":p(&a_temp),"sizeBytes":3}),
        ),
        j(serde_json::json!({"kind":"done","entry":"e1","op":0})),
        j(
            serde_json::json!({"kind":"intent","entry":"e1","op":1,"action":"move","phase":"apply","item":2,"fileId":"title_t02.mkv","from":p(&b),"to":p(&b_temp),"sizeBytes":4}),
        ),
        j(serde_json::json!({"kind":"done","entry":"e1","op":1})),
        j(
            serde_json::json!({"kind":"intent","entry":"e1","op":2,"action":"mkdir","phase":"apply","to":p(show_dir)}),
        ),
        j(serde_json::json!({"kind":"done","entry":"e1","op":2})),
        j(
            serde_json::json!({"kind":"intent","entry":"e1","op":3,"action":"mkdir","phase":"apply","to":p(&season)}),
        ),
        j(
            serde_json::json!({"kind":"intent","entry":"e1","op":4,"action":"move","phase":"apply","item":2,"fileId":"title_t02.mkv","from":p(&b_temp),"to":p(&b_target),"sizeBytes":4}),
        ),
    ];
    // The last line was cut short by the power failure.
    let content = format!("{}\n{{\"kind\":\"do", lines.join("\n"));
    fs::write(journal.path(), content).unwrap();

    let history = journal.list().unwrap();
    assert_eq!(history.len(), 1);
    let mut items: Vec<_> = history[0]
        .items
        .iter()
        .map(|i| (i.from.clone(), i.to.clone()))
        .collect();
    items.sort();
    assert_eq!(
        items,
        vec![(a.clone(), a_temp.clone()), (b.clone(), b_target.clone())]
    );

    let undo = journal.undo(&history[0].id).unwrap();
    assert!(undo.failed.is_empty(), "{:?}", undo.failed);
    assert_eq!(undo.restored, 2);
    assert_eq!(disc.listing(), original_listing());
    assert!(!show_dir.exists());
}

#[test]
fn history_lists_newest_first() {
    let disc = Disc::new();
    let journal = disc.journal();
    let first = apply_plan(
        &disc.plan(
            disc.in_place(),
            &[("title_t01.mkv", 1)],
            NamingScheme::JellyfinPlex,
        ),
        "First",
        &journal,
        &no_subtitles,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let second = apply_plan(
        &disc.plan(
            disc.in_place(),
            &[("title_t02.mkv", 2)],
            NamingScheme::JellyfinPlex,
        ),
        "Second",
        &journal,
        &no_subtitles,
    )
    .unwrap();
    let ids: Vec<_> = journal.list().unwrap().into_iter().map(|e| e.id).collect();
    assert_eq!(
        ids,
        vec![second.history_id.unwrap(), first.history_id.unwrap()]
    );
}

#[test]
fn csv_export_lists_every_file_and_changes_nothing() {
    let disc = Disc::new();
    let csv_path = disc.root.parent().unwrap().join("list.csv");
    let mut episodes = disc.episodes.clone();
    episodes[0].title = "=HYPERLINK(\"x\")".into();
    let disc = Disc { episodes, ..disc };
    let plan = disc.plan(
        SaveMode::ExportList {
            destination: csv_path.clone(),
        },
        &[("title_t01.mkv", 1), ("title_t02.mkv", 2)],
        NamingScheme::JellyfinPlex,
    );
    let outcome = apply_plan(&plan, "S", &disc.journal(), &no_subtitles).unwrap();
    assert_eq!(outcome.completed, 2);
    assert_eq!(outcome.history_id, None);
    assert_eq!(disc.listing(), original_listing());
    let text = fs::read_to_string(&csv_path).unwrap();
    let expected = "\u{feff}file,status,season,episode,title,new_name\n\
        title_t00.mkv,play-all,,,,\n\
        title_t01.mkv,episode,3,1,\"'=HYPERLINK(\"\"x\"\")\",Schoolhouse Rock! (1973)/Season 03/Schoolhouse Rock! (1973) - S03E01 - =HYPERLINK('x').mkv\n\
        title_t02.mkv,episode,3,2,Unpack Your Adjectives,Schoolhouse Rock! (1973)/Season 03/Schoolhouse Rock! (1973) - S03E02 - Unpack Your Adjectives.mkv\n\
        title_t03.mkv,skipped,,,,\n\
        title_t04.mkv,extra,,,,\n";
    assert_eq!(text, expected);
}
