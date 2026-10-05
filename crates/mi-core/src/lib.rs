//! Pipeline orchestration for Media Identifier.
//!
//! [`Engine`] owns the long-lived services (media access, the online sources and their cache, the
//! speech models, the History journal and saved jobs) and runs identification jobs: scan, episode
//! list, reference text, disc order, listening and matching, emitting [`mi_types::JobEvent`]s
//! through an [`EventSink`]. The Tauri app is a thin layer over it. Every service sits behind a
//! trait in [`services`], so tests drive whole jobs with scripted media, catalog and speech.
//!
//! Owner: integrator (see `docs/architecture.md`).

pub mod engine;
pub mod error;
pub mod jobs;
pub mod paths;
pub mod pipeline;
pub mod services;

pub use engine::{Engine, EngineConfig, EventSink};
pub use error::CoreError;
pub use jobs::{JobRecord, JobStore};
pub use paths::DataPaths;
pub use pipeline::{Outcome, PipelineConfig};
pub use services::{
    Catalog, FfmpegMedia, Listener, MediaBackend, OnlineCatalog, Services, SpeechEngine,
    WhisperEngine,
};

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, CoreError>;
