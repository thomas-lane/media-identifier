//! Running the speech model.

use std::borrow::Cow;
use std::path::Path;

use mi_types::{Accelerator, CancelFlag, Segment};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
    WhisperVadContext, WhisperVadContextParams, WhisperVadParams,
};

use crate::TranscribeError;
use crate::sampling::{SamplingPolicy, use_vad};
use crate::vad::SpeechLayout;

/// Sample rate of the PCM the speech model takes (mono, `f32` in -1..1).
pub const SAMPLE_RATE: u32 = 16_000;

/// whisper.cpp refuses input shorter than one second; shorter clips are padded with silence to
/// this many samples (1.05 s).
const MIN_SAMPLES: usize = 16_800;

/// Decoding settings, matching the owner's decisions in `docs/identification.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeOptions {
    /// Language (ISO 639-1); `en` by default. `auto` lets the model detect it. The Fast model is
    /// English-only and always decodes English.
    pub language: String,
    /// Do not feed previous text back as a prompt; stops one misheard line from steering the rest.
    pub no_context: bool,
    /// Suppress non-speech tokens (music notes, sound descriptions).
    pub suppress_non_speech: bool,
    /// Starting temperature; on a failed decode whisper.cpp retries with higher temperatures.
    pub temperature: f32,
    /// Temperature step for those retries (0 disables the fallback).
    pub temperature_increment: f32,
    /// Candidates sampled per retry at a temperature above zero; the most probable is kept.
    pub best_of: u32,
    /// A decode whose token entropy is below this is treated as failed (looping) and retried at
    /// the next temperature.
    pub entropy_threshold: f32,
    /// A decode whose average token log-probability is below this is treated as failed and
    /// retried at the next temperature.
    pub logprob_threshold: f32,
    /// A window whose no-speech probability is above this (and whose decode failed the
    /// log-probability check) is treated as silence.
    pub no_speech_threshold: f32,
    /// Use voice activity detection. Off for clips of six minutes or less and for music-heavy
    /// content, where it cuts sung words (see [`use_vad`]). Needs a VAD model, loaded with
    /// [`WhisperTranscriber::with_vad_model`]; without one this setting is ignored (with a
    /// warning in the log). A window in which no speech is detected yields no segments.
    pub vad: bool,
    /// CPU threads.
    pub threads: u32,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4) as u32;
        Self {
            language: "en".to_owned(),
            no_context: true,
            suppress_non_speech: true,
            temperature: 0.0,
            temperature_increment: 0.2,
            best_of: 5,
            entropy_threshold: 2.4,
            logprob_threshold: -1.0,
            no_speech_threshold: 0.6,
            vad: false,
            threads,
        }
    }
}

impl DecodeOptions {
    /// Default options for one file: voice activity detection is chosen by [`use_vad`].
    pub fn for_file(duration_s: f64, music_heavy: bool, policy: &SamplingPolicy) -> Self {
        Self {
            vad: use_vad(duration_s, music_heavy, policy),
            ..Self::default()
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
///
/// Holds the loaded model and one decoding state, which is reused across calls so memory is
/// allocated once per model rather than once per window.
pub struct WhisperTranscriber {
    context: WhisperContext,
    state: WhisperState,
    accelerator: Accelerator,
    vad: Option<WhisperVadContext>,
}

impl std::fmt::Debug for WhisperTranscriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WhisperTranscriber")
            .field("accelerator", &self.accelerator)
            .field("vad", &self.vad.is_some())
            .finish()
    }
}

fn engine_error(context: &str, error: impl std::fmt::Display) -> TranscribeError {
    TranscribeError::Engine(format!("{context}: {error}"))
}

/// Whether a Vulkan device is available (builds with the `vulkan` feature only).
#[cfg(feature = "vulkan")]
fn vulkan_available() -> bool {
    !whisper_rs::vulkan::list_devices().is_empty()
}

#[cfg(not(feature = "vulkan"))]
fn vulkan_available() -> bool {
    false
}

/// The GPU backend compiled into this build and usable on this computer, if any.
fn gpu_accelerator() -> Option<Accelerator> {
    if cfg!(target_os = "macos") {
        Some(Accelerator::AppleGpu)
    } else if vulkan_available() {
        Some(Accelerator::Vulkan)
    } else {
        None
    }
}

/// Voice activity detection settings: the whisper.cpp defaults, with 100 ms of padding around
/// each speech region (instead of 30 ms) so the first and last syllables are not clipped.
fn vad_params() -> WhisperVadParams {
    let mut params = WhisperVadParams::default();
    params.set_speech_pad(100);
    params
}

impl WhisperTranscriber {
    /// Loads a model file. `use_gpu` selects Metal on macOS (Vulkan on Windows when built with the
    /// `vulkan` feature and a Vulkan device exists) and falls back to the CPU when the GPU context
    /// cannot be created.
    ///
    /// whisper.cpp's own log output is routed to `tracing` (target `whisper_rs`) instead of
    /// standard error.
    pub fn load(model_path: &Path, use_gpu: bool) -> crate::Result<Self> {
        whisper_rs::install_logging_hooks();
        if !model_path.is_file() {
            return Err(TranscribeError::ModelMissing(
                model_path.display().to_string(),
            ));
        }
        let path = model_path
            .to_str()
            .ok_or_else(|| engine_error("model path", "not valid UTF-8"))?;
        let create = |gpu: bool| {
            let mut params = WhisperContextParameters::default();
            params.use_gpu(gpu).flash_attn(true);
            WhisperContext::new_with_params(path, params)
        };
        let gpu = if use_gpu { gpu_accelerator() } else { None };
        let (context, accelerator) = match gpu {
            Some(accelerator) => match create(true) {
                Ok(context) => (context, accelerator),
                Err(error) => {
                    tracing::warn!(%error, "GPU speech model failed to load; using the CPU");
                    (
                        create(false).map_err(|e| engine_error("load model", e))?,
                        Accelerator::Cpu,
                    )
                }
            },
            None => (
                create(false).map_err(|e| engine_error("load model", e))?,
                Accelerator::Cpu,
            ),
        };
        let state = context
            .create_state()
            .map_err(|e| engine_error("create decoding state", e))?;
        Ok(Self {
            context,
            state,
            accelerator,
            vad: None,
        })
    }

    /// Loads the voice activity detection model used when [`DecodeOptions::vad`] is on (see
    /// [`crate::ModelStore::vad_model_path`]). The detector runs on the CPU; it is small.
    pub fn with_vad_model(mut self, path: &Path) -> crate::Result<Self> {
        let path_str = path
            .to_str()
            .ok_or_else(|| engine_error("VAD model path", "not valid UTF-8"))?;
        if !path.is_file() {
            return Err(TranscribeError::ModelMissing(path.display().to_string()));
        }
        let mut params = WhisperVadContextParams::default();
        params.set_use_gpu(false);
        let vad = WhisperVadContext::new(path_str, params)
            .map_err(|e| engine_error("load VAD model", e))?;
        self.vad = Some(vad);
        Ok(self)
    }

    /// Speech regions of `samples` in seconds, from the voice activity detector.
    fn detect_speech(
        vad: &mut WhisperVadContext,
        samples: &[f32],
    ) -> crate::Result<Vec<(f64, f64)>> {
        let segments = vad
            .segments_from_samples(vad_params(), samples)
            .map_err(|e| engine_error("voice activity detection", e))?;
        Ok(segments
            .map(|s| (f64::from(s.start) / 100.0, f64::from(s.end) / 100.0))
            .collect())
    }
}

/// whisper.cpp parameters for one call.
fn build_params<'a>(options: &'a DecodeOptions, cancel: &CancelFlag) -> FullParams<'a, 'a> {
    let mut params = FullParams::new(SamplingStrategy::Greedy {
        best_of: options.best_of.max(1) as i32,
    });
    params.set_n_threads(options.threads.max(1) as i32);
    if options.language.is_empty() || options.language == "auto" {
        params.set_language(Some("auto"));
    } else {
        params.set_language(Some(&options.language));
    }
    params.set_translate(false);
    // `no_context` clears text left over from an earlier call; within one call whisper.cpp still
    // prompts each 30-second block with the previous block's text unless the prompt budget is 0.
    params.set_no_context(options.no_context);
    if options.no_context {
        params.set_n_max_text_ctx(0);
    }
    params.set_suppress_nst(options.suppress_non_speech);
    params.set_suppress_blank(true);
    params.set_temperature(options.temperature);
    params.set_temperature_inc(options.temperature_increment);
    params.set_entropy_thold(options.entropy_threshold);
    params.set_logprob_thold(options.logprob_threshold);
    params.set_no_speech_thold(options.no_speech_threshold);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    // whisper-rs 0.16's `set_abort_callback_safe` stores the closure as a
    // `Box<dyn FnMut() -> bool>` but calls it as the closure's own type `F`. Passing a value that
    // already is a `Box<dyn FnMut() -> bool>` makes `F` that type, so the stored and called types
    // agree. (Passing a plain closure reads unrelated memory and aborts at random.) whisper-rs
    // never frees the box: a few dozen bytes per call.
    let flag = cancel.clone();
    let abort: Box<dyn FnMut() -> bool> = Box::new(move || flag.is_cancelled());
    params.set_abort_callback_safe(abort);
    params
}

/// Mean log-probability of a segment's text tokens (special and timestamp tokens excluded).
fn average_logprob(segment: &whisper_rs::WhisperSegment<'_>, eot: i32) -> f32 {
    let mut sum = 0.0f32;
    let mut count = 0u32;
    for i in 0..segment.n_tokens() {
        if let Some(token) = segment.get_token(i) {
            let data = token.token_data();
            if data.id < eot {
                sum += data.plog;
                count += 1;
            }
        }
    }
    if count == 0 { 0.0 } else { sum / count as f32 }
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(
        &mut self,
        samples: &[f32],
        start_s: f64,
        options: &DecodeOptions,
        cancel: &CancelFlag,
    ) -> crate::Result<Vec<Segment>> {
        if cancel.is_cancelled() {
            return Err(TranscribeError::Cancelled);
        }
        if samples.is_empty() {
            return Ok(Vec::new());
        }
        if !(options.language.is_empty()
            || options.language == "auto"
            || whisper_rs::get_lang_id(&options.language).is_some())
        {
            return Err(engine_error("language", &options.language));
        }
        let duration_s = samples.len() as f64 / f64::from(SAMPLE_RATE);
        let window_end = start_s + duration_s;

        // With voice activity detection, decode only the speech and map times back afterwards.
        let layout = match (&mut self.vad, options.vad) {
            (Some(vad), true) => {
                let speech = Self::detect_speech(vad, samples)?;
                let layout = SpeechLayout::new(&speech, duration_s);
                if layout.is_empty() {
                    return Ok(Vec::new());
                }
                Some(layout)
            }
            (None, true) => {
                tracing::warn!("voice activity detection requested without a model");
                None
            }
            _ => None,
        };
        let mut input: Cow<'_, [f32]> = match &layout {
            Some(layout) => Cow::Owned(layout.join(samples, SAMPLE_RATE)),
            None => Cow::Borrowed(samples),
        };
        if input.len() < MIN_SAMPLES {
            let mut padded = input.into_owned();
            padded.resize(MIN_SAMPLES, 0.0);
            input = Cow::Owned(padded);
        }
        let to_file_time = |cs: i64| {
            let t = cs as f64 / 100.0;
            let t = layout.as_ref().map_or(t, |l| l.to_original(t));
            (start_s + t).min(window_end)
        };

        let params = build_params(options, cancel);
        let result = self.state.full(params, &input);
        if cancel.is_cancelled() {
            return Err(TranscribeError::Cancelled);
        }
        result.map_err(|e| engine_error("transcribe", e))?;

        let eot = self.context.token_eot();
        let mut segments = Vec::new();
        for segment in self.state.as_iter() {
            let text = segment
                .to_str_lossy()
                .map_err(|e| engine_error("read segment", e))?
                .trim()
                .to_owned();
            let start = to_file_time(segment.start_timestamp());
            let end = to_file_time(segment.end_timestamp()).max(start);
            segments.push(Segment {
                start_s: start,
                end_s: end,
                text,
                avg_logprob: average_logprob(&segment, eot),
                no_speech_prob: segment.no_speech_probability(),
                filtered: None,
            });
        }
        Ok(segments)
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

    #[test]
    fn default_options_follow_the_owner_decisions() {
        let o = DecodeOptions::default();
        assert_eq!(o.language, "en");
        assert!(o.no_context);
        assert!(o.suppress_non_speech);
        assert_eq!(o.temperature, 0.0);
        assert!(o.temperature_increment > 0.0, "temperature fallback is on");
        assert!(!o.vad);
        assert!((1..=8).contains(&o.threads));
    }

    #[test]
    fn options_for_a_file_choose_vad_by_length_and_music() {
        let p = SamplingPolicy::default();
        assert!(!DecodeOptions::for_file(300.0, false, &p).vad);
        assert!(DecodeOptions::for_file(1300.0, false, &p).vad);
        assert!(!DecodeOptions::for_file(1300.0, true, &p).vad);
    }

    #[test]
    fn loading_a_missing_model_is_model_missing() {
        let err = WhisperTranscriber::load(Path::new("/nonexistent/ggml-none.bin"), false)
            .expect_err("missing file");
        assert!(matches!(err, TranscribeError::ModelMissing(_)), "{err:?}");
    }

    #[test]
    fn loading_a_file_that_is_not_a_model_is_an_engine_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ggml-bogus.bin");
        std::fs::write(&path, b"not a model").unwrap();
        let err = WhisperTranscriber::load(&path, false).expect_err("bogus file");
        assert!(matches!(err, TranscribeError::Engine(_)), "{err:?}");
    }
}
