//! Locating short files inside the play-all by audio, and deriving disc order.
//!
//! A play-all title contains the disc's short titles back to back, usually as the very same
//! audio. Finding where each short file sits inside it gives the order of the files on the disc.
//!
//! **Fingerprint.** Audio is cut into overlapping frames of 128 ms every 32 ms. Each frame's
//! spectrum is summed into 24 bands spaced evenly in pitch (logarithmically) from 150 Hz to
//! 4 kHz, and each band's energy is taken as a logarithm. A fingerprint frame holds, per band,
//! how much that log energy changed since the previous frame. Logarithms turn a volume change
//! into a constant that the change cancels, and an encoder's tone colouring (a fixed loss of
//! treble, say) is a constant per band that cancels the same way. Steady sounds give changes
//! near zero, so what is compared is where sounds start, stop and move: the part of the audio
//! that survives re-encoding.
//!
//! **Alignment.** A short file is slid along the play-all and, at each offset, the fingerprints
//! are compared by normalised correlation (the cosine between the two sequences of changes):
//! about 1 for the same audio, about 0 for unrelated audio. A cheap first pass compares a sample
//! of 64 positions of the file, each summed over four frames, at every second offset; the eight
//! best offsets are then compared in full, frame by frame, a few frames either side. The best
//! correlation is the alignment's strength.

use std::sync::Arc;

use mi_types::{Chapter, FileId, PlayAllPosition};
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

/// Frame length in seconds.
const FRAME_SECONDS: f64 = 0.128;
/// Frames start every quarter frame.
const HOPS_PER_FRAME: usize = 4;
/// Number of bands per fingerprint frame.
pub const FINGERPRINT_BANDS: usize = 24;
/// Lowest band edge, Hz.
const LOW_HZ: f64 = 150.0;
/// Highest band edge, Hz (lowered for sample rates whose Nyquist frequency is below it).
const HIGH_HZ: f64 = 4000.0;
/// Log-energy changes are stored in steps of 1/16 neper (about 0.54 dB).
const STEPS_PER_NEPER: f32 = 16.0;
/// Band energies below this share of a full-scale tone's (90 dB down) count as silence, so
/// near-silent hiss, which differs between encodes, does not look like change.
const SILENCE_FLOOR: f32 = 1e-9;
/// Bands more than 50 dB below the frame's loudest band count as that level: their energy is
/// mostly leakage and codec noise, which differ between encodes. Being relative, this floor
/// moves with the volume, so a volume change still cancels.
const RELATIVE_FLOOR: f32 = 1e-5;
/// Shortest needle that is aligned, in frames (about 3 seconds).
const MIN_NEEDLE_FRAMES: usize = 94;
/// Frames summed into one position of the first pass.
const POOL: usize = 4;
/// Positions of the needle compared in the first pass.
const COARSE_SAMPLES: usize = 64;
/// Offsets skipped between first-pass comparisons.
const COARSE_STEP: usize = 2;
/// Offsets refined after the first pass.
const COARSE_CANDIDATES: usize = 8;
/// Frames searched either side of each candidate offset in the full comparison.
const REFINE_RADIUS: isize = 4;
/// Share of a needle allowed to run past either end of the play-all (an encoder may trim or pad
/// a little at title boundaries).
const MAX_OVERHANG: f64 = 0.1;

/// Alignment strength below which a file counts as not found in the play-all. Unrelated audio
/// scores near 0 and the same audio in different encodes well above 0.5.
pub const MIN_ALIGNMENT_SCORE: f32 = 0.35;

/// One fingerprint frame: per band, the change in log energy since the previous frame, in steps
/// of 1/16 neper.
pub type FingerprintFrame = [i8; FINGERPRINT_BANDS];

/// A compact audio fingerprint computed from decoded PCM in Rust (no external tool).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fingerprint {
    /// One frame per hop (see the module documentation).
    pub frames: Vec<FingerprintFrame>,
    /// Seconds per frame (the hop between frames).
    pub frame_s: f64,
}

impl Fingerprint {
    /// Computes the fingerprint of mono PCM at `sample_rate` Hz.
    pub fn from_pcm(samples: &[f32], sample_rate: u32) -> Self {
        let mut builder = FingerprintBuilder::new(sample_rate);
        builder.push(samples);
        builder.finish()
    }

    /// Duration covered, seconds.
    pub fn duration_s(&self) -> f64 {
        self.frames.len() as f64 * self.frame_s
    }
}

/// Computes a [`Fingerprint`] from PCM delivered in chunks, so a two-hour play-all never has to
/// be held in memory as samples. Pushing the same samples in any chunk sizes gives the same
/// fingerprint as [`Fingerprint::from_pcm`].
pub struct FingerprintBuilder {
    frame_len: usize,
    hop: usize,
    frame_s: f64,
    floor: f32,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    bands: Vec<std::ops::Range<usize>>,
    pending: Vec<f32>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    previous: Option<[f32; FINGERPRINT_BANDS]>,
    frames: Vec<FingerprintFrame>,
}

impl std::fmt::Debug for FingerprintBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FingerprintBuilder")
            .field("frame_len", &self.frame_len)
            .field("hop", &self.hop)
            .field("frames", &self.frames.len())
            .finish_non_exhaustive()
    }
}

impl FingerprintBuilder {
    /// A builder for mono PCM at `sample_rate` Hz (16 000 in the app).
    pub fn new(sample_rate: u32) -> Self {
        let rate = f64::from(sample_rate.max(1000));
        let frame_len = ((rate * FRAME_SECONDS).round() as usize).max(64);
        let hop = frame_len / HOPS_PER_FRAME;
        let fft = FftPlanner::new().plan_fft_forward(frame_len);
        let window = (0..frame_len)
            .map(|i| {
                let x = std::f64::consts::TAU * i as f64 / frame_len as f64;
                (0.5 - 0.5 * x.cos()) as f32
            })
            .collect();
        let high = HIGH_HZ.min(rate * 0.45);
        let bin_of = |hz: f64| (hz * frame_len as f64 / rate).round() as usize;
        let n = FINGERPRINT_BANDS as f64;
        let bands = (0..FINGERPRINT_BANDS)
            .map(|b| {
                let lo = bin_of(LOW_HZ * (high / LOW_HZ).powf(b as f64 / n)).max(1);
                let hi = bin_of(LOW_HZ * (high / LOW_HZ).powf((b + 1) as f64 / n)).max(lo + 1);
                lo..hi
            })
            .collect();
        let scratch_len = fft.get_inplace_scratch_len();
        // A full-scale sine's peak bin under a Hann window has magnitude frame_len / 4.
        let full_scale = (frame_len as f32 / 4.0).powi(2);
        Self {
            frame_len,
            hop,
            frame_s: hop as f64 / rate,
            floor: full_scale * SILENCE_FLOOR,
            fft,
            window,
            bands,
            pending: Vec::new(),
            buffer: vec![Complex::default(); frame_len],
            scratch: vec![Complex::default(); scratch_len],
            previous: None,
            frames: Vec::new(),
        }
    }

    /// Adds samples.
    pub fn push(&mut self, samples: &[f32]) {
        self.pending.extend_from_slice(samples);
        let mut start = 0;
        while start + self.frame_len <= self.pending.len() {
            let energies = self.log_energies(start);
            if let Some(prev) = &self.previous {
                let mut frame = [0i8; FINGERPRINT_BANDS];
                for (b, v) in frame.iter_mut().enumerate() {
                    *v = ((energies[b] - prev[b]) * STEPS_PER_NEPER)
                        .round()
                        .clamp(-127.0, 127.0) as i8;
                }
                self.frames.push(frame);
            }
            self.previous = Some(energies);
            start += self.hop;
        }
        self.pending.drain(..start);
    }

    fn log_energies(&mut self, start: usize) -> [f32; FINGERPRINT_BANDS] {
        for (i, c) in self.buffer.iter_mut().enumerate() {
            *c = Complex::new(self.pending[start + i] * self.window[i], 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        let mut energies = [0f32; FINGERPRINT_BANDS];
        for (o, r) in energies.iter_mut().zip(&self.bands) {
            *o = self.buffer[r.clone()].iter().map(Complex::norm_sqr).sum();
        }
        let loudest = energies.iter().copied().fold(0.0f32, f32::max);
        let floor = self.floor.max(loudest * RELATIVE_FLOOR);
        energies.map(|e| e.max(floor).ln())
    }

    /// Finishes and returns the fingerprint. Samples that do not fill a whole frame are dropped.
    pub fn finish(self) -> Fingerprint {
        Fingerprint {
            frames: self.frames,
            frame_s: self.frame_s,
        }
    }
}

/// Where a short file was found inside the play-all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Alignment {
    /// Offset of the file's start in the play-all, seconds.
    pub offset_s: f64,
    /// Length of the located range, seconds.
    pub length_s: f64,
    /// Strength in `0.0..=1.0`; below [`MIN_ALIGNMENT_SCORE`] the file counts as not found.
    pub score: f32,
}

/// A sequence of per-band values compared by normalised correlation.
type Frames = [[i32; FINGERPRINT_BANDS]];

/// Normalised correlation of `needle` frames `indices` with `haystack` frames at `offset`, and
/// how many frames were inside the haystack.
fn correlation(needle: &Frames, haystack: &Frames, offset: isize, indices: &[usize]) -> (f32, u32) {
    let (mut dot, mut nn, mut hh, mut count) = (0i64, 0i64, 0i64, 0u32);
    for &j in indices {
        let p = offset + j as isize;
        if p < 0 || p as usize >= haystack.len() {
            continue;
        }
        let (a, b) = (&needle[j], &haystack[p as usize]);
        for k in 0..FINGERPRINT_BANDS {
            let (x, y) = (i64::from(a[k]), i64::from(b[k]));
            dot += x * y;
            nn += x * x;
            hh += y * y;
        }
        count += 1;
    }
    let norm = ((nn as f64) * (hh as f64)).sqrt();
    let r = if norm > 0.0 { dot as f64 / norm } else { 0.0 };
    (r as f32, count)
}

fn widen(frames: &[FingerprintFrame]) -> Vec<[i32; FINGERPRINT_BANDS]> {
    frames.iter().map(|f| f.map(i32::from)).collect()
}

/// Sums of [`POOL`] consecutive frames: the change over 128 ms instead of 32 ms, which varies
/// slowly enough to be compared at every second offset.
fn pooled(frames: &[[i32; FINGERPRINT_BANDS]]) -> Vec<[i32; FINGERPRINT_BANDS]> {
    if frames.len() < POOL {
        return Vec::new();
    }
    (0..=frames.len() - POOL)
        .map(|t| {
            let mut sum = [0i32; FINGERPRINT_BANDS];
            for f in &frames[t..t + POOL] {
                for (s, v) in sum.iter_mut().zip(f) {
                    *s += v;
                }
            }
            sum
        })
        .collect()
}

/// Finds `needle` (a short file) inside `haystack` (the play-all): the offset with the highest
/// correlation. `None` when the needle is shorter than about 3 seconds, longer than the haystack,
/// or the two fingerprints use different frame rates.
pub fn locate(needle: &Fingerprint, haystack: &Fingerprint) -> Option<Alignment> {
    let (n, h) = (needle.frames.len(), haystack.frames.len());
    if n < MIN_NEEDLE_FRAMES || (needle.frame_s - haystack.frame_s).abs() > 1e-9 {
        return None;
    }
    let overhang = (n as f64 * MAX_OVERHANG) as isize;
    let first = -overhang;
    let last = h as isize - n as isize + overhang;
    if last < first {
        return None;
    }
    let needle_frames = widen(&needle.frames);
    let hay = widen(&haystack.frames);

    // First pass: pooled frames at a sample of needle positions, every second offset.
    let needle_pooled = pooled(&needle_frames);
    let hay_pooled = pooled(&hay);
    let samples = COARSE_SAMPLES.min(needle_pooled.len());
    let sampled: Vec<usize> = (0..samples)
        .map(|k| k * needle_pooled.len() / samples)
        .collect();
    let mut coarse: Vec<(f32, isize)> = (first..=last)
        .step_by(COARSE_STEP)
        .filter_map(|o| {
            let (r, c) = correlation(&needle_pooled, &hay_pooled, o, &sampled);
            (c as usize * 2 >= samples).then_some((r, o))
        })
        .collect();
    coarse.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut candidates: Vec<isize> = Vec::new();
    for (_, o) in coarse {
        if candidates.len() == COARSE_CANDIDATES {
            break;
        }
        if candidates.iter().all(|c| (c - o).abs() > 2 * REFINE_RADIUS) {
            candidates.push(o);
        }
    }

    // Second pass: every needle frame around each candidate.
    let all: Vec<usize> = (0..n).collect();
    let min_overlap = (n as f64 * (1.0 - MAX_OVERHANG)) as u32;
    let mut best: Option<(f32, isize)> = None;
    for c in candidates {
        for o in (c - REFINE_RADIUS).max(first)..=(c + REFINE_RADIUS).min(last) {
            let (r, count) = correlation(&needle_frames, &hay, o, &all);
            if count < min_overlap {
                continue;
            }
            if best.is_none_or(|(b, bo)| r > b || (r == b && o < bo)) {
                best = Some((r, o));
            }
        }
    }
    let (r, offset) = best?;
    Some(Alignment {
        offset_s: (offset.max(0) as f64) * haystack.frame_s,
        length_s: n as f64 * needle.frame_s,
        score: r.clamp(0.0, 1.0),
    })
}

/// Why a disc order is not used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscOrderProblem {
    /// Fewer than two files, or fewer than half of the files, were found in the play-all.
    TooFewLocated,
    /// Two files were found in partly overlapping ranges of the play-all, which a play-all made
    /// of back-to-back titles cannot produce.
    Overlapping,
    /// Fewer than half of the located files start at a chapter of the play-all, so the located
    /// ranges do not line up with the disc's own structure.
    OffChapters,
    /// The play-all's order contradicts the files that the dialogue identifies confidently (set
    /// by matching, which knows the episodes).
    ShuffledAgainstContent,
}

/// The disc order derived from alignments.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DiscOrder {
    /// Located files with their positions, sorted by start time.
    pub positions: Vec<(FileId, PlayAllPosition)>,
    /// False when the order is unusable: located ranges overlap, too few files were found, or the
    /// play-all appears shuffled relative to its chapters. An untrustworthy order is ignored, and
    /// files get an `EvidenceNote::PlayAllIgnored`.
    pub trustworthy: bool,
    /// Why the order is not trustworthy; `None` when it is.
    pub problem: Option<DiscOrderProblem>,
}

/// Overlap of two located ranges, seconds, beyond which they count as overlapping: titles in a
/// play-all touch, and alignment is accurate to a frame or two.
const OVERLAP_TOLERANCE_S: f64 = 2.0;
/// Two ranges that share more than this share of the shorter one hold the same audio: the files
/// are duplicates of one title, which says nothing against the order.
const DUPLICATE_SHARE: f64 = 0.5;
/// Distance from a chapter start, seconds, within which a file counts as starting at it.
const CHAPTER_TOLERANCE_S: f64 = 3.0;

/// Builds the disc order from each file's alignment and the play-all's chapters (used to report
/// the chapter number of each position).
///
/// Files whose alignment is missing or weaker than [`MIN_ALIGNMENT_SCORE`] are not located. When
/// two files land on the same range (one holds more than half of the other), they are
/// duplicates of one title: the stronger alignment keeps the position and the other file counts
/// as not located. The order is trustworthy when all of the following hold:
///
/// 1. at least two files, and at least half of the files given, are located;
/// 2. no two located ranges overlap by more than 2 seconds (beyond duplicates);
/// 3. when the play-all has two or more chapters, at least half of the located files start
///    within `max(3 s, 2% of the file's length)` of a chapter start.
pub fn derive_disc_order(
    alignments: &[(FileId, Option<Alignment>)],
    chapters: &[Chapter],
) -> DiscOrder {
    let mut located: Vec<(FileId, Alignment)> = alignments
        .iter()
        .filter_map(|(id, a)| {
            a.filter(|a| a.score >= MIN_ALIGNMENT_SCORE && a.score.is_finite())
                .map(|a| (id.clone(), a))
        })
        .collect();
    // Strongest first, so duplicates keep the better alignment.
    located.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then(a.0.cmp(&b.0)));
    let mut kept: Vec<(FileId, Alignment)> = Vec::new();
    let mut overlapping = false;
    for (id, a) in located {
        let mut duplicate = false;
        for (_, k) in &kept {
            let shared =
                (a.offset_s + a.length_s).min(k.offset_s + k.length_s) - a.offset_s.max(k.offset_s);
            if shared > OVERLAP_TOLERANCE_S {
                if shared > DUPLICATE_SHARE * a.length_s.min(k.length_s) {
                    duplicate = true;
                } else {
                    overlapping = true;
                }
            }
        }
        if !duplicate {
            kept.push((id, a));
        }
    }
    kept.sort_by(|a, b| a.1.offset_s.total_cmp(&b.1.offset_s).then(a.0.cmp(&b.0)));

    let positions: Vec<(FileId, PlayAllPosition)> = kept
        .iter()
        .enumerate()
        .map(|(rank, (id, a))| {
            let probe = a.offset_s + (a.length_s / 2.0).min(1.0);
            let chapter = chapters
                .iter()
                .find(|c| c.start_s <= probe && probe < c.end_s)
                .map(|c| c.index);
            (
                id.clone(),
                PlayAllPosition {
                    chapter,
                    start_s: a.offset_s,
                    end_s: a.offset_s + a.length_s,
                    order_index: rank as u32,
                    alignment_score: a.score,
                },
            )
        })
        .collect();

    let problem = if kept.len() < 2 || kept.len() * 2 < alignments.len() {
        Some(DiscOrderProblem::TooFewLocated)
    } else if overlapping {
        Some(DiscOrderProblem::Overlapping)
    } else if chapters.len() >= 2 {
        let at_chapter = kept
            .iter()
            .filter(|(_, a)| {
                let tolerance = CHAPTER_TOLERANCE_S.max(0.02 * a.length_s);
                chapters
                    .iter()
                    .any(|c| (c.start_s - a.offset_s).abs() <= tolerance)
            })
            .count();
        (at_chapter * 2 < kept.len()).then_some(DiscOrderProblem::OffChapters)
    } else {
        None
    };
    DiscOrder {
        positions,
        trustworthy: problem.is_none(),
        problem,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Rng;

    const RATE: u32 = 16_000;

    /// A synthetic "song": random tones, chords and noise bursts, different for every seed.
    fn song(seed: u64, seconds: f64) -> Vec<f32> {
        let mut rng = Rng::new(seed);
        let total = (seconds * f64::from(RATE)) as usize;
        let mut out = Vec::with_capacity(total);
        while out.len() < total {
            let len = ((0.08 + rng.next_f64() * 0.4) * f64::from(RATE)) as usize;
            let freqs: Vec<f64> = (0..1 + rng.below(3))
                .map(|_| 200.0 + rng.next_f64() * 3000.0)
                .collect();
            let noise = rng.next_f64() < 0.15;
            let amp = 0.1 + rng.next_f64() * 0.5;
            for i in 0..len {
                let t = (out.len() + i) as f64 / f64::from(RATE);
                let v = if noise {
                    rng.next_f64() * 2.0 - 1.0
                } else {
                    freqs
                        .iter()
                        .map(|f| (std::f64::consts::TAU * f * t).sin())
                        .sum::<f64>()
                        / freqs.len() as f64
                };
                out.push((v * amp) as f32);
            }
        }
        out.truncate(total);
        out
    }

    /// The same audio as another encode would deliver it: quieter, low-passed, with added noise.
    fn re_encode(samples: &[f32], seed: u64) -> Vec<f32> {
        let mut rng = Rng::new(seed);
        let mut prev = 0.0f32;
        samples
            .iter()
            .map(|&x| {
                prev = 0.6 * x + 0.4 * prev;
                0.7 * prev + 0.01 * (rng.next_f64() as f32 * 2.0 - 1.0)
            })
            .collect()
    }

    struct Disc {
        play_all: Vec<f32>,
        songs: Vec<Vec<f32>>,
        starts_s: Vec<f64>,
    }

    fn disc(seeds: &[u64], seconds: &[f64]) -> Disc {
        let mut play_all = Vec::new();
        let mut songs = Vec::new();
        let mut starts_s = Vec::new();
        for (&seed, &s) in seeds.iter().zip(seconds) {
            let a = song(seed, s);
            starts_s.push(play_all.len() as f64 / f64::from(RATE));
            play_all.extend_from_slice(&a);
            songs.push(a);
        }
        Disc {
            play_all,
            songs,
            starts_s,
        }
    }

    #[test]
    fn streaming_matches_one_shot() {
        let audio = song(1, 6.0);
        let whole = Fingerprint::from_pcm(&audio, RATE);
        let mut b = FingerprintBuilder::new(RATE);
        let mut rng = Rng::new(9);
        let mut i = 0;
        while i < audio.len() {
            let n = 1 + rng.below(5000);
            let end = (i + n).min(audio.len());
            b.push(&audio[i..end]);
            i = end;
        }
        assert_eq!(b.finish(), whole);
        assert!((whole.frame_s - 0.032).abs() < 1e-9);
        assert!(whole.frames.len() > 150);
    }

    #[test]
    fn fingerprint_ignores_volume() {
        let audio = song(2, 8.0);
        let quiet: Vec<f32> = audio.iter().map(|x| x * 0.25).collect();
        let a = locate(
            &Fingerprint::from_pcm(&quiet, RATE),
            &Fingerprint::from_pcm(&audio, RATE),
        )
        .expect("located");
        assert!(a.score > 0.97, "{a:?}");
        assert!(a.offset_s < 0.01, "{a:?}");
    }

    #[test]
    fn locates_each_song() {
        let d = disc(&[10, 11, 12, 13], &[8.0, 6.0, 10.0, 5.0]);
        let hay = Fingerprint::from_pcm(&d.play_all, RATE);
        for (song, start) in d.songs.iter().zip(&d.starts_s) {
            let a = locate(&Fingerprint::from_pcm(song, RATE), &hay).expect("located");
            assert!((a.offset_s - start).abs() < 0.07, "{a:?} vs {start}");
            // Songs start between the play-all's frames (14 s is 437.5 frames in), which costs
            // some strength; the synthetic tones' abrupt changes make that cost large.
            assert!(a.score > 0.6, "{a:?}");
            assert!((a.length_s - song.len() as f64 / f64::from(RATE)).abs() < 0.3);
        }
        // A song that starts exactly on a frame of the play-all matches almost perfectly.
        let a = locate(&Fingerprint::from_pcm(&d.songs[0], RATE), &hay).expect("located");
        assert!(a.score > 0.97, "{a:?}");
    }

    #[test]
    fn locates_a_different_encode_with_a_sub_frame_offset() {
        let d = disc(&[20, 21, 22], &[8.0, 8.0, 8.0]);
        let hay = Fingerprint::from_pcm(&d.play_all, RATE);
        // Start 7 ms late (not a whole frame), different encode.
        let start = (d.starts_s[1] * f64::from(RATE)) as usize + 112;
        let piece = re_encode(&d.play_all[start..start + d.songs[1].len() - 112], 3);
        let a = locate(&Fingerprint::from_pcm(&piece, RATE), &hay).expect("located");
        assert!((a.offset_s - d.starts_s[1]).abs() < 0.07, "{a:?}");
        assert!(a.score >= MIN_ALIGNMENT_SCORE, "{a:?}");
        assert!(a.score < 0.99, "{a:?}");
    }

    #[test]
    fn unrelated_audio_is_not_found() {
        let d = disc(&[30, 31, 32], &[6.0, 6.0, 6.0]);
        let hay = Fingerprint::from_pcm(&d.play_all, RATE);
        let other = song(99, 5.0);
        let a = locate(&Fingerprint::from_pcm(&other, RATE), &hay).expect("compared");
        assert!(a.score < MIN_ALIGNMENT_SCORE, "{a:?}");
    }

    #[test]
    fn too_short_or_too_long_needles_are_not_aligned() {
        let hay = Fingerprint::from_pcm(&song(40, 6.0), RATE);
        let short = Fingerprint::from_pcm(&song(41, 2.0), RATE);
        assert!(locate(&short, &hay).is_none());
        let long = Fingerprint::from_pcm(&song(42, 12.0), RATE);
        assert!(locate(&long, &hay).is_none());
        let mut other_rate = Fingerprint::from_pcm(&song(43, 5.0), RATE);
        other_rate.frame_s *= 2.0;
        assert!(locate(&other_rate, &hay).is_none());
    }

    fn id(s: &str) -> FileId {
        FileId(s.into())
    }

    fn al(offset_s: f64, length_s: f64, score: f32) -> Option<Alignment> {
        Some(Alignment {
            offset_s,
            length_s,
            score,
        })
    }

    fn chapters(starts: &[f64], end: f64) -> Vec<Chapter> {
        starts
            .iter()
            .enumerate()
            .map(|(i, &s)| Chapter {
                index: i as u32,
                start_s: s,
                end_s: starts.get(i + 1).copied().unwrap_or(end),
                title: None,
            })
            .collect()
    }

    #[test]
    fn order_follows_offsets_and_reports_chapters() {
        let order = derive_disc_order(
            &[
                (id("b"), al(180.0, 175.0, 0.9)),
                (id("a"), al(0.5, 178.0, 0.95)),
                (id("c"), al(360.0, 170.0, 0.8)),
            ],
            &chapters(&[0.0, 180.0, 360.0], 530.0),
        );
        assert!(order.trustworthy, "{order:?}");
        let names: Vec<&str> = order.positions.iter().map(|(f, _)| f.0.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]);
        let chapters: Vec<Option<u32>> = order.positions.iter().map(|(_, p)| p.chapter).collect();
        assert_eq!(chapters, [Some(0), Some(1), Some(2)]);
        assert_eq!(order.positions[2].1.order_index, 2);
    }

    #[test]
    fn weak_alignments_are_not_located() {
        let order = derive_disc_order(
            &[
                (id("a"), al(0.0, 100.0, 0.9)),
                (id("b"), al(100.0, 100.0, 0.1)),
                (id("c"), None),
            ],
            &[],
        );
        assert_eq!(order.positions.len(), 1);
        assert!(!order.trustworthy);
        assert_eq!(order.problem, Some(DiscOrderProblem::TooFewLocated));
    }

    #[test]
    fn too_few_located_is_untrustworthy() {
        let order = derive_disc_order(
            &[
                (id("a"), al(0.0, 100.0, 0.9)),
                (id("b"), al(100.0, 100.0, 0.9)),
                (id("c"), None),
                (id("d"), None),
                (id("e"), None),
            ],
            &[],
        );
        assert_eq!(order.problem, Some(DiscOrderProblem::TooFewLocated));
    }

    #[test]
    fn partial_overlaps_are_untrustworthy() {
        let order = derive_disc_order(
            &[
                (id("a"), al(0.0, 100.0, 0.9)),
                (id("b"), al(70.0, 100.0, 0.9)),
                (id("c"), al(200.0, 100.0, 0.9)),
            ],
            &[],
        );
        assert_eq!(order.problem, Some(DiscOrderProblem::Overlapping));
        assert!(!order.trustworthy);
    }

    #[test]
    fn duplicates_keep_the_stronger_alignment() {
        let order = derive_disc_order(
            &[
                (id("a"), al(0.0, 100.0, 0.9)),
                (id("a-copy"), al(0.5, 99.0, 0.7)),
                (id("b"), al(100.0, 100.0, 0.9)),
            ],
            &[],
        );
        assert!(order.trustworthy, "{order:?}");
        let names: Vec<&str> = order.positions.iter().map(|(f, _)| f.0.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
    }

    #[test]
    fn files_off_the_chapter_marks_are_untrustworthy() {
        let order = derive_disc_order(
            &[
                (id("a"), al(40.0, 100.0, 0.9)),
                (id("b"), al(150.0, 100.0, 0.9)),
                (id("c"), al(260.0, 100.0, 0.9)),
            ],
            &chapters(&[0.0, 120.0, 240.0, 360.0], 400.0),
        );
        assert_eq!(order.problem, Some(DiscOrderProblem::OffChapters));
    }

    #[test]
    fn real_audio_end_to_end() {
        let d = disc(&[50, 51, 52, 53], &[6.0, 7.0, 5.0, 6.0]);
        let hay = Fingerprint::from_pcm(&d.play_all, RATE);
        // Files are named so that name order differs from disc order.
        let names = ["t03", "t01", "t04", "t02"];
        let mut alignments: Vec<(FileId, Option<Alignment>)> = d
            .songs
            .iter()
            .zip(names)
            .map(|(s, n)| (id(n), locate(&Fingerprint::from_pcm(s, RATE), &hay)))
            .collect();
        alignments.push((
            id("extra"),
            locate(&Fingerprint::from_pcm(&song(77, 4.0), RATE), &hay),
        ));
        let total = d.play_all.len() as f64 / f64::from(RATE);
        let order = derive_disc_order(&alignments, &chapters(&d.starts_s, total));
        assert!(order.trustworthy, "{order:?}");
        let got: Vec<&str> = order.positions.iter().map(|(f, _)| f.0.as_str()).collect();
        assert_eq!(got, names);
    }
}
