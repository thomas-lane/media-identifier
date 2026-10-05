//! Duration fit.

/// How well a file of `file_s` seconds fits an episode listed at `runtime_s`, `0.0..=1.0`.
///
/// Listed runtimes are rounded (often to whole minutes, sometimes to the broadcast slot), so the
/// score is 1.0 within a tolerance and falls off smoothly beyond it. `None` when the runtime is
/// unknown.
pub fn duration_fit(file_s: f64, runtime_s: Option<f64>) -> Option<f32> {
    let _ = (file_s, runtime_s);
    todo!("match module: duration fit")
}
