//! The pinned model files: the two speech models and the voice activity detection model.

use mi_types::{ModelInfo, SpeechModel};

/// Commit of `huggingface.co/ggerganov/whisper.cpp` the speech model URLs are pinned to, so the
/// bytes behind a URL cannot change under the pinned digest.
pub const MODEL_REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";

/// Commit of `huggingface.co/ggml-org/whisper-vad` the voice activity detection model URL is
/// pinned to.
pub const VAD_REVISION: &str = "9ffd54a1e1ee413ddf265af9913beaf518d1639b";

/// A file downloaded from a pinned URL and accepted only when its size and SHA-256 match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedFile {
    /// File name on disk.
    pub file_name: String,
    /// Exact size in bytes.
    pub size_bytes: u64,
    /// Lowercase hex SHA-256 of the whole file.
    pub sha256: String,
    /// Download URL, pinned to a repository revision.
    pub url: String,
}

impl From<&ModelInfo> for PinnedFile {
    fn from(info: &ModelInfo) -> Self {
        Self {
            file_name: info.file_name.clone(),
            size_bytes: info.size_bytes,
            sha256: info.sha256.clone(),
            url: info.url.clone(),
        }
    }
}

/// Returns the pinned file for a speech model.
///
/// Sizes and SHA-256 digests are the Git LFS object ids Hugging Face reports for
/// [`MODEL_REVISION`]; a download is accepted only when both match.
pub fn model_info(model: SpeechModel) -> ModelInfo {
    let (file_name, size_bytes, sha256) = match model {
        SpeechModel::Accurate => (
            "ggml-large-v3-turbo-q5_0.bin",
            574_041_195,
            "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        ),
        SpeechModel::Fast => (
            "ggml-small.en-q5_1.bin",
            190_098_681,
            "bfdff4894dcb76bbf647d56263ea2a96645423f1669176f4844a1bf8e478ad30",
        ),
    };
    ModelInfo {
        model,
        file_name: file_name.to_owned(),
        size_bytes,
        sha256: sha256.to_owned(),
        url: format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/{MODEL_REVISION}/{file_name}"
        ),
    }
}

/// The Silero voice activity detection model converted for whisper.cpp (MIT, 885 KB).
///
/// It is downloaded together with either speech model. whisper.cpp uses it to skip stretches
/// without speech in long files (see [`crate::sampling::use_vad`]).
pub fn vad_model_file() -> PinnedFile {
    let file_name = "ggml-silero-v5.1.2.bin";
    PinnedFile {
        file_name: file_name.to_owned(),
        size_bytes: 885_098,
        sha256: "29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf".to_owned(),
        url: format!(
            "https://huggingface.co/ggml-org/whisper-vad/resolve/{VAD_REVISION}/{file_name}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_pinned(file: &PinnedFile, revision: &str) {
        assert!(file.url.contains(revision), "{} is not pinned", file.url);
        assert!(file.url.starts_with("https://huggingface.co/"));
        assert!(file.url.ends_with(&file.file_name));
        assert_eq!(file.sha256.len(), 64);
        assert!(
            file.sha256
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert!(file.size_bytes > 0);
    }

    #[test]
    fn every_file_is_pinned_to_a_revision_with_a_well_formed_digest() {
        for model in [SpeechModel::Fast, SpeechModel::Accurate] {
            let info = model_info(model);
            assert_eq!(info.model, model);
            assert_pinned(&PinnedFile::from(&info), MODEL_REVISION);
        }
        assert_pinned(&vad_model_file(), VAD_REVISION);
        assert!(
            model_info(SpeechModel::Accurate).size_bytes > model_info(SpeechModel::Fast).size_bytes
        );
    }

    #[test]
    fn file_names_are_distinct() {
        let names = [
            model_info(SpeechModel::Fast).file_name,
            model_info(SpeechModel::Accurate).file_name,
            vad_model_file().file_name,
        ];
        assert_ne!(names[0], names[1]);
        assert_ne!(names[0], names[2]);
        assert_ne!(names[1], names[2]);
    }
}
