//! Decoding audio to 16 kHz mono `f32` PCM.

use std::path::Path;

use mi_types::{CancelFlag, SampleWindow};

use crate::Sidecars;

/// Sample rate of all decoded audio, in Hz. whisper.cpp requires 16 kHz.
pub const SAMPLE_RATE: u32 = 16_000;

/// Decoded mono audio.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pcm {
    /// Samples in `-1.0..=1.0`.
    pub samples: Vec<f32>,
    /// Sample rate in Hz (always [`SAMPLE_RATE`] from this crate).
    pub sample_rate: u32,
    /// Time of the first sample, seconds from the start of the file.
    pub start_s: f64,
}

/// What to decode.
#[derive(Debug, Clone, Default)]
pub struct ExtractOptions {
    /// Decode only this range; `None` decodes the whole file.
    pub window: Option<SampleWindow>,
    /// Audio stream to use (absolute index); `None` picks the default stream, preferring the
    /// requested language.
    pub stream_index: Option<u32>,
    /// Preferred language (ISO 639-2) when choosing a stream.
    pub language: Option<String>,
}

/// One block of streamed samples.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioChunk<'a> {
    /// Samples at [`SAMPLE_RATE`].
    pub samples: &'a [f32],
    /// Time of the first sample, seconds from the start of the file.
    pub start_s: f64,
}

/// Decodes audio into memory. Suitable for files and windows up to about 30 minutes (115 MB of
/// samples); use [`stream_audio`] for play-all titles.
///
/// Runs `ffmpeg -nostdin -v error [-ss start -t len] -i <path> -map 0:<stream> -ac 1 -ar 16000
/// -f f32le -` (seeking before `-i` for speed) and reads stdout.
pub fn extract_audio(
    sidecars: &Sidecars,
    path: &Path,
    options: &ExtractOptions,
    cancel: &CancelFlag,
) -> crate::Result<Pcm> {
    let _ = (sidecars, path, options, cancel);
    todo!("media module: decode with ffmpeg to f32le")
}

/// Decodes audio and hands it to `on_chunk` in blocks (about one second each) without holding the
/// whole file in memory. Returning `false` from `on_chunk` stops decoding early (not an error).
pub fn stream_audio(
    sidecars: &Sidecars,
    path: &Path,
    options: &ExtractOptions,
    cancel: &CancelFlag,
    on_chunk: &mut dyn FnMut(AudioChunk<'_>) -> bool,
) -> crate::Result<()> {
    let _ = (sidecars, path, options, cancel, on_chunk);
    todo!("media module: stream f32le from ffmpeg")
}
