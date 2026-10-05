//! Cooperative cancellation shared by every long-running operation.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A cheap, cloneable cancellation flag.
///
/// Long-running functions (probing, audio extraction, transcription, downloads, matching) take a
/// `&CancelFlag` and check [`CancelFlag::is_cancelled`] between units of work, returning their
/// crate's `Cancelled` error when it is set. Clones share the same flag, so the job owner keeps one
/// clone and hands others to workers. A plain atomic is used instead of an async token because
/// the speech model runs on blocking threads that cannot await.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    /// Creates a flag that is not cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation. Every clone observes it.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Returns whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_cancellation() {
        let flag = CancelFlag::new();
        let worker = flag.clone();
        assert!(!worker.is_cancelled());
        flag.cancel();
        assert!(worker.is_cancelled());
    }
}
