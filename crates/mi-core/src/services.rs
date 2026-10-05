//! The services a job uses, behind traits so tests can replace each one.
//!
//! - [`MediaBackend`]: scanning folders and decoding audio and subtitles (ffmpeg in the app).
//! - [`Catalog`]: show search, episode lists and reference text (the online sources in the app).
//! - [`SpeechEngine`] and [`Listener`]: loading a speech model and transcribing audio (whisper.cpp
//!   in the app).
//!
//! The real implementations are [`FfmpegMedia`], [`OnlineCatalog`] and [`WhisperEngine`].
//! Pipeline tests use scripted ones, so they run without ffmpeg, a network or a model.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use mi_media::{ExtractOptions, ScanOptions, SidecarLookup, Sidecars};
use mi_sources::{ApiKeys, Cache, HttpClient, ReferenceProvider, Sources};
use mi_transcribe::{DecodeOptions, ModelStore, Transcriber, WhisperTranscriber};
use mi_types::{
    Accelerator, CancelFlag, Episode, EpisodeOrdering, MediaFile, ReferenceText, SampleWindow,
    ScanSummary, Segment, Show, ShowCandidate, ShowRef, SourceStatus, SpeechModel,
};

/// Reading media files.
///
/// Every method blocks (it runs ffmpeg or ffprobe); the pipeline calls them on Tokio's blocking
/// pool. Each stops with `Cancelled` when `cancel` is set.
pub trait MediaBackend: Send + Sync + 'static {
    /// Lists, probes and classifies the video files in `folder` ([`mi_media::scan_folder`]).
    fn scan(&self, folder: &Path, cancel: &CancelFlag) -> mi_media::Result<ScanSummary>;

    /// Decodes `window` (or the whole file) of `file` to 16 kHz mono samples, from the audio
    /// stream that best fits `language`.
    fn decode(
        &self,
        file: &MediaFile,
        window: Option<SampleWindow>,
        language: &str,
        cancel: &CancelFlag,
    ) -> mi_media::Result<Vec<f32>>;

    /// Decodes the whole file in blocks of about one second, for fingerprinting long files
    /// without holding them in memory. Returning `false` from `on_chunk` stops early.
    fn stream(
        &self,
        file: &MediaFile,
        language: &str,
        cancel: &CancelFlag,
        on_chunk: &mut dyn FnMut(&[f32]) -> bool,
    ) -> mi_media::Result<()>;

    /// One text subtitle stream of `file` as SubRip text.
    fn text_subtitles(
        &self,
        file: &MediaFile,
        stream_index: u32,
        cancel: &CancelFlag,
    ) -> mi_media::Result<String>;
}

/// Show search, episode lists and reference text.
#[async_trait]
pub trait Catalog: Send + Sync + 'static {
    /// Shows matching `query`, best first.
    async fn search_shows(&self, query: &str) -> mi_sources::Result<Vec<ShowCandidate>>;

    /// The episode list of `show` in `ordering`.
    async fn episodes(
        &self,
        show: &ShowRef,
        ordering: EpisodeOrdering,
    ) -> mi_sources::Result<Vec<Episode>>;

    /// Reference text for `episodes`; `on_progress(done, total)` counts episodes.
    async fn reference_texts(
        &self,
        show: &Show,
        episodes: &[Episode],
        language: &str,
        on_progress: &(dyn Fn(u32, u32) + Send + Sync),
        cancel: &CancelFlag,
    ) -> mi_sources::Result<Vec<ReferenceText>>;

    /// Status of each online source, for Settings.
    fn status(&self) -> Vec<SourceStatus>;

    /// Replaces the user's API keys.
    fn set_keys(&self, keys: ApiKeys);
}

/// Something that transcribes the audio of one file window at a time.
///
/// Unlike [`mi_transcribe::Transcriber`] it is told which file the audio belongs to, which lets
/// tests script transcripts per file. Every [`Transcriber`] is a `Listener` that ignores the file.
pub trait Listener: Send + 'static {
    /// Transcribes `samples` of `file` starting at `window.start_s`; segment times are file times.
    fn listen(
        &mut self,
        file: &MediaFile,
        window: SampleWindow,
        samples: &[f32],
        options: &DecodeOptions,
        cancel: &CancelFlag,
    ) -> mi_transcribe::Result<Vec<Segment>>;

    /// The processor in use.
    fn accelerator(&self) -> Accelerator;
}

impl<T: Transcriber + 'static> Listener for T {
    fn listen(
        &mut self,
        _file: &MediaFile,
        window: SampleWindow,
        samples: &[f32],
        options: &DecodeOptions,
        cancel: &CancelFlag,
    ) -> mi_transcribe::Result<Vec<Segment>> {
        self.transcribe(samples, window.start_s, options, cancel)
    }

    fn accelerator(&self) -> Accelerator {
        Transcriber::accelerator(self)
    }
}

/// Loads a speech model for one job.
pub trait SpeechEngine: Send + Sync + 'static {
    /// Loads `model`. Blocking (about a second for whisper.cpp). Fails with `ModelMissing` when
    /// the model is not downloaded.
    fn load(&self, model: SpeechModel) -> mi_transcribe::Result<Box<dyn Listener>>;
}

/// The three services of a job.
#[derive(Clone)]
pub struct Services {
    /// Media access.
    pub media: Arc<dyn MediaBackend>,
    /// Shows, episodes and reference text.
    pub catalog: Arc<dyn Catalog>,
    /// Speech recognition.
    pub speech: Arc<dyn SpeechEngine>,
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Services").finish_non_exhaustive()
    }
}

/// [`MediaBackend`] over the ffmpeg and ffprobe sidecars.
///
/// The sidecars are resolved on first use, so the app starts (and the error reaches the screen
/// that needed them) when they are missing.
#[derive(Debug)]
pub struct FfmpegMedia {
    lookup: SidecarLookup,
    sidecars: OnceLock<Sidecars>,
    options: ScanOptions,
}

impl FfmpegMedia {
    /// Looks the tools up with `lookup` when first needed.
    pub fn new(lookup: SidecarLookup) -> Self {
        Self {
            lookup,
            sidecars: OnceLock::new(),
            options: ScanOptions::default(),
        }
    }

    /// Uses these exact programs (tests and the command-line example).
    pub fn from_sidecars(sidecars: Sidecars) -> Self {
        let media = Self::new(SidecarLookup::default());
        let _ = media.sidecars.set(sidecars);
        media
    }

    fn sidecars(&self) -> mi_media::Result<&Sidecars> {
        if let Some(s) = self.sidecars.get() {
            return Ok(s);
        }
        let resolved = Sidecars::resolve(&self.lookup)?;
        Ok(self.sidecars.get_or_init(|| resolved))
    }

    fn extract_options(
        file: &MediaFile,
        window: Option<SampleWindow>,
        language: &str,
    ) -> ExtractOptions {
        let stream_index = file
            .probe
            .as_ref()
            .and_then(|p| mi_media::choose_audio_stream(p, Some(language)));
        ExtractOptions {
            window,
            stream_index,
            language: Some(language.to_owned()),
        }
    }
}

impl MediaBackend for FfmpegMedia {
    fn scan(&self, folder: &Path, cancel: &CancelFlag) -> mi_media::Result<ScanSummary> {
        mi_media::scan_folder(self.sidecars()?, folder, &self.options, cancel)
    }

    fn decode(
        &self,
        file: &MediaFile,
        window: Option<SampleWindow>,
        language: &str,
        cancel: &CancelFlag,
    ) -> mi_media::Result<Vec<f32>> {
        let options = Self::extract_options(file, window, language);
        Ok(mi_media::extract_audio(self.sidecars()?, &file.path, &options, cancel)?.samples)
    }

    fn stream(
        &self,
        file: &MediaFile,
        language: &str,
        cancel: &CancelFlag,
        on_chunk: &mut dyn FnMut(&[f32]) -> bool,
    ) -> mi_media::Result<()> {
        let options = Self::extract_options(file, None, language);
        mi_media::stream_audio(
            self.sidecars()?,
            &file.path,
            &options,
            cancel,
            &mut |chunk| on_chunk(chunk.samples),
        )
    }

    fn text_subtitles(
        &self,
        file: &MediaFile,
        stream_index: u32,
        cancel: &CancelFlag,
    ) -> mi_media::Result<String> {
        mi_media::extract_text_subtitles(self.sidecars()?, &file.path, stream_index, cancel)
    }
}

/// [`Catalog`] over [`Sources`]: TVmaze, optional TMDb, SubDL, LRCLIB, plus any extra reference
/// providers (local subtitle files).
#[derive(Debug)]
pub struct OnlineCatalog {
    sources: Sources,
}

impl OnlineCatalog {
    /// The real sources with the cache at `cache_path`.
    pub fn open(cache_path: &Path, keys: ApiKeys) -> mi_sources::Result<Self> {
        let http = HttpClient::new()?;
        let cache = Cache::open(cache_path)?;
        Ok(Self::new(Sources::new(http, cache, keys)))
    }

    /// Wraps already-built sources (tests build them over a fixture transport).
    pub fn new(sources: Sources) -> Self {
        Self { sources }
    }

    /// Adds a reference-text source tried before the online ones (for example
    /// [`mi_sources::local::LocalReferences`]).
    pub fn with_reference_provider(mut self, provider: Arc<dyn ReferenceProvider>) -> Self {
        self.sources.add_reference_provider(provider);
        self
    }
}

#[async_trait]
impl Catalog for OnlineCatalog {
    async fn search_shows(&self, query: &str) -> mi_sources::Result<Vec<ShowCandidate>> {
        self.sources.search_shows(query).await
    }

    async fn episodes(
        &self,
        show: &ShowRef,
        ordering: EpisodeOrdering,
    ) -> mi_sources::Result<Vec<Episode>> {
        self.sources.episodes(show, ordering).await
    }

    async fn reference_texts(
        &self,
        show: &Show,
        episodes: &[Episode],
        language: &str,
        on_progress: &(dyn Fn(u32, u32) + Send + Sync),
        cancel: &CancelFlag,
    ) -> mi_sources::Result<Vec<ReferenceText>> {
        self.sources
            .reference_texts(show, episodes, language, on_progress, cancel)
            .await
    }

    fn status(&self) -> Vec<SourceStatus> {
        self.sources.status()
    }

    fn set_keys(&self, keys: ApiKeys) {
        self.sources.set_keys(keys);
    }
}

/// [`Catalog`] that works without a network: an episode list given in advance and reference
/// texts from local subtitle or lyrics files ([`mi_sources::local::LocalReferences`]). Used by
/// the end-to-end checks and the command-line example; the app uses [`OnlineCatalog`].
#[derive(Debug)]
pub struct LocalCatalog {
    show: Show,
    episodes: Vec<Episode>,
    references: mi_sources::local::LocalReferences,
}

impl LocalCatalog {
    /// A catalog of one show.
    pub fn new(
        show: Show,
        episodes: Vec<Episode>,
        references: mi_sources::local::LocalReferences,
    ) -> Self {
        Self {
            show,
            episodes,
            references,
        }
    }
}

#[async_trait]
impl Catalog for LocalCatalog {
    async fn search_shows(&self, _query: &str) -> mi_sources::Result<Vec<ShowCandidate>> {
        Ok(vec![ShowCandidate {
            show: self.show.clone(),
            score: 1.0,
            guessed_from_folder: false,
        }])
    }

    async fn episodes(
        &self,
        _show: &ShowRef,
        ordering: EpisodeOrdering,
    ) -> mi_sources::Result<Vec<Episode>> {
        Ok(self
            .episodes
            .iter()
            .filter(|e| e.ordering == ordering)
            .cloned()
            .collect())
    }

    async fn reference_texts(
        &self,
        show: &Show,
        episodes: &[Episode],
        language: &str,
        on_progress: &(dyn Fn(u32, u32) + Send + Sync),
        cancel: &CancelFlag,
    ) -> mi_sources::Result<Vec<ReferenceText>> {
        let total = episodes.len() as u32;
        on_progress(0, total);
        let ids = mi_sources::ShowIds::default();
        let aired = std::collections::HashMap::new();
        let request = mi_sources::ReferenceRequest {
            show,
            ids: &ids,
            episodes,
            aired: &aired,
            language,
            cancel,
        };
        let texts = self.references.reference_texts(&request).await?;
        on_progress(total, total);
        Ok(texts)
    }

    fn status(&self) -> Vec<SourceStatus> {
        Vec::new()
    }

    fn set_keys(&self, _keys: ApiKeys) {}
}

/// [`SpeechEngine`] over whisper.cpp with the models in a [`ModelStore`] folder.
#[derive(Debug, Clone)]
pub struct WhisperEngine {
    models: PathBuf,
    use_gpu: bool,
}

impl WhisperEngine {
    /// Uses the models downloaded into `models` (the [`ModelStore`] folder).
    pub fn new(models: impl Into<PathBuf>) -> Self {
        Self {
            models: models.into(),
            use_gpu: true,
        }
    }

    /// Forces the CPU (for tests and measurements).
    pub fn cpu_only(mut self) -> Self {
        self.use_gpu = false;
        self
    }
}

impl SpeechEngine for WhisperEngine {
    fn load(&self, model: SpeechModel) -> mi_transcribe::Result<Box<dyn Listener>> {
        let store = ModelStore::new(&self.models);
        let path = store.model_path(model);
        if !path.is_file() {
            return Err(mi_transcribe::TranscribeError::ModelMissing(
                mi_transcribe::model_info(model).file_name,
            ));
        }
        let mut transcriber = WhisperTranscriber::load(&path, self.use_gpu)?;
        match store.vad_model_path() {
            Some(vad) => transcriber = transcriber.with_vad_model(&vad)?,
            None => tracing::warn!("voice activity detection model missing; decoding without it"),
        }
        Ok(Box::new(transcriber))
    }
}
