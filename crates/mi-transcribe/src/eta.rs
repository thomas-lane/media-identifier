//! Estimating how long transcription will take.

use std::time::Duration;

use mi_types::{Accelerator, SampleWindow, SpeechModel};

/// whisper.cpp encodes audio in 30-second blocks and pads a shorter clip to a full block, so a
/// window shorter than this costs about as much as one of this length.
pub const MIN_COST_S: f64 = 30.0;

/// Audio seconds the measured speed is weighted against the starting guess with: the first
/// minute of real work moves the estimate halfway to the measured speed.
const PRIOR_WEIGHT_S: f64 = 60.0;

/// Seconds of work the windows represent: each window's length, but at least [`MIN_COST_S`].
pub fn audio_cost_seconds(windows: &[SampleWindow]) -> f64 {
    windows
        .iter()
        .map(|w| (w.end_s - w.start_s).max(0.0))
        .filter(|len| *len > 0.0)
        .map(|len| len.max(MIN_COST_S))
        .sum()
}

/// Starting guess of transcription speed, in audio seconds per second, before anything has been
/// measured on this computer. Deliberately on the slow side so the first estimate errs long.
pub fn prior_speed(model: SpeechModel, accelerator: Accelerator) -> f64 {
    match (model, accelerator) {
        (SpeechModel::Fast, Accelerator::AppleGpu | Accelerator::Vulkan) => 40.0,
        (SpeechModel::Accurate, Accelerator::AppleGpu | Accelerator::Vulkan) => 15.0,
        (SpeechModel::Fast, Accelerator::Cpu) => 8.0,
        (SpeechModel::Accurate, Accelerator::Cpu) => 2.0,
    }
}

/// Learns the transcription speed on this computer as windows finish and turns remaining work
/// into a time estimate.
///
/// The speed is total audio seconds over total compute seconds, with the starting guess from
/// [`prior_speed`] counted as [`PRIOR_WEIGHT_S`] seconds of audio. Totals rather than an average
/// of per-window speeds are used, so a few short windows do not outweigh long ones.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeedEstimator {
    prior_speed: f64,
    audio_s: f64,
    compute_s: f64,
}

impl SpeedEstimator {
    /// Starts from the guess for this model and processor.
    pub fn new(model: SpeechModel, accelerator: Accelerator) -> Self {
        Self::with_prior(prior_speed(model, accelerator))
    }

    /// Starts from a given speed (audio seconds per second, above zero).
    pub fn with_prior(prior_speed: f64) -> Self {
        Self {
            prior_speed: if prior_speed.is_finite() && prior_speed > 0.0 {
                prior_speed
            } else {
                1.0
            },
            audio_s: 0.0,
            compute_s: 0.0,
        }
    }

    /// Records one finished piece of work: `audio_cost_s` as computed by [`audio_cost_seconds`],
    /// and the time it took. Non-finite or negative values are ignored.
    pub fn record(&mut self, audio_cost_s: f64, elapsed: Duration) {
        let compute = elapsed.as_secs_f64();
        if audio_cost_s.is_finite() && audio_cost_s > 0.0 && compute.is_finite() {
            self.audio_s += audio_cost_s;
            self.compute_s += compute;
        }
    }

    /// Current speed estimate, audio seconds per second.
    pub fn speed(&self) -> f64 {
        (PRIOR_WEIGHT_S + self.audio_s) / (PRIOR_WEIGHT_S / self.prior_speed + self.compute_s)
    }

    /// Estimated seconds to transcribe `audio_cost_s` more seconds of audio.
    pub fn remaining_seconds(&self, audio_cost_s: f64) -> f64 {
        if audio_cost_s.is_finite() && audio_cost_s > 0.0 {
            audio_cost_s / self.speed()
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(start_s: f64, end_s: f64) -> SampleWindow {
        SampleWindow { start_s, end_s }
    }

    #[test]
    fn short_windows_cost_a_full_block() {
        assert_eq!(audio_cost_seconds(&[w(0.0, 10.0)]), 30.0);
        assert_eq!(audio_cost_seconds(&[w(0.0, 105.0), w(200.0, 210.0)]), 135.0);
        assert_eq!(audio_cost_seconds(&[w(5.0, 5.0), w(9.0, 3.0)]), 0.0);
        assert_eq!(audio_cost_seconds(&[]), 0.0);
    }

    #[test]
    fn the_estimate_starts_at_the_prior() {
        let e = SpeedEstimator::with_prior(10.0);
        assert!((e.speed() - 10.0).abs() < 1e-9);
        assert!((e.remaining_seconds(100.0) - 10.0).abs() < 1e-9);
        assert_eq!(e.remaining_seconds(0.0), 0.0);
    }

    #[test]
    fn measurements_move_the_estimate_towards_the_measured_speed() {
        let mut e = SpeedEstimator::with_prior(10.0);
        // One minute of audio at 2 s/s: halfway in the weighted totals.
        e.record(60.0, Duration::from_secs(30));
        let halfway = e.speed();
        assert!(halfway < 10.0 && halfway > 2.0, "{halfway}");
        for _ in 0..100 {
            e.record(60.0, Duration::from_secs(30));
        }
        assert!((e.speed() - 2.0).abs() < 0.1, "{}", e.speed());
    }

    #[test]
    fn invalid_measurements_and_priors_are_ignored() {
        let mut e = SpeedEstimator::with_prior(f64::NAN);
        assert!(e.speed() > 0.0);
        let before = e.clone();
        e.record(-5.0, Duration::from_secs(1));
        e.record(f64::INFINITY, Duration::from_secs(1));
        assert_eq!(e, before);
    }

    #[test]
    fn gpu_priors_are_faster_than_cpu_and_fast_is_faster_than_accurate() {
        for model in [SpeechModel::Fast, SpeechModel::Accurate] {
            assert!(
                prior_speed(model, Accelerator::AppleGpu) > prior_speed(model, Accelerator::Cpu)
            );
        }
        for acc in [Accelerator::AppleGpu, Accelerator::Vulkan, Accelerator::Cpu] {
            assert!(prior_speed(SpeechModel::Fast, acc) > prior_speed(SpeechModel::Accurate, acc));
        }
    }
}
