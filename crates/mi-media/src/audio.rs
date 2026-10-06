//! Decoding audio to 16 kHz mono `f32` PCM.

use std::ffi::OsString;
use std::io::Read;
use std::path::Path;

use mi_types::{AudioStream, CancelFlag, Probe, SampleWindow};

use crate::{MediaError, Sidecars, Tool};

/// Sample rate of all decoded audio, in Hz. whisper.cpp requires 16 kHz.
pub const SAMPLE_RATE: u32 = 16_000;

/// Samples per streamed chunk (one second).
const CHUNK_SAMPLES: usize = SAMPLE_RATE as usize;

/// How often a waiting decoder loop checks the cancel flag.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(25);

/// Decoded mono audio.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pcm {
    /// Samples, nominally in `-1.0..=1.0`.
    pub samples: Vec<f32>,
    /// Sample rate in Hz (always [`SAMPLE_RATE`] from this crate).
    pub sample_rate: u32,
    /// Time of the first sample, seconds from the start of the file.
    pub start_s: f64,
}

impl Pcm {
    /// Duration of the decoded audio in seconds.
    pub fn duration_s(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.samples.len() as f64 / f64::from(self.sample_rate)
        }
    }
}

/// What to decode.
#[derive(Debug, Clone, Default)]
pub struct ExtractOptions {
    /// Decode only this range; `None` decodes the whole file.
    pub window: Option<SampleWindow>,
    /// Audio stream to use (absolute index, [`AudioStream::index`]). `None` probes the file and
    /// picks one with [`choose_audio_stream`]; callers that already hold a probe should pass the
    /// index to save that extra ffprobe run.
    pub stream_index: Option<u32>,
    /// Preferred language when choosing a stream: ISO 639-1 (`en`) or ISO 639-2 (`eng`).
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

/// Chooses the audio stream to listen to: streams in the preferred language first, then the
/// container's default stream, then the lowest index. Returns `None` when there is no audio.
///
/// Language is compared after mapping ISO 639-1 codes to their ISO 639-2 forms (`en` → `eng`;
/// both `fre`/`fra` style codes are accepted), because the app's settings use two-letter codes and
/// containers use three-letter ones.
pub fn choose_audio_stream(probe: &Probe, language: Option<&str>) -> Option<u32> {
    probe
        .audio_streams
        .iter()
        .min_by_key(|s: &&AudioStream| {
            let language_match = match (language, s.language.as_deref()) {
                (Some(want), Some(have)) => languages_match(want, have),
                _ => false,
            };
            (!language_match, !s.is_default, s.index)
        })
        .map(|s| s.index)
}

/// Whether two language codes (ISO 639-1 or 639-2, any case) name the same language.
pub fn languages_match(a: &str, b: &str) -> bool {
    let a = a.trim().to_ascii_lowercase();
    let b = b.trim().to_ascii_lowercase();
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || iso639_2(&a).contains(&b.as_str()) || iso639_2(&b).contains(&a.as_str())
}

/// ISO 639-2 codes (bibliographic and terminology) for common ISO 639-1 codes.
fn iso639_2(code: &str) -> &'static [&'static str] {
    match code {
        "en" => &["eng"],
        "fr" => &["fre", "fra"],
        "de" => &["ger", "deu"],
        "es" => &["spa"],
        "it" => &["ita"],
        "pt" => &["por"],
        "nl" => &["dut", "nld"],
        "sv" => &["swe"],
        "no" => &["nor", "nob", "nno"],
        "da" => &["dan"],
        "fi" => &["fin"],
        "pl" => &["pol"],
        "ru" => &["rus"],
        "ja" => &["jpn"],
        "zh" => &["chi", "zho"],
        "ko" => &["kor"],
        _ => &[],
    }
}

/// Decodes audio into memory. Suitable for files and windows up to about 30 minutes (115 MB of
/// samples); use [`stream_audio`] for play-all titles.
///
/// The returned [`Pcm::start_s`] is the window's start (0 for a whole file). A window beginning
/// after the end of the file yields no samples rather than an error.
pub fn extract_audio(
    sidecars: &Sidecars,
    path: &Path,
    options: &ExtractOptions,
    cancel: &CancelFlag,
) -> crate::Result<Pcm> {
    let mut samples = Vec::new();
    stream_audio(sidecars, path, options, cancel, &mut |chunk| {
        samples.extend_from_slice(chunk.samples);
        true
    })?;
    Ok(Pcm {
        samples,
        sample_rate: SAMPLE_RATE,
        start_s: options.window.map_or(0.0, |w| w.start_s),
    })
}

/// Decodes audio and hands it to `on_chunk` in blocks of one second (the last block may be
/// shorter) without holding the whole file in memory. Returning `false` from `on_chunk` stops
/// decoding early and kills ffmpeg (not an error).
///
/// Runs `ffmpeg -nostdin -hide_banner -v error [-ss <start> -t <length>] -i <path>
/// -map 0:<stream> -vn -sn -dn -ac 1 -ar 16000 -c:a pcm_f32le -f f32le -`. The seek is placed
/// before `-i` so ffmpeg jumps to the window through the container index instead of decoding
/// everything before it; ffmpeg still trims to the exact start time after the jump. `-ac 1`
/// downmixes every channel (including surround) to mono.
///
/// Errors: `NoAudio` when the file has no audio stream, `ToolFailed` when ffmpeg fails,
/// `Cancelled` when `cancel` is set (ffmpeg is killed).
pub fn stream_audio(
    sidecars: &Sidecars,
    path: &Path,
    options: &ExtractOptions,
    cancel: &CancelFlag,
    on_chunk: &mut dyn FnMut(AudioChunk<'_>) -> bool,
) -> crate::Result<()> {
    crate::check_cancel(cancel)?;
    if let Some(w) = options.window
        && w.end_s <= w.start_s
    {
        return Ok(());
    }
    let stream_index = match options.stream_index {
        Some(index) => index,
        None => {
            let probe = crate::probe(sidecars, path, cancel)?;
            choose_audio_stream(&probe, options.language.as_deref())
                .ok_or_else(|| MediaError::NoAudio(path.to_path_buf()))?
        }
    };
    let args = decode_args(path, options.window, stream_index);
    let mut running = crate::run::spawn(sidecars, Tool::Ffmpeg, path, args)?;
    let mut stdout = running.stdout();

    // stdout is read on a helper thread and handed over through a small channel, so this thread
    // can notice cancellation even while ffmpeg is stalled on a slow network share.
    let (sender, receiver) = std::sync::mpsc::sync_channel::<std::io::Result<Vec<u8>>>(4);
    let reader = std::thread::spawn(move || {
        loop {
            let mut block = vec![0u8; CHUNK_SAMPLES * 4];
            let mut filled = 0;
            let result = loop {
                match stdout.read(&mut block[filled..]) {
                    Ok(0) => break Ok(true),
                    Ok(n) => {
                        filled += n;
                        if filled == block.len() {
                            break Ok(false);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => break Err(e),
                }
            };
            block.truncate(filled);
            let at_end = match result {
                Ok(at_end) => at_end,
                Err(e) => {
                    let _ = sender.send(Err(e));
                    return;
                }
            };
            if (!block.is_empty() && sender.send(Ok(block)).is_err()) || at_end {
                return;
            }
        }
    });

    let start_s = options.window.map_or(0.0, |w| w.start_s);
    let mut pending: Vec<u8> = Vec::new();
    let mut samples = Vec::with_capacity(CHUNK_SAMPLES);
    let mut emitted: u64 = 0;
    loop {
        if cancel.is_cancelled() {
            running.kill();
            return Err(MediaError::Cancelled);
        }
        let block = match receiver.recv_timeout(POLL_INTERVAL) {
            Ok(block) => block?,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        pending.extend_from_slice(&block);
        let whole = pending.len() - pending.len() % 4;
        samples.clear();
        samples.extend(
            pending[..whole]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b)),
        );
        pending.drain(..whole);
        if samples.is_empty() {
            continue;
        }
        let chunk = AudioChunk {
            samples: &samples,
            start_s: start_s + emitted as f64 / f64::from(SAMPLE_RATE),
        };
        emitted += samples.len() as u64;
        if !on_chunk(chunk) {
            running.kill();
            return Ok(());
        }
    }
    let _ = reader.join();
    running.finish(cancel)
}

fn decode_args(path: &Path, window: Option<SampleWindow>, stream_index: u32) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-nostdin", "-hide_banner", "-v", "error"]
        .iter()
        .map(Into::into)
        .collect();
    if let Some(w) = window {
        args.push("-ss".into());
        args.push(format!("{:.3}", w.start_s.max(0.0)).into());
        args.push("-t".into());
        args.push(format!("{:.3}", w.end_s - w.start_s.max(0.0)).into());
    }
    args.push("-i".into());
    args.push(path.as_os_str().to_owned());
    for a in [
        "-map".to_owned(),
        format!("0:{stream_index}"),
        "-vn".into(),
        "-sn".into(),
        "-dn".into(),
        "-ac".into(),
        "1".into(),
        "-ar".into(),
        SAMPLE_RATE.to_string(),
        "-c:a".into(),
        "pcm_f32le".into(),
        "-f".into(),
        "f32le".into(),
        "-".into(),
    ] {
        args.push(a.into());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(index: u32, language: Option<&str>, is_default: bool) -> AudioStream {
        AudioStream {
            index,
            codec: "ac3".into(),
            channels: 2,
            sample_rate: 48_000,
            language: language.map(str::to_owned),
            is_default,
        }
    }

    fn probe(streams: Vec<AudioStream>) -> Probe {
        Probe {
            duration_s: 10.0,
            container: "matroska,webm".into(),
            video: None,
            audio_streams: streams,
            subtitle_streams: Vec::new(),
            chapters: Vec::new(),
        }
    }

    #[test]
    fn prefers_language_then_default_then_index() {
        let p = probe(vec![
            stream(1, Some("spa"), true),
            stream(2, Some("eng"), false),
            stream(3, Some("eng"), true),
        ]);
        assert_eq!(choose_audio_stream(&p, Some("en")), Some(3));
        assert_eq!(choose_audio_stream(&p, Some("fra")), Some(1));
        assert_eq!(choose_audio_stream(&p, None), Some(1));
        let no_default = probe(vec![stream(4, None, false), stream(2, None, false)]);
        assert_eq!(choose_audio_stream(&no_default, Some("en")), Some(2));
        assert_eq!(choose_audio_stream(&probe(Vec::new()), None), None);
    }

    #[test]
    fn language_codes_match_across_iso_639_parts() {
        assert!(languages_match("en", "eng"));
        assert!(languages_match("ENG", "en"));
        assert!(languages_match("fr", "fra"));
        assert!(languages_match("fre", "fr"));
        assert!(!languages_match("en", "spa"));
        assert!(!languages_match("", ""));
    }

    #[test]
    fn seeks_before_the_input_for_windows() {
        let args: Vec<String> = decode_args(
            Path::new("/d/t.mkv"),
            Some(SampleWindow {
                start_s: 90.0,
                end_s: 195.5,
            }),
            2,
        )
        .into_iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
        let pos = |s: &str| args.iter().position(|a| a == s).unwrap();
        assert!(pos("-ss") < pos("-i"));
        assert_eq!(args[pos("-ss") + 1], "90.000");
        assert_eq!(args[pos("-t") + 1], "105.500");
        assert_eq!(args[pos("-map") + 1], "0:2");
        assert_eq!(args.last().unwrap(), "-");

        let whole: Vec<String> = decode_args(Path::new("/d/t.mkv"), None, 1)
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(!whole.iter().any(|a| a == "-ss" || a == "-t"));
    }
}
