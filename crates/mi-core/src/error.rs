//! Errors and their mapping to the command error shape.

use mi_types::{ApiError, ErrorCode};

/// Errors from the engine.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// From `mi-media`.
    #[error(transparent)]
    Media(#[from] mi_media::MediaError),
    /// From `mi-transcribe`.
    #[error(transparent)]
    Transcribe(#[from] mi_transcribe::TranscribeError),
    /// From `mi-sources`.
    #[error(transparent)]
    Sources(#[from] mi_sources::SourceError),
    /// From `mi-match`.
    #[error(transparent)]
    Match(#[from] mi_match::MatchError),
    /// From `mi-rename`.
    #[error(transparent)]
    Rename(#[from] mi_rename::RenameError),
    /// Another job is running; only one runs at a time.
    #[error("an identification is already running")]
    Busy,
    /// No such job or entry.
    #[error("{0} not found")]
    NotFound(String),
    /// Cancelled.
    #[error("cancelled")]
    Cancelled,
}

impl From<CoreError> for ApiError {
    fn from(error: CoreError) -> Self {
        use mi_media::MediaError as M;
        use mi_sources::SourceError as S;
        use mi_transcribe::TranscribeError as T;
        let code = match &error {
            CoreError::Busy => ErrorCode::Busy,
            CoreError::NotFound(_) => ErrorCode::NotFound,
            CoreError::Cancelled
            | CoreError::Media(M::Cancelled)
            | CoreError::Transcribe(T::Cancelled)
            | CoreError::Sources(S::Cancelled)
            | CoreError::Match(mi_match::MatchError::Cancelled) => ErrorCode::Cancelled,
            CoreError::Media(M::Io(_)) | CoreError::Transcribe(T::Io(_)) => ErrorCode::Io,
            CoreError::Media(_) => ErrorCode::MediaTool,
            CoreError::Transcribe(T::Download(_)) => ErrorCode::Network,
            CoreError::Transcribe(_) => ErrorCode::SpeechModel,
            CoreError::Sources(S::RateLimited(_)) => ErrorCode::RateLimited,
            CoreError::Sources(S::KeyRejected(_)) => ErrorCode::KeyRejected,
            CoreError::Sources(S::KeyMissing(_)) => ErrorCode::InvalidInput,
            CoreError::Sources(S::Io(_)) => ErrorCode::Io,
            CoreError::Sources(S::Cache(_)) => ErrorCode::Internal,
            CoreError::Sources(_) => ErrorCode::Network,
            CoreError::Match(_) => ErrorCode::Internal,
            CoreError::Rename(mi_rename::RenameError::Conflicts(_)) => ErrorCode::Conflict,
            CoreError::Rename(mi_rename::RenameError::NotFound(_)) => ErrorCode::NotFound,
            CoreError::Rename(mi_rename::RenameError::BadTemplate(_)) => ErrorCode::InvalidInput,
            CoreError::Rename(_) => ErrorCode::Io,
        };
        ApiError::new(code, error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_from_any_crate_maps_to_cancelled() {
        let errors = [
            CoreError::Cancelled,
            CoreError::Media(mi_media::MediaError::Cancelled),
            CoreError::Transcribe(mi_transcribe::TranscribeError::Cancelled),
            CoreError::Sources(mi_sources::SourceError::Cancelled),
            CoreError::Match(mi_match::MatchError::Cancelled),
        ];
        for e in errors {
            assert_eq!(ApiError::from(e).code, ErrorCode::Cancelled);
        }
    }

    #[test]
    fn busy_and_rate_limits_keep_their_codes() {
        assert_eq!(ApiError::from(CoreError::Busy).code, ErrorCode::Busy);
        let limited = CoreError::Sources(mi_sources::SourceError::RateLimited(
            mi_types::ProviderId::Tvmaze,
        ));
        assert_eq!(ApiError::from(limited).code, ErrorCode::RateLimited);
    }
}
