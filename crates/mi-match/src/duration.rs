//! Duration fit.

/// Seconds of difference that always count as a perfect fit: listed runtimes are often rounded
/// to whole minutes.
const MIN_TOLERANCE_S: f64 = 60.0;
/// Share of the listed runtime that always counts as a perfect fit.
const RELATIVE_TOLERANCE: f64 = 0.10;
/// Score for a file that fits a broadcast slot with the advertising removed.
const SLOT_SCORE: f32 = 0.9;
/// Shortest share of a broadcast slot an episode without advertising fills (a 30-minute slot
/// holds about 19 to 24 minutes of programme).
const SLOT_MIN_SHARE: f64 = 0.6;

/// How well a file of `file_s` seconds fits an episode listed at `runtime_s`, `0.0..=1.0`.
///
/// Listed runtimes are rounded (often to whole minutes, sometimes to the broadcast slot), so the
/// score is 1.0 within a tolerance and falls off smoothly beyond it. `None` when the runtime is
/// unknown.
///
/// - Within `max(60 s, 10% of the runtime)` the fit is 1.0.
/// - A listed runtime that is a whole number of half hours is often the broadcast slot rather
///   than the programme length, so a file shorter than the slot but at least 60% of it scores
///   0.9.
/// - Otherwise the score falls off as a Gaussian of the difference beyond the tolerance, with a
///   width of 20% of the runtime plus 30 seconds: a 2-minute song listed at 3 minutes still fits
///   reasonably, a 22-minute file listed at 3 minutes does not fit at all.
pub fn duration_fit(file_s: f64, runtime_s: Option<f64>) -> Option<f32> {
    let runtime = runtime_s.filter(|r| r.is_finite() && *r > 0.0)?;
    if !file_s.is_finite() || file_s <= 0.0 {
        return Some(0.0);
    }
    let diff = (file_s - runtime).abs();
    let tolerance = MIN_TOLERANCE_S.max(RELATIVE_TOLERANCE * runtime);
    if diff <= tolerance {
        return Some(1.0);
    }
    let slot_like = runtime >= 1800.0 && (runtime % 1800.0).abs() < 1.0;
    if slot_like && file_s < runtime && file_s >= SLOT_MIN_SHARE * runtime {
        return Some(SLOT_SCORE);
    }
    let width = 0.2 * runtime + 30.0;
    let excess = (diff - tolerance) / width;
    Some((-excess * excess).exp() as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_runtime_is_not_measured() {
        assert_eq!(duration_fit(120.0, None), None);
        assert_eq!(duration_fit(120.0, Some(0.0)), None);
        assert_eq!(duration_fit(120.0, Some(f64::NAN)), None);
    }

    #[test]
    fn rounded_runtimes_fit_perfectly() {
        assert_eq!(duration_fit(175.0, Some(180.0)), Some(1.0));
        assert_eq!(duration_fit(1320.0, Some(1380.0)), Some(1.0));
        assert_eq!(duration_fit(2700.0, Some(2520.0)), Some(1.0));
    }

    #[test]
    fn broadcast_slots_allow_removed_advertising() {
        assert_eq!(duration_fit(22.0 * 60.0, Some(1800.0)), Some(SLOT_SCORE));
        assert_eq!(duration_fit(42.0 * 60.0, Some(3600.0)), Some(SLOT_SCORE));
        // Not a slot: 25 minutes listed, 17 minutes found.
        assert!(duration_fit(17.0 * 60.0, Some(1500.0)).unwrap() < SLOT_SCORE);
    }

    #[test]
    fn falls_off_with_distance() {
        let near = duration_fit(150.0, Some(240.0)).unwrap();
        let far = duration_fit(22.0 * 60.0, Some(180.0)).unwrap();
        assert!(near > 0.8 && near < 1.0, "{near}");
        assert!(far < 0.01, "{far}");
        let a = duration_fit(1000.0, Some(1500.0)).unwrap();
        let b = duration_fit(800.0, Some(1500.0)).unwrap();
        assert!(a > b);
    }

    #[test]
    fn broken_file_durations_score_zero() {
        assert_eq!(duration_fit(0.0, Some(180.0)), Some(0.0));
        assert_eq!(duration_fit(f64::NAN, Some(180.0)), Some(0.0));
    }
}
