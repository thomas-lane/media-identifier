//! The engine the Tauri commands call.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use mi_media::SidecarLookup;
use mi_sources::ApiKeys;
use mi_types::{
    HistoryEntry, HistoryId, JobEvent, JobId, JobRequest, JobResults, ModelStatus, RecentJob,
    RenameOutcome, RenamePlan, RenamePlanRequest, ScanSummary, Settings, ShowCandidate,
    SourceStatus, SpeechModel, UndoOutcome,
};

use crate::DataPaths;

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
}

/// Long-lived services and at most one running job.
///
/// Concurrency contract: methods take `&self` and may be called from any thread. Only one
/// identification job runs at a time ([`Engine::start_job`] returns `Busy` otherwise); a job's
/// blocking work (ffmpeg, whisper.cpp) runs on Tokio's blocking pool and its network work on the
/// async runtime. Cancelling sets the job's `CancelFlag`; the job then emits `Cancelled` as its
/// last event.
pub struct Engine {
    paths: DataPaths,
    sidecars: SidecarLookup,
    settings: RwLock<(Settings, ApiKeys)>,
    sink: Arc<dyn EventSink>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Creates the engine and its data folders. Resolving the sidecars is deferred to first use,
    /// so the app can start (and show a clear error) when they are missing.
    pub fn new(config: EngineConfig, sink: Arc<dyn EventSink>) -> crate::Result<Self> {
        let paths = DataPaths::new(&config.data_dir);
        Ok(Self {
            paths,
            sidecars: config.sidecars,
            settings: RwLock::new((config.settings, config.keys)),
            sink,
        })
    }

    /// The data folder layout.
    pub fn paths(&self) -> &DataPaths {
        &self.paths
    }

    /// Applies new settings and keys. A running job keeps the settings it started with.
    pub fn update_settings(&self, settings: Settings, keys: ApiKeys) {
        *self.settings.write().expect("settings lock") = (settings, keys);
    }

    /// The settings the next job will use.
    pub fn settings(&self) -> Settings {
        self.settings.read().expect("settings lock").0.clone()
    }

    /// Scans a folder (Start screen → Confirm show).
    pub async fn scan(&self, folder: &Path) -> crate::Result<ScanSummary> {
        let _ = (&self.sidecars, &self.sink, folder);
        todo!("integrator: mi_media::scan_folder on the blocking pool")
    }

    /// Searches shows (Confirm show).
    pub async fn search_shows(&self, query: &str) -> crate::Result<Vec<ShowCandidate>> {
        let _ = query;
        todo!("integrator: Sources::search_shows")
    }

    /// Starts identifying; progress arrives through the sink. Returns `Busy` when a job runs.
    pub fn start_job(&self, request: JobRequest) -> crate::Result<JobId> {
        let _ = request;
        todo!("integrator: spawn crate::pipeline::run")
    }

    /// Requests cancellation of a job. Returns immediately; the job emits `Cancelled`.
    pub fn cancel_job(&self, job: &JobId) -> crate::Result<()> {
        let _ = job;
        todo!("integrator: cancel running job")
    }

    /// Whether a job is running (the updater waits for it before relaunching).
    pub fn is_job_running(&self) -> bool {
        todo!("integrator: job state")
    }

    /// Results so far (complete once the job finished). Saved results of earlier jobs are loaded
    /// from disk.
    pub fn job_results(&self, job: &JobId) -> crate::Result<JobResults> {
        let _ = job;
        todo!("integrator: job results")
    }

    /// Recent jobs for the Start screen, newest first.
    pub fn recent_jobs(&self) -> crate::Result<Vec<RecentJob>> {
        todo!("integrator: recent jobs")
    }

    /// Builds a rename preview.
    pub fn plan_rename(&self, request: &RenamePlanRequest) -> crate::Result<RenamePlan> {
        let _ = request;
        todo!("integrator: mi_rename::build_plan")
    }

    /// Applies a plan (rename, copy or CSV export).
    pub fn apply_rename(&self, plan: &RenamePlan) -> crate::Result<RenameOutcome> {
        let _ = plan;
        todo!("integrator: mi_rename::apply_plan / export_csv")
    }

    /// History entries, newest first.
    pub fn history(&self) -> crate::Result<Vec<HistoryEntry>> {
        todo!("integrator: Journal::list")
    }

    /// Undoes a History entry.
    pub fn undo(&self, id: &HistoryId) -> crate::Result<UndoOutcome> {
        let _ = id;
        todo!("integrator: Journal::undo")
    }

    /// Download state of a speech model.
    pub fn model_status(&self, model: SpeechModel) -> ModelStatus {
        let _ = model;
        todo!("integrator: ModelStore::status")
    }

    /// Downloads (or resumes) a model; progress arrives through the sink.
    pub async fn download_model(&self, model: SpeechModel) -> crate::Result<()> {
        let _ = model;
        todo!("integrator: ModelStore::download")
    }

    /// Pauses a running model download (the partial file is kept and resumes later).
    pub fn pause_model_download(&self) {
        todo!("integrator: cancel the download's flag")
    }

    /// Status of each online source.
    pub fn source_status(&self) -> Vec<SourceStatus> {
        todo!("integrator: Sources::status")
    }
}
