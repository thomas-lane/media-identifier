//! The JSON the UI receives has the shape the generated TypeScript types describe.

use mi_types::*;
use serde_json::json;

#[test]
fn tagged_enum_fields_are_camel_case() {
    let event = JobEvent::File {
        job_id: JobId("j1".into()),
        file_id: FileId("title_t03.mkv".into()),
        status: FileStatus::Done,
        best_so_far: Some("Conjunction Junction".into()),
        verdict: Some(Verdict::Confident),
    };
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({
            "kind": "file",
            "jobId": "j1",
            "fileId": "title_t03.mkv",
            "status": "done",
            "bestSoFar": "Conjunction Junction",
            "verdict": "confident"
        })
    );

    let state = ModelState::Downloading {
        downloaded: 1,
        total: 2,
        bytes_per_second: 3.0,
    };
    assert_eq!(
        serde_json::to_value(&state).unwrap(),
        json!({"kind": "downloading", "downloaded": 1, "total": 2, "bytesPerSecond": 3.0})
    );
}

#[test]
fn struct_fields_are_camel_case_and_newtypes_are_plain_strings() {
    let key = EpisodeKey {
        season: 2,
        number: 3,
    };
    let decision = FileDecision {
        file_id: FileId("a.mkv".into()),
        decision: ReviewDecision::Approved { episode: key },
    };
    assert_eq!(
        serde_json::to_value(&decision).unwrap(),
        json!({"fileId": "a.mkv", "decision": {"kind": "approved", "episode": {"season": 2, "number": 3}}})
    );
}

#[test]
fn every_tagged_value_round_trips() {
    let plan = RenamePlan {
        job_id: JobId("j".into()),
        mode: SaveMode::RenameInPlace {
            root: "/rips/disc1".into(),
        },
        items: vec![],
        untouched: vec![UntouchedFile {
            file_id: FileId("title_t00.mkv".into()),
            path: "/rips/disc1/title_t00.mkv".into(),
            reason: UntouchedReason::PlayAll,
        }],
        conflicts: vec![PlanConflict::DuplicateTarget {
            file_ids: vec![FileId("a".into()), FileId("b".into())],
            path: "/x".into(),
        }],
    };
    let text = serde_json::to_string(&plan).unwrap();
    assert_eq!(serde_json::from_str::<RenamePlan>(&text).unwrap(), plan);
    assert!(text.contains("\"fileIds\""));
}
