//! Tests against the real Fast model, downloaded from Hugging Face. Ignored by default because
//! they download about 190 MB; run them with
//!
//! ```text
//! cargo test -p mi-transcribe --test real_model -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Models are kept in `MI_TEST_MODEL_DIR` (default: `target/tmp/models`) so later runs reuse them.
//! Speech is generated with the macOS `say` command, so these tests run on macOS only.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mi_transcribe::filter::normalize;
use mi_transcribe::{
    DecodeOptions, HallucinationFilter, ModelStore, SAMPLE_RATE, TranscribeError, Transcriber,
    WhisperTranscriber,
};
use mi_types::{Accelerator, CancelFlag, ModelState, ModelStatus, SpeechModel};

const SPEECH: &str = "The lighthouse keeper counted eleven ships before midnight. \
    Then the fog rolled in, and the harbor bell started ringing.";

fn model_dir() -> PathBuf {
    std::env::var_os("MI_TEST_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_TARGET_TMPDIR")).join("models"))
}

fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,whisper_rs=warn".into()),
        )
        .with_test_writer()
        .try_init();
}

/// Downloads the Fast model if needed. A fresh download is cancelled once after 5 MB and then
/// resumed, so the real Hugging Face redirect and `Range` handling are exercised.
async fn fast_model() -> (ModelStore, PathBuf) {
    ensure_model(SpeechModel::Fast).await
}

/// Downloads `model` if needed, as described for [`fast_model`].
async fn ensure_model(model: SpeechModel) -> (ModelStore, PathBuf) {
    let store = ModelStore::new(model_dir());
    if store.status(model).state == ModelState::Ready {
        let path = store.model_path(model);
        return (store, path);
    }
    let fresh = store.status(model).state == ModelState::Missing;
    if fresh {
        let cancel = CancelFlag::new();
        let trigger = cancel.clone();
        let last = Arc::new(Mutex::new(None::<ModelState>));
        let seen = last.clone();
        let on_progress = move |s: ModelStatus| {
            if let ModelState::Downloading { downloaded, .. } = s.state
                && downloaded > 5_000_000
            {
                trigger.cancel();
            }
            *seen.lock().unwrap() = Some(s.state);
        };
        let err = store
            .download(model, &on_progress, &cancel)
            .await
            .expect_err("cancelled");
        assert!(matches!(err, TranscribeError::Cancelled), "{err:?}");
        let paused = last.lock().unwrap().clone();
        assert!(
            matches!(paused, Some(ModelState::Paused { downloaded, .. }) if downloaded > 5_000_000),
            "{paused:?}"
        );
        println!("cancelled after {paused:?}; resuming");
    }
    let started = Instant::now();
    let last = Arc::new(Mutex::new(None::<ModelState>));
    let seen = last.clone();
    let path = store
        .download(
            model,
            &move |s: ModelStatus| *seen.lock().unwrap() = Some(s.state),
            &CancelFlag::new(),
        )
        .await
        .expect("download");
    println!(
        "download finished in {:.1} s (resumed: {fresh})",
        started.elapsed().as_secs_f64()
    );
    assert_eq!(*last.lock().unwrap(), Some(ModelState::Ready));
    assert_eq!(store.status(model).state, ModelState::Ready);
    (store, path)
}

/// Speaks `text` with `say` into a 16 kHz mono WAV and returns the samples.
fn say(text: &str, dir: &Path) -> Vec<f32> {
    let wav = dir.join("speech.wav");
    let status = Command::new("say")
        .args(["-o"])
        .arg(&wav)
        .args(["--data-format=LEI16@16000", text])
        .status()
        .expect("run say");
    assert!(status.success());
    let mut reader = hound::WavReader::open(&wav).unwrap();
    let spec = reader.spec();
    assert_eq!((spec.sample_rate, spec.channels), (SAMPLE_RATE, 1));
    reader
        .samples::<i16>()
        .map(|s| f32::from(s.unwrap()) / 32768.0)
        .collect()
}

fn words(segments: &[mi_types::Segment]) -> String {
    normalize(
        &segments
            .iter()
            .filter(|s| s.filtered.is_none())
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    )
}

#[tokio::test]
#[ignore = "downloads the 190 MB Fast model"]
async fn fast_model_transcribes_generated_speech() {
    init_logging();
    let (store, path) = fast_model().await;
    let scratch = tempfile::tempdir().unwrap();
    let samples = say(SPEECH, scratch.path());
    let audio_s = samples.len() as f64 / f64::from(SAMPLE_RATE);

    let mut transcriber = WhisperTranscriber::load(&path, true)
        .unwrap()
        .with_vad_model(&store.vad_model_path().expect("VAD model"))
        .unwrap();
    assert_eq!(transcriber.accelerator(), Accelerator::AppleGpu);
    let options = DecodeOptions::default();
    let started = Instant::now();
    let mut segments = transcriber
        .transcribe(&samples, 600.0, &options, &CancelFlag::new())
        .unwrap();
    let elapsed = started.elapsed().as_secs_f64();
    HallucinationFilter::default().apply(&mut segments);
    for s in &segments {
        println!(
            "{:7.2}-{:7.2} {:?} logprob {:.2} no-speech {:.3} {:?}",
            s.start_s, s.end_s, s.text, s.avg_logprob, s.no_speech_prob, s.filtered
        );
    }
    println!(
        "{audio_s:.1} s of audio in {elapsed:.2} s ({:.1}x real time, Metal)",
        audio_s / elapsed
    );

    let heard = words(&segments);
    for expected in [
        "lighthouse",
        "keeper",
        "ships",
        "midnight",
        "fog",
        "harbor",
        "bell",
    ] {
        assert!(
            heard.contains(expected),
            "{expected:?} missing from {heard:?}"
        );
    }
    // Whisper writes numbers as digits.
    assert!(
        heard.contains("eleven") || heard.contains("11"),
        "{heard:?}"
    );
    // Times are file times: the window started at 600 s.
    let first = segments.first().unwrap();
    let last = segments.last().unwrap();
    assert!(first.start_s >= 600.0 && first.start_s < 602.0, "{first:?}");
    assert!(last.end_s <= 600.0 + audio_s + 1e-6, "{last:?}");
    assert!(segments.iter().all(|s| s.end_s >= s.start_s));
    assert!(
        segments
            .iter()
            .all(|s| s.avg_logprob < 0.0 && s.avg_logprob > -2.0)
    );
}

#[tokio::test]
#[ignore = "downloads the 190 MB Fast model"]
async fn vad_keeps_file_times_across_long_silences() {
    init_logging();
    let (store, path) = fast_model().await;
    let vad = store
        .vad_model_path()
        .expect("VAD model downloaded with the speech model");
    let scratch = tempfile::tempdir().unwrap();
    let speech = say(SPEECH, scratch.path());
    let silence = vec![0.0f32; 40 * SAMPLE_RATE as usize];
    let samples = [silence.clone(), speech.clone(), silence].concat();

    let mut transcriber = WhisperTranscriber::load(&path, true)
        .unwrap()
        .with_vad_model(&vad)
        .unwrap();
    let options = DecodeOptions {
        vad: true,
        ..DecodeOptions::default()
    };
    let segments = transcriber
        .transcribe(&samples, 100.0, &options, &CancelFlag::new())
        .unwrap();
    for s in &segments {
        println!("{:7.2}-{:7.2} {:?}", s.start_s, s.end_s, s.text);
    }
    let heard = words(&segments);
    assert!(
        heard.contains("lighthouse") && heard.contains("harbor"),
        "{heard:?}"
    );
    let speech_s = speech.len() as f64 / f64::from(SAMPLE_RATE);
    let first = segments.iter().find(|s| !s.text.is_empty()).unwrap();
    assert!(
        (first.start_s - 140.0).abs() < 2.0,
        "speech starts 40 s into a window at 100 s: {first:?}"
    );
    let last = segments.last().unwrap();
    assert!(last.end_s <= 140.0 + speech_s + 2.0, "{last:?}");
    // Nothing is decoded from the silences, so nothing is invented there.
    assert!(segments.iter().all(|s| s.start_s >= 138.0), "{segments:?}");

    // A window without speech gives no segments at all.
    let silent = vec![0.0f32; 60 * SAMPLE_RATE as usize];
    let none = transcriber
        .transcribe(&silent, 0.0, &options, &CancelFlag::new())
        .unwrap();
    assert!(none.is_empty(), "{none:?}");
}

#[tokio::test]
#[ignore = "downloads the 190 MB Fast model"]
async fn cpu_transcription_works() {
    init_logging();
    let (_store, path) = fast_model().await;
    let scratch = tempfile::tempdir().unwrap();
    let samples = say("Eleven ships before midnight.", scratch.path());
    let mut transcriber = WhisperTranscriber::load(&path, false).unwrap();
    assert_eq!(transcriber.accelerator(), Accelerator::Cpu);
    let started = Instant::now();
    let segments = transcriber
        .transcribe(&samples, 0.0, &DecodeOptions::default(), &CancelFlag::new())
        .unwrap();
    println!(
        "CPU: {:?} in {:.2} s",
        words(&segments),
        started.elapsed().as_secs_f64()
    );
    assert!(words(&segments).contains("midnight"));
}

#[tokio::test]
#[ignore = "downloads the 190 MB Fast model"]
async fn cancelling_stops_transcription_quickly() {
    init_logging();
    let (_store, path) = fast_model().await;
    let scratch = tempfile::tempdir().unwrap();
    let speech = say(SPEECH, scratch.path());
    // About five minutes of speech: far longer than the time allowed below.
    let samples: Vec<f32> = speech
        .iter()
        .copied()
        .cycle()
        .take(300 * SAMPLE_RATE as usize)
        .collect();
    let mut transcriber = WhisperTranscriber::load(&path, true).unwrap();
    // Warm up first: the first call compiles GPU kernels, which cannot be interrupted.
    let warm_up = say("Harbor bell.", scratch.path());
    transcriber
        .transcribe(&warm_up, 0.0, &DecodeOptions::default(), &CancelFlag::new())
        .unwrap();
    let cancel = CancelFlag::new();
    let trigger = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        trigger.cancel();
    });
    let started = Instant::now();
    let result = transcriber.transcribe(&samples, 0.0, &DecodeOptions::default(), &cancel);
    let elapsed = started.elapsed();
    println!("stopped after {:.2} s", elapsed.as_secs_f64());
    assert!(
        matches!(result, Err(TranscribeError::Cancelled)),
        "{result:?}"
    );
    // Cancellation is checked between encoder and decoder passes, so it takes effect within one
    // pass; transcribing all five minutes would take far longer.
    assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");

    // The transcriber stays usable after a cancelled call.
    let segments = transcriber
        .transcribe(&warm_up, 0.0, &DecodeOptions::default(), &CancelFlag::new())
        .unwrap();
    assert!(words(&segments).contains("bell"), "{segments:?}");
}

/// whisper-rs 0.16's safe abort callback reads the wrong memory unless it is given a boxed
/// closure (see `engine.rs`); with the wrong type, calls aborted at random with "failed to
/// encode". Several calls in a row with a flag that is never set must all succeed.
#[tokio::test]
#[ignore = "downloads the 190 MB Fast model"]
async fn transcription_is_never_aborted_without_cancellation() {
    init_logging();
    let (_store, path) = fast_model().await;
    let scratch = tempfile::tempdir().unwrap();
    let samples = say("Eleven ships before midnight.", scratch.path());
    for use_gpu in [true, false] {
        let mut transcriber = WhisperTranscriber::load(&path, use_gpu).unwrap();
        for _ in 0..3 {
            let segments = transcriber
                .transcribe(&samples, 0.0, &DecodeOptions::default(), &CancelFlag::new())
                .unwrap();
            assert!(words(&segments).contains("midnight"));
        }
    }
}

#[tokio::test]
#[ignore = "downloads the 547 MiB Accurate model"]
async fn accurate_model_transcribes_generated_speech() {
    init_logging();
    let (_store, path) = ensure_model(SpeechModel::Accurate).await;
    let scratch = tempfile::tempdir().unwrap();
    let samples = say(SPEECH, scratch.path());
    let audio_s = samples.len() as f64 / f64::from(SAMPLE_RATE);
    let mut transcriber = WhisperTranscriber::load(&path, true).unwrap();
    // The first call compiles GPU kernels; time the second.
    transcriber
        .transcribe(&samples, 0.0, &DecodeOptions::default(), &CancelFlag::new())
        .unwrap();
    let started = Instant::now();
    let segments = transcriber
        .transcribe(&samples, 0.0, &DecodeOptions::default(), &CancelFlag::new())
        .unwrap();
    let elapsed = started.elapsed().as_secs_f64();
    for s in &segments {
        println!("{:7.2}-{:7.2} {:?}", s.start_s, s.end_s, s.text);
    }
    println!(
        "Accurate: {audio_s:.1} s of audio in {elapsed:.2} s ({:.1}x real time, Metal)",
        audio_s / elapsed
    );
    let heard = words(&segments);
    assert!(
        heard.contains("lighthouse") && heard.contains("harbor"),
        "{heard:?}"
    );
}
