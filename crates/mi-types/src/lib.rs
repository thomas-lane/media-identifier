//! Shared data types for Media Identifier.
//!
//! Every type that crosses a crate boundary or the Tauri command/event boundary lives here, so
//! that the Rust crates and the UI agree on one definition. Types derive `serde` (JSON, camelCase
//! fields) and `ts_rs::TS`. The UI's copies in `ui/src/types/generated/` are written by
//! `MI_UPDATE_BINDINGS=1 cargo test -p mi-types --test bindings`; the same test without the
//! variable fails when the committed files are stale, so CI catches a forgotten regeneration.
//!
//! Conventions:
//! - Durations and positions are seconds as `f64`.
//! - Timestamps are Unix milliseconds as `i64`. 64-bit integers are exported to TypeScript as
//!   `number` (not `bigint`, because JSON numbers arrive as `number`); every value used here is
//!   far below 2^53.
//! - Scores, similarities and confidences are `f32` in `0.0..=1.0`.
//! - Paths are absolute, as `std::path::PathBuf` (a `string` in TypeScript).
//!
//! The only non-serde item is [`cancel::CancelFlag`], the cooperative cancellation flag every
//! long-running function accepts.

pub mod cancel;
pub mod catalog;
pub mod error;
pub mod events;
pub mod job;
pub mod matching;
pub mod media;
pub mod models;
pub mod reference;
pub mod rename;
pub mod settings;
pub mod transcript;
pub mod update;

pub use cancel::CancelFlag;
pub use catalog::*;
pub use error::*;
pub use job::*;
pub use matching::*;
pub use media::*;
pub use models::*;
pub use reference::*;
pub use rename::*;
pub use settings::*;
pub use transcript::*;
pub use update::*;
