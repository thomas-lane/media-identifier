//! Pipeline orchestration for Media Identifier.
//!
//! [`Engine`] owns the long-lived services (sidecar paths, model store, sources and cache,
//! history journal) and runs identification jobs: episode list, reference text, disc order,
//! listening and matching, emitting [`mi_types::JobEvent`]s through an [`EventSink`]. The Tauri
//! app is a thin layer over it; tests drive it with fake sinks.
//!
//! Owner: integrator (see `docs/architecture.md`).

pub mod engine;
pub mod error;
pub mod paths;
pub mod pipeline;

pub use engine::{Engine, EngineConfig, EventSink};
pub use error::CoreError;
pub use paths::DataPaths;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, CoreError>;
