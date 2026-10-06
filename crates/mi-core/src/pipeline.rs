//! One identification job, stage by stage.
//!
//! 1. **Scan** (reused from the Confirm show screen when the folder is the same) and load the
//!    speech model; then `Started`. The play-all is reported at once as `PlayAll`.
//! 2. **Episode list** from the catalog, limited to the requested seasons; then `Episodes`.
//! 3. **Reference text** (cache first, then local files, SubDL, LRCLIB, summaries). A failure
//!    here marks the stage failed and the job continues: episode summaries and titles still help.
//! 4. **Disc order** (only with a play-all and at least two files): fingerprint the play-all and
//!    every file, locate each file inside the play-all, and derive the order.
//! 5. **Listening**, file by file: embedded text subtitles when the file has them, then the
//!    planned windows are decoded and transcribed. Each file is matched on its own as soon as it
//!    is heard (`Matched`, so Review can start early).
//! 6. **Matching**: every file against every episode with the disc order, then the global
//!    assignment. Files whose margin is low are listened to further (more windows, then the whole
//!    file) and everything is matched again. The final `Matched` of every file follows.
//!
//! Cancelling stops at the next check (between windows, inside ffmpeg and whisper.cpp); the job
//! then ends with `Cancelled`, keeping the results it has.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use mi_match::{
    DiscOrder, EpisodeInput, FileInput, FingerprintBuilder, MatchConfig, MatchInput,
    derive_disc_order, locate, match_files, match_with_outcome, needs_more_listening,
};
use mi_transcribe::{
    DecodeOptions, HallucinationFilter, SamplingPolicy, SpeedEstimator, add_window,
    audio_cost_seconds, escalation_windows, plan_windows,
};
use mi_types::{
    CancelFlag, Confidence, Episode, FileId, FileMatch, FileRole, FileStatus, FilterReason,
    JobEvent, JobId, JobRequest, MediaFile, PlayAllPosition, ReferenceText, SampleWindow,
    ScanSummary, Settings, SpeechModel, Stage, StageState, Suggestion, TextKind, Transcript,
    Verdict,
};

use crate::jobs::{JobRecord, SharedRecord};
use crate::services::{Listener, Services};
use crate::{CoreError, EventSink};

/// Tunable rules of a job.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Which parts of long files to transcribe.
    pub sampling: SamplingPolicy,
    /// Matching weights and thresholds.
    pub matching: MatchConfig,
    /// Rounds of further listening for low-margin files: the first adds windows in the gaps, the
    /// last transcribes the rest of the file.
    pub max_escalations: u32,
    /// Fewest words an embedded text subtitle stream needs to be used as the file's dialogue.
    pub min_embedded_words: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            sampling: SamplingPolicy::default(),
            matching: MatchConfig::default(),
            max_escalations: 2,
            min_embedded_words: 20,
        }
    }
}

/// Everything one job needs.
pub struct JobContext {
    /// The job.
    pub job_id: JobId,
    /// What to identify.
    pub request: JobRequest,
    /// Settings when the job started.
    pub settings: Settings,
    /// Media, catalog and speech.
    pub services: Services,
    /// Receives progress events.
    pub sink: Arc<dyn EventSink>,
    /// Set to cancel.
    pub cancel: CancelFlag,
    /// The folder's scan when the Confirm show screen already made it.
    pub scan: Option<ScanSummary>,
    /// Where results are kept as they arrive.
    pub record: SharedRecord,
    /// Rules.
    pub config: PipelineConfig,
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Every file has its final result.
    Finished,
    /// The user cancelled.
    Cancelled,
    /// The job could not continue; the message is shown to the user.
    Failed(String),
}

/// Runs one job to its end. The caller sends [`final_event`] afterwards, once it has stored the
/// results, so the last event never arrives before the results can be read.
pub async fn run(ctx: JobContext) -> Outcome {
    match Job::new(&ctx).run().await {
        Ok(()) => Outcome::Finished,
        Err(e) if is_cancelled(&e) || ctx.cancel.is_cancelled() => Outcome::Cancelled,
        Err(e) => {
            tracing::warn!(error = %e, "identification failed");
            Outcome::Failed(failure_message(&e))
        }
    }
}

/// The last event of a job: `Finished`, `Cancelled` or `Failed`.
pub fn final_event(job_id: &JobId, outcome: &Outcome) -> JobEvent {
    let job_id = job_id.clone();
    match outcome {
        Outcome::Finished => JobEvent::Finished { job_id },
        Outcome::Cancelled => JobEvent::Cancelled { job_id },
        Outcome::Failed(message) => JobEvent::Failed {
            job_id,
            message: message.clone(),
        },
    }
}

fn is_cancelled(e: &CoreError) -> bool {
    matches!(
        e,
        CoreError::Cancelled
            | CoreError::Media(mi_media::MediaError::Cancelled)
            | CoreError::Transcribe(mi_transcribe::TranscribeError::Cancelled)
            | CoreError::Sources(mi_sources::SourceError::Cancelled)
            | CoreError::Match(mi_match::MatchError::Cancelled)
    )
}

/// The sentence shown when a job fails.
pub fn failure_message(e: &CoreError) -> String {
    match e {
        CoreError::Transcribe(mi_transcribe::TranscribeError::ModelMissing(_)) => {
            "The speech model is not downloaded yet. Download it on the Start screen, then try again."
                .to_owned()
        }
        CoreError::Media(mi_media::MediaError::SidecarMissing { tool, .. }) => format!(
            "Media Identifier could not find its {tool} program. Reinstalling the app should fix this."
        ),
        CoreError::Sources(e) => format!("Couldn't get the episode list: {e}."),
        CoreError::Job(message) | CoreError::Invalid(message) => message.clone(),
        other => other.to_string(),
    }
}

/// One file being identified.
struct Item {
    file: MediaFile,
    duration_s: f64,
    transcript: Transcript,
    embedded: Option<String>,
    music_heavy: bool,
    failed: bool,
}

impl Item {
    /// Nothing usable was heard or read in this file.
    fn unusable(&self) -> bool {
        self.failed && self.embedded.is_none() && self.transcript.segments.is_empty()
    }
}

struct Job<'a> {
    ctx: &'a JobContext,
    model: SpeechModel,
    language: String,
    filter: HallucinationFilter,
}

/// Runs blocking work on Tokio's blocking pool.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, CoreError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| CoreError::Job(format!("a background task failed: {e}")))
}

impl<'a> Job<'a> {
    fn new(ctx: &'a JobContext) -> Self {
        let model = ctx.settings.speech_model;
        // The Fast model (small.en) understands English only.
        let language = match model {
            SpeechModel::Fast => "en".to_owned(),
            SpeechModel::Accurate => ctx.request.language.clone(),
        };
        Self {
            ctx,
            model,
            language,
            filter: HallucinationFilter::default(),
        }
    }

    fn emit(&self, event: JobEvent) {
        self.ctx.sink.job_event(event);
    }

    fn stage(&self, stage: Stage, state: StageState) {
        self.emit(JobEvent::Stage {
            job_id: self.ctx.job_id.clone(),
            stage,
            state,
        });
    }

    fn file_status(
        &self,
        file_id: &FileId,
        status: FileStatus,
        best_so_far: Option<String>,
        verdict: Option<Verdict>,
    ) {
        self.emit(JobEvent::File {
            job_id: self.ctx.job_id.clone(),
            file_id: file_id.clone(),
            status,
            best_so_far,
            verdict,
        });
    }

    fn check_cancel(&self) -> Result<(), CoreError> {
        if self.ctx.cancel.is_cancelled() {
            Err(CoreError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn update_record(&self, f: impl FnOnce(&mut JobRecord)) {
        let mut record = self.ctx.record.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut record);
    }

    /// Records each match as its file's current result and sends it.
    fn publish_all(&self, matches: &[FileMatch]) {
        for m in matches {
            self.publish(m.clone());
        }
    }

    /// Records `m` as the file's current result and sends it.
    fn publish(&self, m: FileMatch) {
        self.update_record(|r| {
            match r
                .results
                .matches
                .iter_mut()
                .find(|x| x.file_id == m.file_id)
            {
                Some(existing) => *existing = m.clone(),
                None => r.results.matches.push(m.clone()),
            }
        });
        self.emit(JobEvent::Matched {
            job_id: self.ctx.job_id.clone(),
            result: m,
        });
    }

    async fn run(&self) -> Result<(), CoreError> {
        let ctx = self.ctx;
        let services = &ctx.services;

        // 1. Scan and model.
        let scan = match ctx.scan.clone() {
            Some(scan) if scan.folder == ctx.request.folder => scan,
            _ => {
                let media = Arc::clone(&services.media);
                let folder = ctx.request.folder.clone();
                let cancel = ctx.cancel.clone();
                blocking(move || media.scan(&folder, &cancel)).await??
            }
        };
        let candidates: Vec<MediaFile> = scan
            .files
            .iter()
            .filter(|f| f.role == FileRole::Candidate)
            .cloned()
            .collect();
        if candidates.is_empty() {
            return Err(CoreError::Invalid(
                "There are no video files to identify in this folder.".to_owned(),
            ));
        }
        let play_all: Option<MediaFile> = scan
            .play_all
            .as_ref()
            .and_then(|p| scan.files.iter().find(|f| f.id == p.file_id))
            .cloned();
        self.update_record(|r| r.files = scan.files.clone());

        let speech = Arc::clone(&services.speech);
        let model = self.model;
        let mut listener: Box<dyn Listener> = blocking(move || speech.load(model)).await??;
        self.check_cancel()?;

        let mut file_ids: Vec<FileId> = candidates.iter().map(|f| f.id.clone()).collect();
        if let Some(p) = &play_all {
            file_ids.push(p.id.clone());
        }
        let accelerator = listener.accelerator();
        let mut speed = SpeedEstimator::new(model, accelerator);
        self.emit(JobEvent::Started {
            job_id: ctx.job_id.clone(),
            file_ids,
            accelerator,
        });
        if let Some(p) = &play_all {
            self.file_status(&p.id, FileStatus::Done, None, Some(Verdict::PlayAll));
            self.publish(FileMatch {
                file_id: p.id.clone(),
                suggestion: Suggestion::PlayAll,
                confidence: Confidence {
                    score: 1.0,
                    margin: 1.0,
                    verdict: Verdict::PlayAll,
                },
                candidates: Vec::new(),
            });
        }

        // 2. Episode list.
        self.stage(
            Stage::EpisodeList,
            StageState::Running { done: 0, total: 1 },
        );
        let episodes = self.episode_list().await.inspect_err(|e| {
            if !is_cancelled(e) {
                self.stage(
                    Stage::EpisodeList,
                    StageState::Failed {
                        message: failure_message(e),
                    },
                );
            }
        })?;
        self.stage(Stage::EpisodeList, StageState::Done);
        self.update_record(|r| r.results.episodes = episodes.clone());
        self.emit(JobEvent::Episodes {
            job_id: ctx.job_id.clone(),
            episodes: episodes.clone(),
        });

        // 3. Reference text.
        let texts = self.reference_texts(&episodes).await?;
        let show_is_musical = show_is_musical(&episodes, &texts);
        let episode_inputs = episode_inputs(&episodes, texts);

        // 4. Disc order.
        let disc_order = self.disc_order(play_all.as_ref(), &candidates).await?;
        let positions: HashMap<FileId, PlayAllPosition> = disc_order
            .as_ref()
            .map(|o| o.positions.iter().cloned().collect())
            .unwrap_or_default();

        // 5. Listening.
        let total = candidates.len() as u32;
        let policy = &ctx.config.sampling;
        let sample = ctx.settings.sample_long_files;
        let mut items: Vec<Item> = candidates
            .into_iter()
            .map(|file| {
                let duration_s = file.probe.as_ref().map_or(0.0, |p| p.duration_s);
                Item {
                    transcript: Transcript {
                        file_id: file.id.clone(),
                        model,
                        language: self.language.clone(),
                        windows: Vec::new(),
                        segments: Vec::new(),
                    },
                    file,
                    duration_s,
                    embedded: None,
                    music_heavy: show_is_musical,
                    failed: false,
                }
            })
            .collect();
        let planned: Vec<Vec<SampleWindow>> = items
            .iter()
            .map(|i| plan_windows(i.duration_s, policy, sample))
            .collect();
        let mut remaining_cost: f64 = planned.iter().map(|w| audio_cost_seconds(w)).sum();
        self.stage(Stage::Listening, StageState::Running { done: 0, total });
        for (index, item) in items.iter_mut().enumerate() {
            self.check_cancel()?;
            self.file_status(&item.file.id, FileStatus::Listening, None, None);
            item.embedded = self.embedded_dialogue(&item.file).await?;
            for window in &planned[index] {
                self.emit(JobEvent::Eta {
                    job_id: ctx.job_id.clone(),
                    seconds: speed.remaining_seconds(remaining_cost),
                });
                let cost = audio_cost_seconds(std::slice::from_ref(window));
                let started = Instant::now();
                match self.listen(&mut listener, item, *window).await {
                    Ok(()) => speed.record(cost, started.elapsed()),
                    Err(e) if is_cancelled(&e) => return Err(e),
                    Err(e) => {
                        tracing::warn!(file = %item.file.id.0, error = %e, "could not listen to a file");
                        item.failed = true;
                    }
                }
                remaining_cost = (remaining_cost - cost).max(0.0);
                if item.failed {
                    break;
                }
            }
            if item.unusable() {
                self.file_status(&item.file.id, FileStatus::Failed, None, None);
            } else {
                // Match this file on its own so Review can show it before the others finish.
                self.file_status(&item.file.id, FileStatus::Matching, None, None);
                let input = MatchInput {
                    files: vec![self.file_input(item, positions.get(&item.file.id).cloned())],
                    episodes: episode_inputs.clone(),
                    disc_order: None,
                };
                let matching = ctx.config.matching.clone();
                let cancel = ctx.cancel.clone();
                let interim = blocking(move || match_files(&input, &matching, &cancel)).await??;
                if let Some(m) = interim.into_iter().next() {
                    let best = m.candidates.first().map(|c| c.title.clone());
                    let verdict = m.confidence.verdict;
                    self.publish(m);
                    self.file_status(&item.file.id, FileStatus::Done, best, Some(verdict));
                }
            }
            self.update_record(|r| {
                r.transcripts
                    .insert(item.file.id.clone(), item.transcript.clone());
            });
            self.stage(
                Stage::Listening,
                StageState::Running {
                    done: index as u32 + 1,
                    total,
                },
            );
        }
        self.stage(Stage::Listening, StageState::Done);
        self.emit(JobEvent::Eta {
            job_id: ctx.job_id.clone(),
            seconds: 0.0,
        });

        // 6. Matching, with further listening for uncertain files.
        let rounds = ctx.config.max_escalations;
        self.stage(
            Stage::Matching,
            StageState::Running {
                done: 0,
                total: rounds + 1,
            },
        );
        let mut matches = self
            .match_all(&items, &episode_inputs, &disc_order, &positions)
            .await?;
        // Published now, so a job cancelled during further listening keeps the global
        // assignment rather than each file's own interim match.
        self.publish_all(&matches);
        for round in 0..rounds {
            let uncertain = needs_more_listening(&matches, &ctx.config.matching);
            let mut listened = false;
            for item in items.iter_mut().filter(|i| uncertain.contains(&i.file.id)) {
                // Embedded subtitles replace the transcript as dialogue, so hearing more of the
                // file could change only the title hook, which the subtitles are searched for too.
                if item.failed || item.embedded.is_some() {
                    continue;
                }
                // A file found to be mostly music is heard further without voice activity
                // detection, which would cut sung words.
                item.music_heavy |= mostly_music(&item.transcript);
                let more = escalation_windows(
                    item.duration_s,
                    &item.transcript.windows,
                    policy,
                    round + 1 == rounds,
                );
                if more.is_empty() {
                    continue;
                }
                self.file_status(&item.file.id, FileStatus::Listening, None, None);
                for window in more {
                    match self.listen(&mut listener, item, window).await {
                        Ok(()) => listened = true,
                        Err(e) if is_cancelled(&e) => return Err(e),
                        Err(e) => {
                            tracing::warn!(file = %item.file.id.0, error = %e, "could not listen further");
                            break;
                        }
                    }
                }
                self.update_record(|r| {
                    r.transcripts
                        .insert(item.file.id.clone(), item.transcript.clone());
                });
                self.file_status(&item.file.id, FileStatus::Matching, None, None);
            }
            self.stage(
                Stage::Matching,
                StageState::Running {
                    done: round + 1,
                    total: rounds + 1,
                },
            );
            if !listened {
                break;
            }
            matches = self
                .match_all(&items, &episode_inputs, &disc_order, &positions)
                .await?;
            self.publish_all(&matches);
        }
        for (item, m) in items.iter().zip(matches) {
            let best = m.candidates.first().map(|c| c.title.clone());
            let verdict = m.confidence.verdict;
            self.publish(m);
            let status = if item.unusable() {
                FileStatus::Failed
            } else {
                FileStatus::Done
            };
            self.file_status(&item.file.id, status, best, Some(verdict));
        }
        self.stage(Stage::Matching, StageState::Done);
        self.update_record(|r| r.results.complete = true);
        Ok(())
    }

    async fn episode_list(&self) -> Result<Vec<Episode>, CoreError> {
        let request = &self.ctx.request;
        let mut episodes = self
            .ctx
            .services
            .catalog
            .episodes(&request.show.show_ref, request.ordering)
            .await?;
        if let Some(seasons) = request.seasons.as_ref().filter(|s| !s.is_empty()) {
            episodes.retain(|e| seasons.contains(&e.key.season));
        }
        if episodes.is_empty() {
            return Err(CoreError::Invalid(format!(
                "No episodes of {} were found for the chosen seasons.",
                request.show.name
            )));
        }
        Ok(episodes)
    }

    async fn reference_texts(&self, episodes: &[Episode]) -> Result<Vec<ReferenceText>, CoreError> {
        let ctx = self.ctx;
        let total = episodes.len() as u32;
        self.stage(Stage::Subtitles, StageState::Running { done: 0, total });
        let sink = Arc::clone(&ctx.sink);
        let job_id = ctx.job_id.clone();
        let progress = move |done: u32, total: u32| {
            sink.job_event(JobEvent::Stage {
                job_id: job_id.clone(),
                stage: Stage::Subtitles,
                state: StageState::Running { done, total },
            });
        };
        match ctx
            .services
            .catalog
            .reference_texts(
                &ctx.request.show,
                episodes,
                &self.language,
                &progress,
                &ctx.cancel,
            )
            .await
        {
            Ok(texts) => {
                self.stage(Stage::Subtitles, StageState::Done);
                Ok(texts)
            }
            Err(mi_sources::SourceError::Cancelled) => Err(CoreError::Cancelled),
            Err(e) => {
                tracing::warn!(error = %e, "reference text unavailable");
                self.stage(
                    Stage::Subtitles,
                    StageState::Failed {
                        message: e.to_string(),
                    },
                );
                Ok(Vec::new())
            }
        }
    }

    /// Fingerprints the play-all and each file and locates the files inside it.
    async fn disc_order(
        &self,
        play_all: Option<&MediaFile>,
        candidates: &[MediaFile],
    ) -> Result<Option<DiscOrder>, CoreError> {
        let Some(play_all) = play_all.filter(|_| candidates.len() >= 2) else {
            self.stage(Stage::DiscOrder, StageState::Skipped);
            return Ok(None);
        };
        let total = candidates.len() as u32 + 1;
        self.stage(Stage::DiscOrder, StageState::Running { done: 0, total });
        let haystack = match self.fingerprint(play_all).await {
            Ok(f) => Arc::new(f),
            Err(e) if is_cancelled(&e) => return Err(e),
            Err(e) => {
                tracing::warn!(error = %e, "could not read the play-all");
                self.stage(
                    Stage::DiscOrder,
                    StageState::Failed {
                        message: format!("Couldn't read the play-all title: {e}"),
                    },
                );
                return Ok(None);
            }
        };
        let mut alignments = Vec::with_capacity(candidates.len());
        for (i, file) in candidates.iter().enumerate() {
            self.stage(
                Stage::DiscOrder,
                StageState::Running {
                    done: i as u32 + 1,
                    total,
                },
            );
            let alignment = match self.fingerprint(file).await {
                Ok(needle) => {
                    let haystack = Arc::clone(&haystack);
                    blocking(move || locate(&needle, &haystack)).await?
                }
                Err(e) if is_cancelled(&e) => return Err(e),
                Err(e) => {
                    tracing::warn!(file = %file.id.0, error = %e, "could not fingerprint a file");
                    None
                }
            };
            alignments.push((file.id.clone(), alignment));
        }
        let chapters = play_all
            .probe
            .as_ref()
            .map(|p| p.chapters.clone())
            .unwrap_or_default();
        let order = derive_disc_order(&alignments, &chapters);
        tracing::info!(
            located = order.positions.len(),
            trustworthy = order.trustworthy,
            problem = ?order.problem,
            "disc order"
        );
        self.stage(Stage::DiscOrder, StageState::Done);
        Ok(Some(order))
    }

    async fn fingerprint(&self, file: &MediaFile) -> Result<mi_match::Fingerprint, CoreError> {
        let media = Arc::clone(&self.ctx.services.media);
        let file = file.clone();
        let language = self.language.clone();
        let cancel = self.ctx.cancel.clone();
        blocking(move || {
            let mut builder = FingerprintBuilder::new(mi_media::SAMPLE_RATE);
            media.stream(&file, &language, &cancel, &mut |samples| {
                builder.push(samples);
                true
            })?;
            Ok::<_, CoreError>(builder.finish())
        })
        .await?
    }

    /// The dialogue of the first text subtitle stream in the job's language, when it has enough
    /// words to be useful. Failures are logged and ignored.
    async fn embedded_dialogue(&self, file: &MediaFile) -> Result<Option<String>, CoreError> {
        let Some(stream) = file.probe.as_ref().and_then(|p| {
            p.subtitle_streams.iter().find(|s| {
                s.is_text
                    && s.language
                        .as_deref()
                        .is_none_or(|l| mi_media::languages_match(l, &self.language))
            })
        }) else {
            return Ok(None);
        };
        let media = Arc::clone(&self.ctx.services.media);
        let (file, index) = (file.clone(), stream.index);
        let cancel = self.ctx.cancel.clone();
        match blocking(move || media.text_subtitles(&file, index, &cancel)).await? {
            Ok(srt) => {
                let text = mi_sources::embedded::dialogue_from_srt(&srt);
                let words = text.split_whitespace().count();
                Ok((words >= self.ctx.config.min_embedded_words).then_some(text))
            }
            Err(mi_media::MediaError::Cancelled) => Err(CoreError::Cancelled),
            Err(e) => {
                tracing::warn!(error = %e, "could not read an embedded subtitle stream");
                Ok(None)
            }
        }
    }

    /// Decodes and transcribes one window of `item` and adds it to its transcript.
    async fn listen(
        &self,
        listener: &mut Box<dyn Listener>,
        item: &mut Item,
        window: SampleWindow,
    ) -> Result<(), CoreError> {
        let media = Arc::clone(&self.ctx.services.media);
        let file = item.file.clone();
        let language = self.language.clone();
        let cancel = self.ctx.cancel.clone();
        let mut options =
            DecodeOptions::for_file(item.duration_s, item.music_heavy, &self.ctx.config.sampling);
        options.language = language.clone();
        // Hand the listener to the blocking thread and take it back afterwards.
        let mut taken: Box<dyn Listener> = std::mem::replace(listener, Box::new(Unavailable));
        let (returned, result) = blocking(move || {
            let result = media
                .decode(&file, Some(window), &language, &cancel)
                .map_err(CoreError::from)
                .and_then(|samples| {
                    taken
                        .listen(&file, window, &samples, &options, &cancel)
                        .map_err(CoreError::from)
                });
            (taken, result)
        })
        .await?;
        *listener = returned;
        add_window(&mut item.transcript, window, result?, &self.filter);
        Ok(())
    }

    fn file_input(&self, item: &Item, position: Option<PlayAllPosition>) -> FileInput {
        let policy = &self.ctx.config.sampling;
        let covered = mi_transcribe::sampling::covered_seconds(&item.transcript.windows);
        let sampled = item.duration_s > policy.whole_file_max_s
            && covered + mi_transcribe::sampling::MIN_GAP_S < item.duration_s;
        FileInput {
            file_id: item.file.id.clone(),
            duration_s: item.duration_s,
            transcript: item.transcript.matching_text(),
            embedded_text: item.embedded.clone(),
            mostly_music: item.music_heavy || mostly_music(&item.transcript),
            play_all_position: position,
            sampled_windows: sampled.then_some(item.transcript.windows.len() as u32),
        }
    }

    async fn match_all(
        &self,
        items: &[Item],
        episodes: &[EpisodeInput],
        disc_order: &Option<DiscOrder>,
        positions: &HashMap<FileId, PlayAllPosition>,
    ) -> Result<Vec<FileMatch>, CoreError> {
        let input = MatchInput {
            files: items
                .iter()
                .map(|i| self.file_input(i, positions.get(&i.file.id).cloned()))
                .collect(),
            episodes: episodes.to_vec(),
            disc_order: disc_order.clone(),
        };
        let config = self.ctx.config.matching.clone();
        let cancel = self.ctx.cancel.clone();
        let outcome = blocking(move || match_with_outcome(&input, &config, &cancel)).await??;
        tracing::info!(disc_order = ?outcome.disc_order, "matched");
        Ok(outcome.matches)
    }
}

/// Whether the show is sung: at least half of its episodes have lyrics as reference text. Voice
/// activity detection would cut sung words, and music is expected rather than a sign of a bonus
/// feature. Episodes are counted rather than texts, so one episode whose title matches a song
/// does not make a whole show musical.
fn show_is_musical(episodes: &[Episode], texts: &[ReferenceText]) -> bool {
    let with_lyrics: std::collections::HashSet<_> = texts
        .iter()
        .filter(|t| t.kind == TextKind::Lyrics)
        .map(|t| t.episode)
        .collect();
    !with_lyrics.is_empty() && with_lyrics.len() * 2 >= episodes.len()
}

/// True when most of what the model produced was marked as sound rather than speech (music
/// notes, sound descriptions, no-speech segments).
fn mostly_music(transcript: &Transcript) -> bool {
    let total = transcript.segments.len();
    if total == 0 {
        return false;
    }
    let musical = transcript
        .segments
        .iter()
        .filter(|s| s.filtered == Some(FilterReason::NoSpeech) || s.text.contains('♪'))
        .count();
    musical * 2 > total
}

/// Groups reference texts by episode, in episode order.
pub fn episode_inputs(episodes: &[Episode], texts: Vec<ReferenceText>) -> Vec<EpisodeInput> {
    let mut by_key: HashMap<(u32, u32), Vec<ReferenceText>> = HashMap::new();
    for t in texts {
        by_key
            .entry((t.episode.season, t.episode.number))
            .or_default()
            .push(t);
    }
    episodes
        .iter()
        .map(|e| EpisodeInput {
            episode: e.clone(),
            texts: by_key
                .remove(&(e.key.season, e.key.number))
                .unwrap_or_default(),
        })
        .collect()
}

/// Stands in for the listener while it works on a blocking thread.
struct Unavailable;

impl Listener for Unavailable {
    fn listen(
        &mut self,
        _file: &MediaFile,
        _window: SampleWindow,
        _samples: &[f32],
        _options: &DecodeOptions,
        _cancel: &CancelFlag,
    ) -> mi_transcribe::Result<Vec<mi_types::Segment>> {
        Err(mi_transcribe::TranscribeError::Engine(
            "the speech model is busy".to_owned(),
        ))
    }

    fn accelerator(&self) -> mi_types::Accelerator {
        mi_types::Accelerator::Cpu
    }
}

/// The record a job starts with.
pub fn new_record(job_id: &JobId, request: &JobRequest, model: SpeechModel) -> JobRecord {
    JobRecord {
        results: mi_types::JobResults {
            job_id: job_id.clone(),
            request: request.clone(),
            episodes: Vec::new(),
            matches: Vec::new(),
            model,
            complete: false,
        },
        files: Vec::new(),
        transcripts: HashMap::new(),
        saved: false,
        finished_at_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_types::{EpisodeKey, EpisodeOrdering, ProviderId, ShowRef};

    fn episode(number: u32) -> Episode {
        Episode {
            show_ref: ShowRef {
                provider: ProviderId::Tvmaze,
                id: "1".into(),
            },
            ordering: EpisodeOrdering::Aired,
            key: EpisodeKey { season: 1, number },
            title: format!("Episode {number}"),
            runtime_s: Some(300.0),
            airdate: None,
            summary: None,
            provider_episode_id: number.to_string(),
        }
    }

    fn text(number: u32, kind: TextKind) -> ReferenceText {
        ReferenceText {
            show_ref: episode(number).show_ref,
            ordering: EpisodeOrdering::Aired,
            episode: EpisodeKey { season: 1, number },
            kind,
            provider: ProviderId::Lrclib,
            provider_ref: number.to_string(),
            text: "words".into(),
            language: "en".into(),
            fetched_at_ms: 0,
        }
    }

    #[test]
    fn a_show_is_musical_when_half_of_its_episodes_have_lyrics() {
        let episodes: Vec<Episode> = (1..=10).map(episode).collect();
        // One episode titled like a song, and no subtitles at all (no SubDL key).
        assert!(!show_is_musical(&episodes, &[text(3, TextKind::Lyrics)]));
        let half: Vec<ReferenceText> = (1..=5).map(|n| text(n, TextKind::Lyrics)).collect();
        assert!(show_is_musical(&episodes, &half));
        // Two lyrics records for one episode count once.
        let one = [text(3, TextKind::Lyrics), text(3, TextKind::Lyrics)];
        assert!(!show_is_musical(&episodes[..3], &one));
        assert!(!show_is_musical(&episodes, &[text(1, TextKind::Subtitles)]));
    }
}
