//! One identification job, stage by stage.
//!
//! Order: (1) episode list; (2) reference text, including embedded subtitle streams; (3) disc
//! order: fingerprint every candidate and the play-all and locate each inside it (skipped without
//! a play-all); (4) listening: plan windows per file, decode, transcribe, filter; (5) matching:
//! score all, assign, classify; files whose margin is low get escalation windows and are matched
//! again. Stages 2-4 overlap where possible; `Matched` events are sent as files finish and again
//! after the final global assignment.

use std::sync::Arc;

use mi_types::{CancelFlag, JobId, JobRequest};

use crate::EventSink;

/// Runs one job to completion, emitting events through `sink`. Returns when the job finished,
/// failed or was cancelled; the last event is `Finished`, `Failed` or `Cancelled`.
pub async fn run(job: JobId, request: JobRequest, sink: Arc<dyn EventSink>, cancel: CancelFlag) {
    let _ = (job, request, sink, cancel);
    todo!("integrator: pipeline")
}
