//! Forwarding engine events to the window.

use mi_core::EventSink;
use mi_types::{JobEvent, ModelStatus, events};
use tauri::{AppHandle, Emitter};

/// Emits engine events as Tauri events on the channels in [`mi_types::events`].
pub struct TauriSink {
    app: AppHandle,
}

impl TauriSink {
    /// Creates the sink.
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl EventSink for TauriSink {
    fn job_event(&self, event: JobEvent) {
        if let Err(e) = self.app.emit(events::JOB_EVENT, event) {
            tracing::warn!("could not emit job event: {e}");
        }
    }

    fn model_status(&self, status: ModelStatus) {
        if let Err(e) = self.app.emit(events::MODEL_DOWNLOAD_EVENT, status) {
            tracing::warn!("could not emit model status: {e}");
        }
    }
}
