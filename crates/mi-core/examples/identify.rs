//! Identifies a folder from the command line, without the app window: the same engine and
//! pipeline the app runs, with either the online episode list (TVmaze) or one read from JSON, and
//! optionally local subtitle files as reference text. Used for the end-to-end checks in
//! `docs/development.md`.
//!
//! ```text
//! cargo run --release -p mi-core --example identify -- <folder>
//!     (--tvmaze <show id> | --show <show.json> --episodes <episodes.json>)
//!     [--season N] [--references <folder>] [--model fast|accurate] [--models <folder>]
//!     [--whole] [--truth <truth.json>] [--json <results.json>]
//! ```
//!
//! `--models` defaults to `MI_TEST_MODEL_DIR`, then the app's own model folder is not used: pass
//! the folder holding the downloaded `ggml-*.bin` files. ffmpeg and ffprobe come from
//! `MI_FFMPEG`/`MI_FFPROBE`, else the built sidecars in `src-tauri/binaries/`, else `PATH`.
//! `--truth` maps file names to `S01E02`, `extra` or `playAll` and prints whether each answer is
//! right.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mi_core::{
    Catalog, Engine, EventSink, FfmpegMedia, LocalCatalog, OnlineCatalog, Services, WhisperEngine,
};
use mi_sources::local::LocalReferences;
use mi_sources::{ApiKeys, Cache, HttpClient, Sources};
use mi_types::{
    EpisodeOrdering, JobEvent, JobRequest, ModelStatus, Settings, Show, SpeechModel, Stage,
    StageState, Suggestion,
};

#[derive(Default)]
struct Printer {
    events: Mutex<Vec<JobEvent>>,
}

impl EventSink for Printer {
    fn job_event(&self, event: JobEvent) {
        match &event {
            JobEvent::Stage { stage, state, .. }
                if !matches!(state, StageState::Running { .. }) =>
            {
                eprintln!("  {stage:?}: {state:?}");
            }
            JobEvent::File {
                file_id,
                status,
                best_so_far,
                ..
            } => eprintln!(
                "  {}: {status:?}{}",
                file_id.0,
                best_so_far
                    .as_ref()
                    .map(|b| format!(" ({b})"))
                    .unwrap_or_default()
            ),
            JobEvent::Failed { message, .. } => eprintln!("  failed: {message}"),
            _ => {}
        }
        self.events.lock().unwrap().push(event);
    }

    fn model_status(&self, _status: ModelStatus) {}
}

struct Args {
    folder: PathBuf,
    tvmaze: Option<String>,
    show: Option<PathBuf>,
    episodes: Option<PathBuf>,
    season: Option<u32>,
    references: Option<PathBuf>,
    model: SpeechModel,
    models: PathBuf,
    whole: bool,
    truth: Option<PathBuf>,
    json: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut folder = None;
    let mut parsed = Args {
        folder: PathBuf::new(),
        tvmaze: None,
        show: None,
        episodes: None,
        season: None,
        references: None,
        model: SpeechModel::Fast,
        models: std::env::var_os("MI_TEST_MODEL_DIR")
            .map(PathBuf::from)
            .unwrap_or_default(),
        whole: false,
        truth: None,
        json: None,
    };
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--tvmaze" => parsed.tvmaze = Some(value()?),
            "--show" => parsed.show = Some(value()?.into()),
            "--episodes" => parsed.episodes = Some(value()?.into()),
            "--season" => parsed.season = Some(value()?.parse().map_err(|_| "bad season")?),
            "--references" => parsed.references = Some(value()?.into()),
            "--model" => {
                parsed.model = match value()?.as_str() {
                    "fast" => SpeechModel::Fast,
                    "accurate" => SpeechModel::Accurate,
                    other => return Err(format!("unknown model {other}")),
                }
            }
            "--models" => parsed.models = value()?.into(),
            "--whole" => parsed.whole = true,
            "--truth" => parsed.truth = Some(value()?.into()),
            "--json" => parsed.json = Some(value()?.into()),
            _ if folder.is_none() && !a.starts_with("--") => folder = Some(PathBuf::from(a)),
            _ => return Err(format!("unknown argument {a}")),
        }
    }
    parsed.folder = folder.ok_or("give the folder to identify")?;
    if parsed.models.as_os_str().is_empty() {
        return Err(
            "pass --models <folder with the ggml model files> or set MI_TEST_MODEL_DIR".into(),
        );
    }
    Ok(parsed)
}

fn sidecars() -> Result<FfmpegMedia, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/binaries");
    let triple = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else {
        "x86_64-pc-windows-msvc"
    };
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let built = (
        root.join(format!("ffmpeg-{triple}{ext}")),
        root.join(format!("ffprobe-{triple}{ext}")),
    );
    let usable = |p: &Path| std::fs::metadata(p).is_ok_and(|m| m.len() > 0);
    if std::env::var_os("MI_FFMPEG").is_none() && usable(&built.0) && usable(&built.1) {
        return Ok(FfmpegMedia::from_sidecars(mi_media::Sidecars {
            ffmpeg: built.0,
            ffprobe: built.1,
        }));
    }
    let sidecars = mi_media::Sidecars::resolve(&mi_media::SidecarLookup {
        exe_dir: None,
        allow_path_fallback: true,
    })
    .map_err(|e| e.to_string())?;
    Ok(FfmpegMedia::from_sidecars(sidecars))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,whisper_rs=error".into()),
        )
        .with_writer(std::io::stderr)
        .try_init();
    let args = parse_args()?;
    let data = std::env::temp_dir().join(format!("mi-identify-{}", std::process::id()));
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let references = match &args.references {
        Some(dir) => Some(LocalReferences::from_folder(dir).map_err(|e| e.to_string())?),
        None => None,
    };

    let (show, catalog): (Show, Arc<dyn Catalog>) = match (&args.tvmaze, &args.show, &args.episodes)
    {
        (Some(id), _, _) => {
            let sources = Sources::new(
                HttpClient::new().map_err(|e| e.to_string())?,
                Cache::open(&data.join("cache.sqlite")).map_err(|e| e.to_string())?,
                ApiKeys::default(),
            );
            let (show, _) = sources.tvmaze().show(id).await.map_err(|e| e.to_string())?;
            let mut catalog = OnlineCatalog::new(sources);
            if let Some(r) = references {
                catalog = catalog.with_reference_provider(Arc::new(r));
            }
            (show, Arc::new(catalog))
        }
        (None, Some(show), Some(episodes)) => {
            let show: Show = read_json(show)?;
            let episodes = read_json(episodes)?;
            let references = references.unwrap_or_else(|| LocalReferences::from_files(Vec::new()));
            (
                show.clone(),
                Arc::new(LocalCatalog::new(show, episodes, references)),
            )
        }
        _ => return Err("pass --tvmaze <id>, or --show and --episodes".into()),
    };

    let printer = Arc::new(Printer::default());
    let services = Services {
        media: Arc::new(sidecars()?),
        catalog,
        speech: Arc::new(WhisperEngine::new(&args.models)),
    };
    let settings = Settings {
        speech_model: args.model,
        sample_long_files: !args.whole,
        ..Settings::default()
    };
    let engine = Engine::with_services(
        &data,
        settings,
        tokio::runtime::Handle::current(),
        services,
        printer.clone(),
    );
    let started = Instant::now();
    eprintln!("Scanning {}", args.folder.display());
    let scan = engine.scan(&args.folder).await.map_err(|e| e.to_string())?;
    if let Some(p) = &scan.play_all {
        eprintln!("  play-all: {} ({})", p.file_id.0, p.reason);
    }
    let job = engine
        .start_job(JobRequest {
            folder: args.folder.clone(),
            show: show.clone(),
            ordering: EpisodeOrdering::Aired,
            seasons: args.season.map(|s| vec![s]),
            language: "en".into(),
        })
        .map_err(|e| e.to_string())?;
    loop {
        let ended = printer.events.lock().unwrap().iter().any(|e| {
            matches!(
                e,
                JobEvent::Finished { .. } | JobEvent::Cancelled { .. } | JobEvent::Failed { .. }
            )
        });
        if ended {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let elapsed = started.elapsed();
    let results = engine.job_results(&job).map_err(|e| e.to_string())?;
    let truth: BTreeMap<String, String> = match &args.truth {
        Some(path) => read_json(path)?,
        None => BTreeMap::new(),
    };

    println!(
        "{} with the {:?} model in {:.0} s",
        show.name,
        args.model,
        elapsed.as_secs_f64()
    );
    println!(
        "{:<22} {:<10} {:<40} {:>6} {:>7}  right?",
        "file", "verdict", "suggestion", "score", "margin"
    );
    let (mut right, mut judged) = (0, 0);
    let mut matches = results.matches.clone();
    matches.sort_by(|a, b| a.file_id.cmp(&b.file_id));
    for m in &matches {
        let (code, title) = match &m.suggestion {
            Suggestion::Episode { episode } => (
                format!("S{:02}E{:02}", episode.season, episode.number),
                results
                    .episodes
                    .iter()
                    .find(|e| e.key == *episode)
                    .map(|e| e.title.clone())
                    .unwrap_or_default(),
            ),
            Suggestion::NotAnEpisode => ("extra".to_owned(), String::new()),
            Suggestion::PlayAll => ("playAll".to_owned(), String::new()),
        };
        let verdict = format!("{:?}", m.confidence.verdict);
        let mark = match truth.get(&m.file_id.0) {
            Some(expected) => {
                judged += 1;
                if *expected == code {
                    right += 1;
                    "yes".to_owned()
                } else {
                    format!("NO (is {expected})")
                }
            }
            None => String::new(),
        };
        println!(
            "{:<22} {:<10} {:<40} {:>6.2} {:>7.2}  {mark}",
            m.file_id.0,
            verdict,
            format!("{code} {title}")
                .chars()
                .take(40)
                .collect::<String>(),
            m.confidence.score,
            m.confidence.margin,
        );
    }
    if judged > 0 {
        println!("{right} of {judged} right");
    }
    if let Some(path) = &args.json {
        let bytes = serde_json::to_vec_pretty(&results).map_err(|e| e.to_string())?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())?;
    }
    let disc = printer.events.lock().unwrap().iter().find_map(|e| match e {
        JobEvent::Stage {
            stage: Stage::DiscOrder,
            state,
            ..
        } if !matches!(state, StageState::Running { .. }) => Some(format!("{state:?}")),
        _ => None,
    });
    if let Some(d) = disc {
        println!("disc order stage: {d}");
    }
    let _ = std::fs::remove_dir_all(&data);
    Ok(())
}
