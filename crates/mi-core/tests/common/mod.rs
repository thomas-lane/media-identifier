//! Scripted services for pipeline tests: a fake folder, a fake catalog and a fake speech model.
//!
//! The show, "Harbour Tales", is four episodes written for these tests. Nothing here runs ffmpeg,
//! a network request or a speech model, and nothing here says anything about accuracy on real
//! recordings.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mi_core::{Catalog, Engine, EventSink, Listener, MediaBackend, Services, SpeechEngine};
use mi_sources::ApiKeys;
use mi_transcribe::DecodeOptions;
use mi_types::{
    Accelerator, CancelFlag, Chapter, Episode, EpisodeKey, EpisodeOrdering, FileId, FileRole,
    JobEvent, JobId, MediaFile, ModelStatus, PlayAllInfo, Probe, ProviderId, ReferenceText,
    SampleWindow, ScanSummary, Segment, Settings, Show, ShowCandidate, ShowRef, SourceState,
    SourceStatus, SpeechModel, SubtitleStream, TextKind,
};

pub const SAMPLE_RATE: usize = 16_000;

pub const TITLES: [&str; 4] = [
    "The Lighthouse Keeper",
    "The Missing Anchor",
    "The Storm at Midnight",
    "The Fishing Contest",
];

pub const STORIES: [&str; 4] = [
    "The old keeper climbed the spiral stairs every evening to light the great lamp. \
     He polished the brass lens until it shone like a second moon over the bay. \
     One night the lamp flickered and the keeper found a family of owls nesting in the gears. \
     He built them a wooden box on the balcony so the light could turn again.",
    "Captain Morgan woke to find the ship's anchor gone from the deck. \
     The crew searched the hold, the galley and even the captain's cabin. \
     Little Pip noticed a trail of wet footprints leading to the blacksmith's forge. \
     The blacksmith confessed he had borrowed it to straighten a bent iron gate.",
    "Thunder rolled across the harbour and the waves crashed over the sea wall. \
     The fishermen tied their boats with double knots and hurried indoors. \
     Grandmother Rose lit candles in every window so the late boats could find their way home. \
     By sunrise every sailor was safe and the village shared hot soup on the quay.",
    "Every summer the village held a contest for the biggest fish of the season. \
     Tom the baker had never caught anything larger than a sardine. \
     This year he used a crust of his famous raisin bread as bait. \
     A giant silver salmon leapt onto his line and he won the golden trophy.",
];

pub fn show() -> Show {
    Show {
        show_ref: ShowRef {
            provider: ProviderId::Tvmaze,
            id: "9001".into(),
        },
        name: "Harbour Tales".into(),
        year: Some(1999),
        kind: Some("Animation".into()),
        season_count: Some(1),
        episode_count: Some(4),
        url: None,
    }
}

pub fn episode(i: usize) -> Episode {
    Episode {
        show_ref: show().show_ref,
        ordering: EpisodeOrdering::Aired,
        key: EpisodeKey {
            season: 1,
            number: i as u32 + 1,
        },
        title: TITLES[i].into(),
        runtime_s: None,
        airdate: None,
        summary: Some(format!("A story about {}.", TITLES[i].to_lowercase())),
        provider_episode_id: format!("ep{}", i + 1),
    }
}

pub fn episodes() -> Vec<Episode> {
    (0..TITLES.len()).map(episode).collect()
}

pub fn subtitles(i: usize) -> ReferenceText {
    ReferenceText {
        show_ref: show().show_ref,
        ordering: EpisodeOrdering::Aired,
        episode: episode(i).key,
        kind: TextKind::Subtitles,
        provider: ProviderId::Local,
        provider_ref: format!("ep{}.srt", i + 1),
        text: STORIES[i].replace(". ", ".\n"),
        language: "en".into(),
        fetched_at_ms: 0,
    }
}

/// A probed media file in `folder`.
pub fn media_file(folder: &Path, name: &str, duration_s: f64, role: FileRole) -> MediaFile {
    MediaFile {
        id: FileId(name.into()),
        path: folder.join(name),
        file_name: name.into(),
        size_bytes: 1000,
        probe: Some(Probe {
            duration_s,
            container: "matroska,webm".into(),
            video: None,
            audio_streams: Vec::new(),
            subtitle_streams: Vec::new(),
            chapters: Vec::new(),
        }),
        role,
    }
}

/// A scan of `files` in `folder` with an optional play-all among them.
pub fn scan(folder: &Path, files: Vec<MediaFile>) -> ScanSummary {
    let play_all = files.iter().find(|f| f.role == FileRole::PlayAll).map(|f| {
        let probe = f.probe.as_ref().unwrap();
        PlayAllInfo {
            file_id: f.id.clone(),
            duration_s: probe.duration_s,
            chapter_count: probe.chapters.len() as u32,
            candidates_total_s: 0.0,
            chapters_matched: 0,
            confidence: 1.0,
            reason: "test".into(),
        }
    });
    ScanSummary {
        folder: folder.to_path_buf(),
        candidate_count: files
            .iter()
            .filter(|f| f.role == FileRole::Candidate)
            .count() as u32,
        files,
        play_all,
        show_guess: Some("Harbour Tales".into()),
        warnings: Vec::new(),
    }
}

/// Adds a text subtitle stream (index 2) to a file's probe.
pub fn with_text_subtitles(mut file: MediaFile) -> MediaFile {
    file.probe
        .as_mut()
        .unwrap()
        .subtitle_streams
        .push(SubtitleStream {
            index: 2,
            codec: "subrip".into(),
            language: Some("eng".into()),
            title: None,
            is_text: true,
        });
    file
}

/// Adds chapters at the given start times to a file's probe.
pub fn with_chapters(mut file: MediaFile, starts: &[f64]) -> MediaFile {
    let end = file.probe.as_ref().unwrap().duration_s;
    let chapters = starts
        .iter()
        .enumerate()
        .map(|(i, s)| Chapter {
            index: i as u32,
            start_s: *s,
            end_s: starts.get(i + 1).copied().unwrap_or(end),
            title: None,
        })
        .collect();
    file.probe.as_mut().unwrap().chapters = chapters;
    file
}

/// Deterministic noise, so each file's "audio" is distinct and can be found in a play-all.
pub fn noise(seed: u64, seconds: f64) -> Vec<f32> {
    let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    let n = (seconds * SAMPLE_RATE as f64) as usize;
    let mut out = Vec::with_capacity(n);
    // Noise shaped by a slowly changing gain, so the fingerprint has structure over time.
    for i in 0..n {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let white = ((state >> 33) as f32 / (1u64 << 31) as f32) - 0.5;
        let gain = 0.3 + 0.7 * (((i / 4000) as u64 ^ seed) % 7) as f32 / 7.0;
        out.push(white * gain);
    }
    out
}

/// A folder whose files decode to scripted samples.
#[derive(Default)]
pub struct FakeMedia {
    pub summary: Mutex<Option<ScanSummary>>,
    /// Whole-file samples per file; files without an entry decode to silence.
    pub audio: HashMap<FileId, Vec<f32>>,
    /// Files whose decoding fails.
    pub broken: HashSet<FileId>,
    /// SubRip text per file for its text subtitle stream.
    pub subtitles: HashMap<FileId, String>,
    /// Every decoded window, in order.
    pub decoded: Mutex<Vec<(FileId, SampleWindow)>>,
}

impl FakeMedia {
    pub fn new(summary: ScanSummary) -> Self {
        Self {
            summary: Mutex::new(Some(summary)),
            ..Self::default()
        }
    }

    pub fn decoded_windows(&self, file: &str) -> Vec<SampleWindow> {
        self.decoded
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, _)| id.0 == file)
            .map(|(_, w)| *w)
            .collect()
    }

    fn samples(&self, file: &MediaFile) -> Vec<f32> {
        self.audio.get(&file.id).cloned().unwrap_or_else(|| {
            let d = file.probe.as_ref().map_or(0.0, |p| p.duration_s);
            vec![0.0; (d.min(30.0) * SAMPLE_RATE as f64) as usize]
        })
    }
}

impl MediaBackend for FakeMedia {
    fn scan(&self, folder: &Path, _cancel: &CancelFlag) -> mi_media::Result<ScanSummary> {
        let mut s = self
            .summary
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| mi_media::MediaError::Io(std::io::Error::other("no such folder")))?;
        s.folder = folder.to_path_buf();
        Ok(s)
    }

    fn decode(
        &self,
        file: &MediaFile,
        window: Option<SampleWindow>,
        _language: &str,
        cancel: &CancelFlag,
    ) -> mi_media::Result<Vec<f32>> {
        mi_media::check_cancel(cancel)?;
        let duration = file.probe.as_ref().map_or(0.0, |p| p.duration_s);
        let window = window.unwrap_or(SampleWindow {
            start_s: 0.0,
            end_s: duration,
        });
        self.decoded.lock().unwrap().push((file.id.clone(), window));
        if self.broken.contains(&file.id) {
            return Err(mi_media::MediaError::ToolFailed {
                tool: mi_media::Tool::Ffmpeg,
                path: file.path.clone(),
                message: "Invalid data found when processing input".into(),
            });
        }
        // A short buffer stands for the window; the scripted listener ignores the samples.
        Ok(vec![0.0; SAMPLE_RATE])
    }

    fn stream(
        &self,
        file: &MediaFile,
        _language: &str,
        cancel: &CancelFlag,
        on_chunk: &mut dyn FnMut(&[f32]) -> bool,
    ) -> mi_media::Result<()> {
        if self.broken.contains(&file.id) {
            return Err(mi_media::MediaError::ToolFailed {
                tool: mi_media::Tool::Ffmpeg,
                path: file.path.clone(),
                message: "Invalid data found when processing input".into(),
            });
        }
        for chunk in self.samples(file).chunks(SAMPLE_RATE) {
            mi_media::check_cancel(cancel)?;
            if !on_chunk(chunk) {
                break;
            }
        }
        Ok(())
    }

    fn text_subtitles(
        &self,
        file: &MediaFile,
        _stream_index: u32,
        _cancel: &CancelFlag,
    ) -> mi_media::Result<String> {
        self.subtitles
            .get(&file.id)
            .cloned()
            .ok_or_else(|| mi_media::MediaError::ToolFailed {
                tool: mi_media::Tool::Ffmpeg,
                path: file.path.clone(),
                message: "no subtitles".into(),
            })
    }
}

/// An episode list and reference texts held in memory.
pub struct FakeCatalog {
    pub episodes: Result<Vec<Episode>, String>,
    pub texts: Result<Vec<ReferenceText>, String>,
}

impl FakeCatalog {
    pub fn with_subtitles() -> Self {
        Self {
            episodes: Ok(episodes()),
            texts: Ok((0..TITLES.len()).map(subtitles).collect()),
        }
    }
}

#[async_trait]
impl Catalog for FakeCatalog {
    async fn search_shows(&self, _query: &str) -> mi_sources::Result<Vec<ShowCandidate>> {
        Ok(vec![ShowCandidate {
            show: show(),
            score: 1.0,
            guessed_from_folder: false,
        }])
    }

    async fn episodes(
        &self,
        _show: &ShowRef,
        _ordering: EpisodeOrdering,
    ) -> mi_sources::Result<Vec<Episode>> {
        self.episodes
            .clone()
            .map_err(|message| mi_sources::SourceError::Network {
                provider: ProviderId::Tvmaze,
                message,
            })
    }

    async fn reference_texts(
        &self,
        _show: &Show,
        episodes: &[Episode],
        _language: &str,
        on_progress: &(dyn Fn(u32, u32) + Send + Sync),
        cancel: &CancelFlag,
    ) -> mi_sources::Result<Vec<ReferenceText>> {
        if cancel.is_cancelled() {
            return Err(mi_sources::SourceError::Cancelled);
        }
        on_progress(episodes.len() as u32, episodes.len() as u32);
        self.texts
            .clone()
            .map_err(|message| mi_sources::SourceError::Network {
                provider: ProviderId::Subdl,
                message,
            })
    }

    fn status(&self) -> Vec<SourceStatus> {
        vec![SourceStatus {
            provider: ProviderId::Tvmaze,
            state: SourceState::Ready,
            has_key: false,
        }]
    }

    fn set_keys(&self, _keys: ApiKeys) {}
}

/// What each file "says": (time in the file, text) lines.
pub type Script = HashMap<FileId, Vec<(f64, String)>>;

/// A speech model that returns each file's scripted lines inside the window it is given.
pub struct FakeSpeech {
    pub script: Script,
    pub missing_model: bool,
    /// Time each window takes, so a test can cancel mid-job.
    pub delay: Duration,
    /// Every window transcribed, with whether voice activity detection was on.
    pub heard: Arc<Mutex<Vec<(FileId, SampleWindow, bool)>>>,
}

impl FakeSpeech {
    pub fn new(script: Script) -> Self {
        Self {
            script,
            missing_model: false,
            delay: Duration::ZERO,
            heard: Arc::default(),
        }
    }
}

struct FakeListener {
    script: Script,
    delay: Duration,
    heard: Arc<Mutex<Vec<(FileId, SampleWindow, bool)>>>,
}

impl SpeechEngine for FakeSpeech {
    fn load(&self, model: SpeechModel) -> mi_transcribe::Result<Box<dyn Listener>> {
        if self.missing_model {
            return Err(mi_transcribe::TranscribeError::ModelMissing(
                mi_transcribe::model_info(model).file_name,
            ));
        }
        Ok(Box::new(FakeListener {
            script: self.script.clone(),
            delay: self.delay,
            heard: Arc::clone(&self.heard),
        }))
    }
}

impl Listener for FakeListener {
    fn listen(
        &mut self,
        file: &MediaFile,
        window: SampleWindow,
        _samples: &[f32],
        options: &DecodeOptions,
        cancel: &CancelFlag,
    ) -> mi_transcribe::Result<Vec<Segment>> {
        self.heard
            .lock()
            .unwrap()
            .push((file.id.clone(), window, options.vad));
        let until = Instant::now() + self.delay;
        while Instant::now() < until {
            if cancel.is_cancelled() {
                return Err(mi_transcribe::TranscribeError::Cancelled);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if cancel.is_cancelled() {
            return Err(mi_transcribe::TranscribeError::Cancelled);
        }
        Ok(self
            .script
            .get(&file.id)
            .into_iter()
            .flatten()
            .filter(|(t, _)| *t >= window.start_s && *t < window.end_s)
            .map(|(t, text)| Segment {
                start_s: *t,
                end_s: t + 4.0,
                text: text.clone(),
                avg_logprob: -0.2,
                no_speech_prob: 0.01,
                filtered: None,
            })
            .collect())
    }

    fn accelerator(&self) -> Accelerator {
        Accelerator::AppleGpu
    }
}

/// Collects every event.
#[derive(Default)]
pub struct Recorder {
    pub events: Mutex<Vec<JobEvent>>,
    pub models: Mutex<Vec<ModelStatus>>,
}

impl EventSink for Recorder {
    fn job_event(&self, event: JobEvent) {
        self.events.lock().unwrap().push(event);
    }

    fn model_status(&self, status: ModelStatus) {
        self.models.lock().unwrap().push(status);
    }
}

impl Recorder {
    pub fn events(&self) -> Vec<JobEvent> {
        self.events.lock().unwrap().clone()
    }

    /// Waits (up to 60 s) for the job's last event and returns it.
    pub async fn wait_for_end(&self, job: &JobId) -> JobEvent {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(e) = self.events().into_iter().find(|e| {
                matches!(e,
                    JobEvent::Finished { job_id } | JobEvent::Cancelled { job_id }
                    | JobEvent::Failed { job_id, .. } if job_id == job)
            }) {
                return e;
            }
            assert!(Instant::now() < deadline, "the job did not end");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

/// An engine over the fakes in a temporary data folder.
pub struct Harness {
    pub engine: Engine,
    pub recorder: Arc<Recorder>,
    pub media: Arc<FakeMedia>,
    pub data: tempfile::TempDir,
    pub folder: PathBuf,
}

pub fn harness(media: FakeMedia, catalog: FakeCatalog, speech: FakeSpeech) -> Harness {
    harness_with(media, catalog, speech, Settings::default())
}

pub fn harness_with(
    media: FakeMedia,
    catalog: FakeCatalog,
    speech: FakeSpeech,
    settings: Settings,
) -> Harness {
    let data = tempfile::tempdir().unwrap();
    let recorder = Arc::new(Recorder::default());
    let media = Arc::new(media);
    let folder = media
        .summary
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| s.folder.clone())
        .unwrap_or_default();
    let services = Services {
        media: media.clone(),
        catalog: Arc::new(catalog),
        speech: Arc::new(speech),
    };
    let engine = Engine::with_services(
        data.path(),
        settings,
        tokio::runtime::Handle::current(),
        services,
        recorder.clone(),
    );
    Harness {
        engine,
        recorder,
        media,
        data,
        folder,
    }
}

/// The script of file `i`: its story, line by line, starting at `start_s`, 5 s apart.
pub fn story_lines(i: usize, start_s: f64) -> Vec<(f64, String)> {
    STORIES[i]
        .split(". ")
        .enumerate()
        .map(|(n, line)| (start_s + n as f64 * 5.0, line.to_owned()))
        .collect()
}

pub fn request(folder: &Path) -> mi_types::JobRequest {
    mi_types::JobRequest {
        folder: folder.to_path_buf(),
        show: show(),
        ordering: EpisodeOrdering::Aired,
        seasons: None,
        language: "en".into(),
    }
}
