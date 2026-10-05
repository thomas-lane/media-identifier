//! Job results in memory and on disk.
//!
//! Each job is one JSON file, `<data>/jobs/<job id>.json`, written when the job ends (finished,
//! cancelled or failed after producing results) and again when its results are saved
//! (renamed, copied or exported). It holds what Review and Rename need after a relaunch: the
//! results, the scanned files and what was heard in each file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mi_types::{FileId, JobId, JobResults, MediaFile, RecentJob, Transcript, Verdict};
use serde::{Deserialize, Serialize};

/// Everything kept about one job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecord {
    /// The results shown in Review.
    pub results: JobResults,
    /// The scanned files (paths and probes), for the rename plan.
    pub files: Vec<MediaFile>,
    /// What was heard in each file, for "Also save subtitles of what was heard".
    pub transcripts: HashMap<FileId, Transcript>,
    /// Whether the results were saved (renamed, copied or exported).
    pub saved: bool,
    /// When the job ended, Unix milliseconds; `None` while it runs.
    pub finished_at_ms: Option<i64>,
}

impl JobRecord {
    /// The Recent row for this job.
    pub fn recent(&self) -> RecentJob {
        let to_review = if self.saved {
            0
        } else {
            self.results
                .matches
                .iter()
                .filter(|m| m.confidence.verdict == Verdict::Check)
                .count() as u32
        };
        RecentJob {
            job_id: self.results.job_id.clone(),
            folder: self.results.request.folder.clone(),
            show_name: self.results.request.show.name.clone(),
            file_count: self
                .results
                .matches
                .iter()
                .filter(|m| m.confidence.verdict != Verdict::PlayAll)
                .count() as u32,
            to_review,
            saved: self.saved,
            finished_at_ms: self.finished_at_ms.unwrap_or_default(),
        }
    }
}

/// A shared, updatable job record.
pub type SharedRecord = Arc<Mutex<JobRecord>>;

/// Jobs of this session in memory, earlier ones on disk.
#[derive(Debug)]
pub struct JobStore {
    dir: PathBuf,
    live: Mutex<HashMap<JobId, SharedRecord>>,
}

/// How many jobs the Recent list shows.
pub const RECENT_LIMIT: usize = 20;

impl JobStore {
    /// Keeps saved jobs in `dir` (created on first save).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            live: Mutex::new(HashMap::new()),
        }
    }

    /// Registers a job of this session.
    pub fn insert(&self, record: JobRecord) -> SharedRecord {
        let id = record.results.job_id.clone();
        let shared = Arc::new(Mutex::new(record));
        self.live
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, Arc::clone(&shared));
        shared
    }

    /// A job of this session, or one saved earlier.
    pub fn get(&self, id: &JobId) -> Option<SharedRecord> {
        if let Some(r) = self
            .live
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
        {
            return Some(Arc::clone(r));
        }
        let record = read_record(&self.path_of(id)?)?;
        Some(self.insert(record))
    }

    /// Writes a job's record to disk (temporary file, then rename).
    pub fn save(&self, record: &JobRecord) -> std::io::Result<()> {
        let Some(path) = self.path_of(&record.results.job_id) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid job id",
            ));
        };
        std::fs::create_dir_all(&self.dir)?;
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(record).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &path)
    }

    /// Finished jobs, newest first, at most [`RECENT_LIMIT`]. Running jobs are left out;
    /// unreadable files are skipped.
    pub fn recent(&self) -> Vec<RecentJob> {
        let mut by_id: HashMap<JobId, RecentJob> = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json")
                    && let Some(record) = read_record(&path)
                    && record.finished_at_ms.is_some()
                {
                    by_id.insert(record.results.job_id.clone(), record.recent());
                }
            }
        }
        for record in self
            .live
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
        {
            let record = record.lock().unwrap_or_else(|p| p.into_inner());
            if record.finished_at_ms.is_some() {
                by_id.insert(record.results.job_id.clone(), record.recent());
            }
        }
        let mut list: Vec<RecentJob> = by_id.into_values().collect();
        list.sort_by(|a, b| {
            b.finished_at_ms
                .cmp(&a.finished_at_ms)
                .then(b.job_id.0.cmp(&a.job_id.0))
        });
        list.truncate(RECENT_LIMIT);
        list
    }

    /// The file of a job; `None` for an id that is not a plain file name (it comes from the UI).
    fn path_of(&self, id: &JobId) -> Option<PathBuf> {
        let ok = !id.0.is_empty()
            && id
                .0
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        ok.then(|| self.dir.join(format!("{}.json", id.0)))
    }
}

fn read_record(path: &Path) -> Option<JobRecord> {
    let bytes = std::fs::read(path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(r) => Some(r),
        Err(e) => {
            tracing::warn!(?path, error = %e, "skipping an unreadable saved job");
            None
        }
    }
}
