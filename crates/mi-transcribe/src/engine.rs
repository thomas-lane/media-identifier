//! Running the speech model.

use std::path::Path;

use mi_types::{Accelerator, CancelFlag, Segment};

/// Decoding settings, matching the owner's decisions in `docs/identification.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeOptions {
    /// Language (ISO 639-1); `en` by default.
    pub language: String,
    /// Do not feed previous text back as a prompt; stops one misheard line from steering the rest.
    pub no_context: bool,
    /// Suppress non-speech tokens (music notes, sound descriptions).
    pub suppress_non_speech: bool,
    /// Starting temperature; on a failed decode whisper.cpp retries with higher temperatures.
    pub temperature: f32,
    /// Temperature step for those retries (0 disables the fallback).
    pub temperature_increment: f32,
    /// Use voice activity detection. Off for clips of six minutes or less and for music-heavy
    /// content, where it cuts sung words.
    pub vad: bool,
    /// CPU threads.
    pub threads: u32,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            language: "en".to_owned(),
            no_context: true,
            suppress_non_speech: true,
            temperature: 0.0,
            temperature_increment: 0.2,
            vad: false,
            threads: 4,
        }
    }
}

/// Something that turns 16 kHz mono PCM into timed segments.
///
/// A trait so `mi-core` and tests can use a scripted transcriber without a model file.
pub trait Transcriber: Send {
    /// Transcribes `samples` (16 kHz mono) whose first sample is at `start_s` seconds in the file;
    /// returned segment times are file times. Stops with `Cancelled` when `cancel` is set (checked
    /// through whisper.cpp's abort callback).
    fn transcribe(
        &mut self,
        samples: &[f32],
        start_s: f64,
        options: &DecodeOptions,
        cancel: &CancelFlag,
    ) -> crate::Result<Vec<Segment>>;

    /// The processor in use.
    fn accelerator(&self) -> Accelerator;
}

/// The whisper.cpp implementation.
pub struct WhisperTranscriber {
    context: whisper_rs::WhisperContext,
    accelerator: Accelerator,
}

impl std::fmt::Debug for WhisperTranscriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WhisperTranscriber")
            .field("accelerator", &self.accelerator)
            .finish()
    }
}

impl WhisperTranscriber {
    /// Loads a model file. `use_gpu` selects Metal on macOS (Vulkan on Windows when built with the
    /// `vulkan` feature) and falls back to the CPU when the GPU cannot be initialised.
    pub fn load(model_path: &Path, use_gpu: bool) -> crate::Result<Self> {
        let _ = (model_path, use_gpu);
        todo!("transcribe module: create WhisperContext")
    }
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(
        &mut self,
        samples: &[f32],
        start_s: f64,
        options: &DecodeOptions,
        cancel: &CancelFlag,
    ) -> crate::Result<Vec<Segment>> {
        let _ = (&self.context, samples, start_s, options, cancel);
        todo!("transcribe module: run whisper_full and convert segments")
    }

    fn accelerator(&self) -> Accelerator {
        self.accelerator
    }
}

/// Version of the linked whisper.cpp.
pub fn whisper_cpp_version() -> &'static str {
    whisper_rs::WHISPER_CPP_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whisper_cpp_is_linked() {
        assert!(!whisper_cpp_version().is_empty());
        assert!(whisper_rs::get_lang_id("en").is_some());
    }
}
