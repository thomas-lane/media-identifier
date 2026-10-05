//! End to end on a synthetic ripped disc: real ffmpeg, the real Fast speech model and the whole
//! pipeline, on speech made with macOS's `say` (see `scripts/make-synthetic-disc.sh`).
//!
//! Ignored by default because it needs macOS, a full ffmpeg to make the files, and the Fast and
//! VAD model files (downloaded by the `mi-transcribe` real-model tests into `MI_TEST_MODEL_DIR`):
//!
//! ```text
//! MI_TEST_MODEL_DIR=<folder with ggml-small.en-q5_1.bin and ggml-silero-v5.1.2.bin> \
//!   cargo test -p mi-core --test synthetic_disc -- --ignored --nocapture
//! ```
//!
//! `MI_SYNTHETIC_DIR` reuses a disc the script already made. The result says how the pipeline
//! behaves on clean synthetic speech; it is not a measure of accuracy on real recordings.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mi_core::{Engine, EventSink, FfmpegMedia, LocalCatalog, Services, WhisperEngine};
use mi_sources::local::LocalReferences;
use mi_types::{
    EpisodeOrdering, JobEvent, JobRequest, ModelStatus, Settings, Show, SpeechModel, Suggestion,
    Verdict,
};

#[derive(Default)]
struct Events(Mutex<Vec<JobEvent>>);

impl EventSink for Events {
    fn job_event(&self, event: JobEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn model_status(&self, _status: ModelStatus) {}
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn media() -> FfmpegMedia {
    let bin = repo_root().join("src-tauri/binaries");
    let (ffmpeg, ffprobe) = (
        bin.join("ffmpeg-aarch64-apple-darwin"),
        bin.join("ffprobe-aarch64-apple-darwin"),
    );
    let usable = |p: &Path| std::fs::metadata(p).is_ok_and(|m| m.len() > 0);
    if std::env::var_os("MI_FFMPEG").is_none() && usable(&ffmpeg) && usable(&ffprobe) {
        println!("using the built sidecars");
        return FfmpegMedia::from_sidecars(mi_media::Sidecars { ffmpeg, ffprobe });
    }
    println!("using ffmpeg from MI_FFMPEG/MI_FFPROBE or PATH");
    FfmpegMedia::from_sidecars(
        mi_media::Sidecars::resolve(&mi_media::SidecarLookup {
            exe_dir: None,
            allow_path_fallback: true,
        })
        .expect("ffmpeg and ffprobe"),
    )
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs macOS say, a full ffmpeg and the Fast model; see the module docs"]
async fn a_synthetic_disc_is_identified_end_to_end() {
    let scratch = tempfile::tempdir().unwrap();
    let disc_root = match std::env::var_os("MI_SYNTHETIC_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => {
            let out = scratch.path().join("synthetic");
            let status =
                std::process::Command::new(repo_root().join("scripts/make-synthetic-disc.sh"))
                    .arg(&out)
                    .status()
                    .expect("run make-synthetic-disc.sh");
            assert!(status.success(), "make-synthetic-disc.sh failed");
            out
        }
    };
    let models = std::env::var_os("MI_TEST_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target/tmp/models"));

    let show: Show = read_json(&disc_root.join("show.json"));
    let episodes = read_json(&disc_root.join("episodes.json"));
    let truth: BTreeMap<String, String> = read_json(&disc_root.join("truth.json"));
    let references = LocalReferences::from_folder(&disc_root.join("refs")).unwrap();
    let events = Arc::new(Events::default());
    let engine = Engine::with_services(
        scratch.path(),
        Settings {
            speech_model: SpeechModel::Fast,
            ..Settings::default()
        },
        tokio::runtime::Handle::current(),
        Services {
            media: Arc::new(media()),
            catalog: Arc::new(LocalCatalog::new(show.clone(), episodes, references)),
            speech: Arc::new(WhisperEngine::new(&models)),
        },
        events.clone(),
    );

    let folder = disc_root.join("disc");
    let scan = engine.scan(&folder).await.unwrap();
    assert_eq!(
        scan.play_all.as_ref().map(|p| p.file_id.0.as_str()),
        Some("title_t00.mkv"),
        "the play-all is detected"
    );
    let started = Instant::now();
    let job = engine
        .start_job(JobRequest {
            folder,
            show,
            ordering: EpisodeOrdering::Aired,
            seasons: None,
            language: "en".into(),
        })
        .unwrap();
    loop {
        let end = events.0.lock().unwrap().iter().find_map(|e| match e {
            JobEvent::Finished { .. } => Some(Ok(())),
            JobEvent::Failed { message, .. } => Some(Err(message.clone())),
            JobEvent::Cancelled { .. } => Some(Err("cancelled".into())),
            _ => None,
        });
        if let Some(end) = end {
            end.expect("the job finishes");
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(900),
            "the job takes too long"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let results = engine.job_results(&job).unwrap();
    let mut right = 0;
    println!(
        "{:<16} {:<10} {:<8} {:>6} {:>7}",
        "file", "verdict", "answer", "score", "margin"
    );
    for m in &results.matches {
        let answer = match &m.suggestion {
            Suggestion::Episode { episode } => {
                format!("S{:02}E{:02}", episode.season, episode.number)
            }
            Suggestion::NotAnEpisode => "extra".into(),
            Suggestion::PlayAll => "playAll".into(),
        };
        let expected = &truth[&m.file_id.0];
        let ok = *expected == answer;
        right += usize::from(ok);
        println!(
            "{:<16} {:<10} {:<8} {:>6.2} {:>7.2}  {}",
            m.file_id.0,
            format!("{:?}", m.confidence.verdict),
            answer,
            m.confidence.score,
            m.confidence.margin,
            if ok {
                "right".to_owned()
            } else {
                format!("WRONG, is {expected}")
            }
        );
    }
    println!(
        "{right} of {} right in {:.0} s",
        truth.len(),
        started.elapsed().as_secs_f64()
    );
    assert_eq!(results.matches.len(), truth.len());
    assert_eq!(right, truth.len(), "every file is identified correctly");
    let confident = results
        .matches
        .iter()
        .filter(|m| m.confidence.verdict == Verdict::Confident)
        .count();
    assert!(confident >= 7, "most episodes are confident ({confident})");
}
