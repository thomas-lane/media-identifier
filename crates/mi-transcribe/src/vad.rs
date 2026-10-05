//! Voice activity detection: keeping only the speech of a window before decoding it.
//!
//! whisper.cpp applies its own voice activity detection only in `whisper_full`, which works on the
//! context's built-in decoding state; `whisper-rs` decodes through a separate state
//! (`whisper_full_with_state`), where whisper.cpp ignores the setting. So the detector is run
//! here: the speech regions it finds are joined into one shorter buffer, separated by a little
//! silence, the buffer is transcribed in one call, and segment times are mapped back to the
//! original window with a [`SpeechLayout`].

/// Speech regions closer than this (seconds) are joined into one, so a short pause does not
/// split a sentence.
const JOIN_GAP_S: f64 = 0.3;
/// Silence placed between joined regions (seconds), so the model hears a boundary between words
/// that were far apart in the original.
const SEPARATOR_S: f64 = 0.2;

/// One kept region: `len_s` seconds starting at `processed_s` in the joined buffer and at
/// `original_s` in the original samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Region {
    pub processed_s: f64,
    pub original_s: f64,
    pub len_s: f64,
}

/// Where each speech region of a window sits in the joined buffer.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SpeechLayout {
    pub regions: Vec<Region>,
}

impl SpeechLayout {
    /// Builds the layout from speech regions in seconds (any order, may overlap) for a window of
    /// `duration_s` seconds. Regions are clipped to the window, sorted, and joined when less than
    /// [`JOIN_GAP_S`] apart.
    pub fn new(speech: &[(f64, f64)], duration_s: f64) -> Self {
        let mut spans: Vec<(f64, f64)> = speech
            .iter()
            .map(|&(s, e)| (s.max(0.0), e.min(duration_s)))
            .filter(|(s, e)| s.is_finite() && e.is_finite() && e > s)
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut joined: Vec<(f64, f64)> = Vec::new();
        for (s, e) in spans {
            match joined.last_mut() {
                Some(last) if s - last.1 < JOIN_GAP_S => last.1 = last.1.max(e),
                _ => joined.push((s, e)),
            }
        }
        let mut processed = 0.0;
        let regions = joined
            .into_iter()
            .enumerate()
            .map(|(i, (s, e))| {
                if i > 0 {
                    processed += SEPARATOR_S;
                }
                let region = Region {
                    processed_s: processed,
                    original_s: s,
                    len_s: e - s,
                };
                processed += e - s;
                region
            })
            .collect();
        Self { regions }
    }

    /// True when no speech was found.
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// The joined buffer: each region's samples with [`SEPARATOR_S`] of silence between them.
    pub fn join(&self, samples: &[f32], sample_rate: u32) -> Vec<f32> {
        let rate = f64::from(sample_rate);
        let to_index = |t: f64| ((t * rate).round() as usize).min(samples.len());
        let separator = (SEPARATOR_S * rate).round() as usize;
        let mut out = Vec::new();
        for (i, r) in self.regions.iter().enumerate() {
            if i > 0 {
                out.resize(out.len() + separator, 0.0);
            }
            let start = to_index(r.original_s);
            let end = to_index(r.original_s + r.len_s).max(start);
            out.extend_from_slice(&samples[start..end]);
        }
        out
    }

    /// Maps a time in the joined buffer back to the original window. A time inside a separator
    /// maps to the end of the region before it.
    pub fn to_original(&self, t: f64) -> f64 {
        let Some(first) = self.regions.first() else {
            return t;
        };
        let region = self
            .regions
            .iter()
            .rev()
            .find(|r| r.processed_s <= t)
            .unwrap_or(first);
        region.original_s + (t - region.processed_s).clamp(0.0, region.len_s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_are_clipped_sorted_joined_and_separated() {
        let layout = SpeechLayout::new(
            &[(50.0, 60.0), (-1.0, 2.0), (60.1, 61.0), (90.0, 120.0)],
            100.0,
        );
        let expected = [(0.0, 0.0, 2.0), (2.2, 50.0, 11.0), (13.4, 90.0, 10.0)];
        assert_eq!(layout.regions.len(), expected.len(), "{layout:?}");
        for (r, (p, o, l)) in layout.regions.iter().zip(expected) {
            assert!((r.processed_s - p).abs() < 1e-9, "{r:?}");
            assert!((r.original_s - o).abs() < 1e-9, "{r:?}");
            assert!((r.len_s - l).abs() < 1e-9, "{r:?}");
        }
    }

    #[test]
    fn times_map_back_to_the_original_window() {
        let layout = SpeechLayout::new(&[(40.0, 46.5), (70.0, 75.0)], 120.0);
        assert!((layout.to_original(0.0) - 40.0).abs() < 1e-9);
        assert!((layout.to_original(3.0) - 43.0).abs() < 1e-9);
        // Inside the separator: end of the first region.
        assert!((layout.to_original(6.6) - 46.5).abs() < 1e-9);
        assert!((layout.to_original(6.75) - 70.05).abs() < 1e-9);
        assert!((layout.to_original(8.7) - 72.0).abs() < 1e-9);
        // Past the end: end of the last region.
        assert!((layout.to_original(100.0) - 75.0).abs() < 1e-9);
    }

    #[test]
    fn joining_copies_each_region_with_silence_between() {
        let rate = 10;
        let samples: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let layout = SpeechLayout::new(&[(1.0, 2.0), (5.0, 5.5)], 10.0);
        let joined = layout.join(&samples, rate);
        let expected: Vec<f32> = (10..20)
            .map(|i| i as f32)
            .chain([0.0, 0.0])
            .chain((50..55).map(|i| i as f32))
            .collect();
        assert_eq!(joined, expected);
    }

    #[test]
    fn no_speech_gives_an_empty_layout() {
        let layout = SpeechLayout::new(&[], 30.0);
        assert!(layout.is_empty());
        assert!(layout.join(&[0.5; 100], 10).is_empty());
        assert_eq!(layout.to_original(3.0), 3.0);
        assert!(SpeechLayout::new(&[(40.0, 50.0)], 30.0).is_empty());
    }
}
