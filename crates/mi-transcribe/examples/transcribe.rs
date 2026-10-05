//! Transcribes a 16 kHz mono WAV file and prints the segments and the transcription speed.
//!
//! ```text
//! cargo run --release -p mi-transcribe --example transcribe -- <model.bin> <audio.wav> [--cpu] [--vad <vad-model.bin>]
//! ```
//!
//! Make a suitable WAV with `ffmpeg -i <video> -ac 1 -ar 16000 -c:a pcm_s16le out.wav`, or on
//! macOS with `say -o out.wav --data-format=LEI16@16000 "some text"`.

use std::path::PathBuf;
use std::time::Instant;

use mi_transcribe::{
    DecodeOptions, HallucinationFilter, SAMPLE_RATE, Transcriber, WhisperTranscriber,
};
use mi_types::CancelFlag;

fn read_wav(path: &PathBuf) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if spec.sample_rate != SAMPLE_RATE || spec.channels != 1 {
        return Err(format!(
            "need 16 kHz mono, got {} Hz with {} channels",
            spec.sample_rate, spec.channels
        )
        .into());
    }
    Ok(match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()?
        }
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: transcribe <model.bin> <audio.wav> [--cpu] [--vad <vad-model.bin>]";
    let model = PathBuf::from(args.next().ok_or(usage)?);
    let wav = PathBuf::from(args.next().ok_or(usage)?);
    let mut use_gpu = true;
    let mut vad = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cpu" => use_gpu = false,
            "--vad" => vad = Some(PathBuf::from(args.next().ok_or(usage)?)),
            _ => return Err(usage.into()),
        }
    }

    let samples = read_wav(&wav)?;
    let audio_s = samples.len() as f64 / f64::from(SAMPLE_RATE);
    let loaded = Instant::now();
    let mut transcriber = WhisperTranscriber::load(&model, use_gpu)?;
    if let Some(vad) = &vad {
        transcriber = transcriber.with_vad_model(vad)?;
    }
    println!(
        "loaded in {:.2} s on {:?}",
        loaded.elapsed().as_secs_f64(),
        transcriber.accelerator()
    );
    let options = DecodeOptions {
        vad: vad.is_some(),
        ..DecodeOptions::default()
    };
    let started = Instant::now();
    let mut segments = transcriber.transcribe(&samples, 0.0, &options, &CancelFlag::new())?;
    let elapsed = started.elapsed().as_secs_f64();
    HallucinationFilter::default().apply(&mut segments);
    for s in &segments {
        let mark = s.filtered.map(|r| format!(" [{r:?}]")).unwrap_or_default();
        println!(
            "{:8.2} {:8.2}  {}  (logprob {:.2}, no-speech {:.2}){mark}",
            s.start_s, s.end_s, s.text, s.avg_logprob, s.no_speech_prob
        );
    }
    println!(
        "{audio_s:.1} s of audio in {elapsed:.2} s: {:.1}x real time",
        audio_s / elapsed
    );
    Ok(())
}
