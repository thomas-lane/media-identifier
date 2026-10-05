//! The engine the Tauri commands call.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use mi_media::SidecarLookup;
use mi_rename::{Journal, PlanContext};
use mi_sources::ApiKeys;
use mi_transcribe::ModelStore;
use mi_types::{
    CancelFlag, HistoryEntry, HistoryId, JobEvent, JobId, JobRequest, JobResults, ModelState,
    ModelStatus, RecentJob, RenameOutcome, RenamePlan, RenamePlanRequest, SaveMode, ScanSummary,
    Settings, ShowCandidate, SourceStatus, SpeechModel, UndoOutcome, Verdict,
};

use crate::jobs::JobStore;
use crate::pipeline::{self, JobContext, Outcome, PipelineConfig};
use crate::services::{FfmpegMedia, OnlineCatalog, Services, WhisperEngine};
use crate::{CoreError, DataPaths};

/// Receives events from the engine. The Tauri app forwards them to the window; tests collect
/// them. Implementations must be cheap and must not block (events are sent from worker threads).
pub trait EventSink: Send + Sync + 'static {
    /// A job progress event (`job-event` channel).
    fn job_event(&self, event: JobEvent);
    /// A model download progress update (`model-download` channel).
    fn model_status(&self, status: ModelStatus);
}

/// Engine construction parameters.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// The app's data folder.
    pub data_dir: PathBuf,
    /// Where to find ffmpeg and ffprobe.
    pub sidecars: SidecarLookup,
    /// Current settings.
    pub settings: Settings,
    /// Current API keys.
    pub keys: ApiKeys,
    /// The Tokio runtime jobs and downloads run on (Tauri's own runtime in the app).
    pub runtime: tokio::runtime::Handle,
}

/// The running job.
struct Running {
    job_id: JobId,
    cancel: CancelFlag,
}

/// The model download in progress.
struct Download {
    model: SpeechModel,
    cancel: CancelFlag,
    last: Option<ModelStatus>,
}

/// Long-lived services and at most one running job.
///
/// Concurrency contract: methods take `&self` and may be called from any thread. Only one
/// identification job runs at a time ([`Engine::start_job`] returns `Busy` otherwise); a job's
/// blocking work (ffmpeg, whisper.cpp, matching) runs on Tokio's blocking pool and its network
/// work on the runtime. Cancelling sets the job's `CancelFlag`; the job then emits `Cancelled` as
/// its last event. Only one model download runs at a time.
pub struct Engine {
    paths: DataPaths,
    settings: RwLock<Settings>,
    services: Services,
    sink: Arc<dyn EventSink>,
    runtime: tokio::runtime::Handle,
    jobs: Arc<JobStore>,
    running: Arc<Mutex<Option<Running>>>,
    last_scan: Mutex<Option<ScanSummary>>,
    models: ModelStore,
    download: Arc<Mutex<Option<Download>>>,
    journal: Journal,
    pipeline: PipelineConfig,
    next_job: AtomicU64,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

impl Engine {
    /// Creates the engine with the real services: ffmpeg sidecars (resolved on first use), the
    /// online sources with their cache, and whisper.cpp with the downloaded models.
    pub fn new(config: EngineConfig, sink: Arc<dyn EventSink>) -> crate::Result<Self> {
        let paths = DataPaths::new(&config.data_dir);
        std::fs::create_dir_all(&paths.root)?;
        let services = Services {
            media: Arc::new(FfmpegMedia::new(config.sidecars)),
            catalog: Arc::new(OnlineCatalog::open(&paths.cache_db, config.keys)?),
            speech: Arc::new(WhisperEngine::new(&paths.models)),
        };
        Ok(Self::with_services(
            &config.data_dir,
            config.settings,
            config.runtime,
            services,
            sink,
        ))
    }

    /// Creates the engine over the given services (tests and the command-line example).
    pub fn with_services(
        data_dir: &Path,
        settings: Settings,
        runtime: tokio::runtime::Handle,
        services: Services,
        sink: Arc<dyn EventSink>,
    ) -> Self {
        let paths = DataPaths::new(data_dir);
        Self {
            jobs: Arc::new(JobStore::new(&paths.jobs)),
            models: ModelStore::new(&paths.models),
            journal: Journal::new(&paths.history),
            paths,
            settings: RwLock::new(settings),
            services,
            sink,
            runtime,
            running: Arc::new(Mutex::new(None)),
            last_scan: Mutex::new(None),
            download: Arc::new(Mutex::new(None)),
            pipeline: PipelineConfig::default(),
            next_job: AtomicU64::new(1),
        }
    }

    /// Replaces the rules jobs use (tests).
    pub fn with_pipeline_config(mut self, config: PipelineConfig) -> Self {
        self.pipeline = config;
        self
    }

    /// The data folder layout.
    pub fn paths(&self) -> &DataPaths {
        &self.paths
    }

    /// Applies new settings and keys. A running job keeps the settings it started with.
    pub fn update_settings(&self, settings: Settings, keys: ApiKeys) {
        *self.settings.write().unwrap_or_else(|p| p.into_inner()) = settings;
        self.services.catalog.set_keys(keys);
    }

    /// The settings the next job will use.
    pub fn settings(&self) -> Settings {
        self.settings
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Scans a folder (Start screen → Confirm show). The scan is kept and reused by the job that
    /// identifies the same folder.
    pub async fn scan(&self, folder: &Path) -> crate::Result<ScanSummary> {
        let media = Arc::clone(&self.services.media);
        let path = folder.to_path_buf();
        let summary = self
            .runtime
            .spawn_blocking(move || media.scan(&path, &CancelFlag::new()))
            .await
            .map_err(|e| CoreError::Job(e.to_string()))??;
        *self.last_scan.lock().unwrap_or_else(|p| p.into_inner()) = Some(summary.clone());
        Ok(summary)
    }

    /// Searches shows (Confirm show). Results for the show name guessed from the scanned folder
    /// are marked as such.
    pub async fn search_shows(&self, query: &str) -> crate::Result<Vec<ShowCandidate>> {
        let mut found = self.services.catalog.search_shows(query).await?;
        let guessed = self
            .last_scan
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .and_then(|s| s.show_guess.clone())
            .is_some_and(|g| g.trim().eq_ignore_ascii_case(query.trim()));
        if guessed {
            for c in &mut found {
                c.guessed_from_folder = true;
            }
        }
        Ok(found)
    }

    /// Starts identifying; progress arrives through the sink. Returns `Busy` when a job runs.
    pub fn start_job(&self, request: JobRequest) -> crate::Result<JobId> {
        let mut running = self.running.lock().unwrap_or_else(|p| p.into_inner());
        if running.is_some() {
            return Err(CoreError::Busy);
        }
        let job_id = JobId(format!(
            "job-{}-{}",
            now_ms(),
            self.next_job.fetch_add(1, Ordering::Relaxed)
        ));
        let cancel = CancelFlag::new();
        let settings = self.settings();
        let record = self.jobs.insert(pipeline::new_record(
            &job_id,
            &request,
            settings.speech_model,
        ));
        let scan = self
            .last_scan
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .filter(|s| s.folder == request.folder);
        *running = Some(Running {
            job_id: job_id.clone(),
            cancel: cancel.clone(),
        });
        drop(running);

        let ctx = JobContext {
            job_id: job_id.clone(),
            request,
            settings,
            services: self.services.clone(),
            sink: Arc::clone(&self.sink),
            cancel,
            scan,
            record: Arc::clone(&record),
            config: self.pipeline.clone(),
        };
        let jobs = Arc::clone(&self.jobs);
        let running = Arc::clone(&self.running);
        let sink = Arc::clone(&self.sink);
        let id = job_id.clone();
        self.runtime.spawn(async move {
            let outcome = pipeline::run(ctx).await;
            {
                let mut r = record.lock().unwrap_or_else(|p| p.into_inner());
                r.finished_at_ms = Some(now_ms());
                let has_results = r
                    .results
                    .matches
                    .iter()
                    .any(|m| m.confidence.verdict != Verdict::PlayAll);
                if (outcome == Outcome::Finished || has_results)
                    && let Err(e) = jobs.save(&r)
                {
                    tracing::warn!(error = %e, "could not save the job's results");
                }
            }
            running
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take_if(|r| r.job_id == id);
            sink.job_event(pipeline::final_event(&id, &outcome));
        });
        Ok(job_id)
    }

    /// Requests cancellation of a job. Returns immediately; the job emits `Cancelled`. Cancelling
    /// a job that already ended does nothing.
    pub fn cancel_job(&self, job: &JobId) -> crate::Result<()> {
        if let Some(r) = self
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .filter(|r| &r.job_id == job)
        {
            r.cancel.cancel();
            return Ok(());
        }
        match self.jobs.get(job) {
            Some(_) => Ok(()),
            None => Err(CoreError::NotFound(format!("job {}", job.0))),
        }
    }

    /// Whether a job is running (the updater waits for it before relaunching).
    pub fn is_job_running(&self) -> bool {
        self.running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }

    /// Results so far (complete once the job finished). Saved results of earlier jobs are loaded
    /// from disk.
    pub fn job_results(&self, job: &JobId) -> crate::Result<JobResults> {
        let record = self
            .jobs
            .get(job)
            .ok_or_else(|| CoreError::NotFound(format!("job {}", job.0)))?;
        let record = record.lock().unwrap_or_else(|p| p.into_inner());
        Ok(record.results.clone())
    }

    /// Recent jobs for the Start screen, newest first.
    pub fn recent_jobs(&self) -> crate::Result<Vec<RecentJob>> {
        Ok(self.jobs.recent())
    }

    /// Builds a rename preview from the job's results and the user's decisions.
    pub fn plan_rename(&self, request: &RenamePlanRequest) -> crate::Result<RenamePlan> {
        let record = self
            .jobs
            .get(&request.job_id)
            .ok_or_else(|| CoreError::NotFound(format!("job {}", request.job_id.0)))?;
        let record = record.lock().unwrap_or_else(|p| p.into_inner());
        let context = PlanContext {
            show: &record.results.request.show,
            episodes: &record.results.episodes,
            files: &record.files,
            matches: &record.results.matches,
        };
        Ok(mi_rename::build_plan(context, request, &|p: &Path| {
            p.exists()
        })?)
    }

    /// Applies a plan (rename, copy or CSV export) after checking it belongs to the job: every
    /// item must be one of the job's files at its scanned path, and every target an absolute path
    /// without `.` or `..` parts.
    pub async fn apply_rename(&self, plan: RenamePlan) -> crate::Result<RenameOutcome> {
        let record = self
            .jobs
            .get(&plan.job_id)
            .ok_or_else(|| CoreError::NotFound(format!("job {}", plan.job_id.0)))?;
        let (show_name, transcripts) = {
            let r = record.lock().unwrap_or_else(|p| p.into_inner());
            for item in &plan.items {
                let known = r
                    .files
                    .iter()
                    .any(|f| f.id == item.file_id && f.path == item.from);
                let targets_ok = is_plain_absolute(&item.to)
                    && item.heard_subtitles_to.as_deref().is_none_or(is_plain_absolute);
                if !known || !targets_ok {
                    return Err(CoreError::Invalid(
                        "The rename preview no longer matches the identified files. Open the preview again."
                            .to_owned(),
                    ));
                }
            }
            (r.results.request.show.name.clone(), r.transcripts.clone())
        };
        let journal = self.journal.clone();
        let applied_plan = plan.clone();
        let outcome = self
            .runtime
            .spawn_blocking(move || {
                mi_rename::apply_plan(&applied_plan, &show_name, &journal, &|id| {
                    transcripts
                        .get(id)
                        .and_then(|t| mi_rename::heard_srt(&t.segments))
                })
            })
            .await
            .map_err(|e| CoreError::Job(e.to_string()))??;
        let saved = outcome.completed > 0
            || (matches!(plan.mode, SaveMode::ExportList { .. }) && outcome.failed.is_empty());
        if saved {
            let mut r = record.lock().unwrap_or_else(|p| p.into_inner());
            r.saved = true;
            if r.finished_at_ms.is_some()
                && let Err(e) = self.jobs.save(&r)
            {
                tracing::warn!(error = %e, "could not record that the results were saved");
            }
        }
        Ok(outcome)
    }

    /// History entries, newest first.
    pub fn history(&self) -> crate::Result<Vec<HistoryEntry>> {
        Ok(self.journal.list()?)
    }

    /// Undoes a History entry.
    pub async fn undo(&self, id: &HistoryId) -> crate::Result<UndoOutcome> {
        let journal = self.journal.clone();
        let id = id.clone();
        Ok(self
            .runtime
            .spawn_blocking(move || journal.undo(&id))
            .await
            .map_err(|e| CoreError::Job(e.to_string()))??)
    }

    /// Download state of a speech model; while it downloads, its latest progress.
    pub fn model_status(&self, model: SpeechModel) -> ModelStatus {
        if let Some(d) = self
            .download
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .filter(|d| d.model == model)
            && let Some(last) = &d.last
        {
            return last.clone();
        }
        self.models.status(model)
    }

    /// Downloads (or resumes) a model; progress arrives through the sink. `Busy` while another
    /// download runs; `Cancelled` when paused.
    pub async fn download_model(&self, model: SpeechModel) -> crate::Result<()> {
        let cancel = CancelFlag::new();
        {
            let mut download = self.download.lock().unwrap_or_else(|p| p.into_inner());
            if download.is_some() {
                return Err(CoreError::Busy);
            }
            *download = Some(Download {
                model,
                cancel: cancel.clone(),
                last: None,
            });
        }
        let sink = Arc::clone(&self.sink);
        let slot = Arc::clone(&self.download);
        let models = self.models.clone();
        let result = self
            .runtime
            .spawn(async move {
                let on_progress = move |status: ModelStatus| {
                    if let Some(d) = slot.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                        d.last = matches!(
                            status.state,
                            ModelState::Downloading { .. } | ModelState::Verifying
                        )
                        .then(|| status.clone());
                    }
                    sink.model_status(status);
                };
                models.download(model, &on_progress, &cancel).await
            })
            .await
            .map_err(|e| CoreError::Job(e.to_string()));
        *self.download.lock().unwrap_or_else(|p| p.into_inner()) = None;
        result??;
        Ok(())
    }

    /// Pauses a running model download (the partial file is kept and resumes later).
    pub fn pause_model_download(&self) {
        if let Some(d) = self
            .download
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            d.cancel.cancel();
        }
    }

    /// Status of each online source.
    pub fn source_status(&self) -> Vec<SourceStatus> {
        self.services.catalog.status()
    }
}

/// An absolute path with no `.` or `..` parts.
fn is_plain_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|c| !matches!(c, Component::ParentDir | Component::CurDir))
}
