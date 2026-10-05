//! Choosing which parts of a file to transcribe.

use mi_types::SampleWindow;

/// Sampling rules.
#[derive(Debug, Clone, PartialEq)]
pub struct SamplingPolicy {
    /// Files up to this length (seconds) are transcribed whole. Short musical clips need every
    /// second, and whole-file decoding of a few minutes is fast.
    pub whole_file_max_s: f64,
    /// Length of each window for longer files, seconds (90-120).
    pub window_s: f64,
    /// Window centres as fractions of the runtime. Avoids the first and last minutes, where
    /// opening and closing credits are shared by every episode.
    pub positions: Vec<f64>,
}

impl Default for SamplingPolicy {
    fn default() -> Self {
        Self {
            whole_file_max_s: 360.0,
            window_s: 105.0,
            positions: vec![0.15, 0.40, 0.65, 0.85],
        }
    }
}

/// Initial windows for a file of `duration_s` seconds.
///
/// One window covering the whole file when `duration_s <= whole_file_max_s` or when `sample` is
/// false (the user turned sampling off); otherwise one `window_s` window centred on each
/// position, clamped inside the file and merged where they overlap.
pub fn plan_windows(duration_s: f64, policy: &SamplingPolicy, sample: bool) -> Vec<SampleWindow> {
    let _ = (duration_s, policy, sample);
    todo!("transcribe module: initial sampling windows")
}

/// Extra windows to transcribe when the match margin after `done` windows is too low: windows
/// between the existing ones, or the remaining uncovered parts of the file when `whole_file` is
/// true. Never returns ranges already covered by `done`.
pub fn escalation_windows(
    duration_s: f64,
    done: &[SampleWindow],
    policy: &SamplingPolicy,
    whole_file: bool,
) -> Vec<SampleWindow> {
    let _ = (duration_s, done, policy, whole_file);
    todo!("transcribe module: escalation windows")
}
