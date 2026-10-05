//! The pinned speech model files.

use mi_types::{ModelInfo, SpeechModel};

/// Commit of `huggingface.co/ggerganov/whisper.cpp` the URLs are pinned to, so the bytes behind
/// a URL cannot change under the pinned digest.
pub const MODEL_REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";

/// Returns the pinned file for a model.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_pinned_to_the_revision_and_digests_are_well_formed() {
        for model in [SpeechModel::Fast, SpeechModel::Accurate] {
            let info = model_info(model);
            assert!(info.url.contains(MODEL_REVISION));
            assert!(info.url.ends_with(&info.file_name));
            assert_eq!(info.sha256.len(), 64);
            assert!(
                info.sha256
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            );
        }
        assert!(
            model_info(SpeechModel::Accurate).size_bytes > model_info(SpeechModel::Fast).size_bytes
        );
    }
}
