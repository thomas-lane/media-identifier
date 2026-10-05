//! Identification jobs and their progress events.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::catalog::{Episode, EpisodeOrdering, Show};
use crate::matching::{FileMatch, Verdict};
use crate::media::FileId;
use crate::transcript::SpeechModel;

/// Identifier of one identification job (a UUID-like string).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct JobId(pub String);

/// What the Confirm show screen sends to start identifying.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct JobRequest {
    /// The scanned folder.
    pub folder: PathBuf,
    /// The confirmed show.
    pub show: Show,
    /// Episode numbering to use.
    pub ordering: EpisodeOrdering,
    /// Restrict candidates to these seasons; `None` means all seasons.
    pub seasons: Option<Vec<u32>>,
    /// Language of the audio and subtitles (ISO 639-1); `en` by default.
    pub language: String,
}

/// The stages shown as progress bars on the Identifying screen, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    /// Downloading the episode list.
    EpisodeList,
    /// Fetching reference text (subtitles, lyrics, embedded streams).
    Subtitles,
    /// Locating each file in the play-all.
    DiscOrder,
    /// Transcribing files.
    Listening,
    /// Scoring and assigning.
    Matching,
}

/// The state of one stage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StageState {
    /// Not started.
    Waiting,
    /// In progress; `done` of `total` units finished.
    Running {
        /// Units finished.
        done: u32,
        /// Units in total.
        total: u32,
    },
    /// Finished.
    Done,
    /// Not needed for this job (for example disc order without a play-all).
    Skipped,
    /// Failed; the job continues without this stage when it can.
    Failed {
        /// Plain-language reason.
        message: String,
    },
}

/// Per-file status on the Identifying screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FileStatus {
    /// Queued.
    Waiting,
    /// Being transcribed.
    Listening,
    /// Being scored.
    Matching,
    /// Finished with a verdict.
    Done,
    /// Could not be processed.
    Failed,
}

/// The processor running the speech model, shown next to the time estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Accelerator {
    /// Apple GPU through Metal.
    AppleGpu,
    /// A GPU through Vulkan (Windows).
    Vulkan,
    /// The CPU.
    Cpu,
}

/// Progress events emitted on the `job-event` channel, in order, for one job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum JobEvent {
    /// The job started.
    Started {
        /// The job.
        job_id: JobId,
        /// Files to identify, in display order.
        file_ids: Vec<FileId>,
        /// Processor in use.
        accelerator: Accelerator,
    },
    /// A stage changed state.
    Stage {
        /// The job.
        job_id: JobId,
        /// The stage.
        stage: Stage,
        /// Its new state.
        state: StageState,
    },
    /// A file changed status.
    File {
        /// The job.
        job_id: JobId,
        /// The file.
        file_id: FileId,
        /// Its new status.
        status: FileStatus,
        /// Best episode title so far, for the "Best match so far" column.
        best_so_far: Option<String>,
        /// Verdict once known.
        verdict: Option<Verdict>,
    },
    /// The episode list is known (sent once, after the EpisodeList stage).
    Episodes {
        /// The job.
        job_id: JobId,
        /// All candidate episodes.
        episodes: Vec<Episode>,
    },
    /// A file's match is ready for review. May be sent again for the same file when a later
    /// global assignment changes it.
    Matched {
        /// The job.
        job_id: JobId,
        /// The result.
        result: FileMatch,
    },
    /// Estimated time remaining.
    Eta {
        /// The job.
        job_id: JobId,
        /// Seconds remaining.
        seconds: f64,
    },
    /// The job finished; every file has a final `Matched` event.
    Finished {
        /// The job.
        job_id: JobId,
    },
    /// The job was cancelled by the user.
    Cancelled {
        /// The job.
        job_id: JobId,
    },
    /// The job failed as a whole.
    Failed {
        /// The job.
        job_id: JobId,
        /// Plain-language reason.
        message: String,
    },
}

/// Final results of a job, for the Review screen (and for reopening after the window reloads).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct JobResults {
    /// The job.
    pub job_id: JobId,
    /// The original request.
    pub request: JobRequest,
    /// All candidate episodes.
    pub episodes: Vec<Episode>,
    /// One entry per identified file (play-all included, with `Verdict::PlayAll`).
    pub matches: Vec<FileMatch>,
    /// Speech model used.
    pub model: SpeechModel,
    /// True when every file has a final result.
    pub complete: bool,
}

/// A row of the Recent list on the Start screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecentJob {
    /// The job.
    pub job_id: JobId,
    /// Folder that was identified.
    pub folder: PathBuf,
    /// Show name.
    pub show_name: String,
    /// Number of files.
    pub file_count: u32,
    /// Files still needing review.
    pub to_review: u32,
    /// Whether the results were saved (renamed, copied or exported).
    pub saved: bool,
    /// When it finished, Unix milliseconds.
    pub finished_at_ms: i64,
}
