//! The matching entry point.
//!
//! [`match_files`] runs these steps:
//!
//! 1. **Content signals.** Every file is scored against every episode for dialogue, title hook
//!    and length ([`score_all`] returns the resulting matrix).
//! 2. **Disc order check.** When a trustworthy disc order is given, the files the content
//!    identifies confidently by themselves become *anchors*. If the play-all's order contradicts
//!    the anchors' episode order, the play-all is shuffled and ignored.
//! 3. **Disc order signal.** For each located file, episodes that fit between the neighbouring
//!    anchors score well, and the episode that continues the anchors' sequence scores best.
//! 4. **Assignment.** Located files are assigned by order-preserving dynamic programming, the
//!    rest by Hungarian assignment with "no episode" options.
//! 5. **Confidence.** Each file's margin over its runner-up gives Confident, Check or Extra.

use mi_types::{
    CancelFlag, Candidate, Episode, EpisodeKey, Evidence, EvidenceNote, FileId, FileMatch,
    PlayAllPosition, QuotePart, ReferenceText, Signals, Suggestion, TextKind, Verdict,
};

use crate::align::{DiscOrder, DiscOrderProblem};
use crate::assign::{hungarian, order_preserving};
use crate::confidence::classify;
use crate::duration::duration_fit;
use crate::quote::{marked_quote, overlap_quotes};
use crate::text::{DialogueScore, SummaryIndex, SummaryScore, TfIdfIndex};
use crate::title_hook::{HeardText, TitleHook, find_title};
use crate::{MatchError, Result};

/// One file to identify.
#[derive(Debug, Clone, PartialEq)]
pub struct FileInput {
    /// The file.
    pub file_id: FileId,
    /// Its duration, seconds.
    pub duration_s: f64,
    /// Unfiltered transcript text (`Transcript::matching_text`).
    pub transcript: String,
    /// Dialogue from an embedded text subtitle stream, when the file has one. It is used instead
    /// of the transcript for dialogue, because it is the file's exact dialogue; the title is
    /// searched in both.
    pub embedded_text: Option<String>,
    /// True when most of the audio is music (few speech segments over the sampled time).
    pub mostly_music: bool,
    /// Where the file is in the play-all, when located. Used for display when no
    /// [`MatchInput::disc_order`] is given; a disc order's own positions take precedence.
    pub play_all_position: Option<PlayAllPosition>,
    /// Number of windows transcribed when only samples of a long file were transcribed; `None`
    /// when the file was transcribed whole.
    pub sampled_windows: Option<u32>,
}

/// One candidate episode with its reference texts.
#[derive(Debug, Clone, PartialEq)]
pub struct EpisodeInput {
    /// The episode.
    pub episode: Episode,
    /// Its reference texts (may be empty). Subtitles and lyrics are compared as dialogue;
    /// summaries (or, without any, `episode.summary`) only when the episode has no dialogue text.
    pub texts: Vec<ReferenceText>,
}

/// Everything matching needs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MatchInput {
    /// Files to identify (the play-all excluded).
    pub files: Vec<FileInput>,
    /// Candidate episodes, in episode order.
    pub episodes: Vec<EpisodeInput>,
    /// Disc order, when a play-all was found.
    pub disc_order: Option<DiscOrder>,
}

/// Relative weights of the signals in the combined score. Missing signals are left out and the
/// remaining weights renormalised.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalWeights {
    /// Dialogue similarity.
    pub dialogue: f32,
    /// Title hook.
    pub title_hook: f32,
    /// Duration fit.
    pub duration: f32,
    /// Disc order fit.
    pub disc_order: f32,
}

/// Matching thresholds and weights.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchConfig {
    /// Signal weights.
    pub weights: SignalWeights,
    /// Minimum lead over the runner-up for `Confident`.
    pub confident_margin: f32,
    /// Score of the "no episode" option; a file whose best episode scores below this becomes an
    /// extra.
    pub no_episode_score: f32,
    /// Penalty for leaving a file unassigned in the order-preserving assignment.
    pub skip_file_penalty: f32,
    /// Number of candidates kept per file for the Review dropdown.
    pub max_candidates: usize,
    /// Dialogue strength below which the combined score is scaled down in proportion, when the
    /// episode has dialogue text and its title was not clearly heard: length and disc order
    /// cannot make a file an episode when nothing heard resembles it.
    pub identity_floor: f32,
    /// Fewest heard content words (words other than common function words such as "the" and
    /// sung or hesitation sounds such as "la", "oh", "mm") for the dialogue signal to be
    /// measured; below it, too little was heard for its absence to mean anything.
    pub min_heard_words: usize,
    /// Factor applied to the title hook weight for mostly-music files, whose sung words are
    /// transcribed less reliably than their repeated title.
    pub music_title_boost: f32,
    /// Smallest share of anchors that must already be in episode order along the play-all (the
    /// length of the longest increasing subsequence of their episodes divided by their number)
    /// for the order to be used.
    pub min_order_agreement: f32,
}

impl Default for MatchConfig {
    fn default() -> Self {
        Self {
            weights: SignalWeights {
                dialogue: 0.55,
                title_hook: 0.15,
                duration: 0.10,
                disc_order: 0.20,
            },
            confident_margin: 0.15,
            no_episode_score: 0.25,
            skip_file_penalty: 0.10,
            max_candidates: 5,
            identity_floor: 0.3,
            min_heard_words: 8,
            music_title_boost: 2.0,
            min_order_agreement: 0.75,
        }
    }
}

/// Scores for every file/episode pair.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScoreMatrix {
    /// Row labels.
    pub files: Vec<FileId>,
    /// Column labels.
    pub episodes: Vec<EpisodeKey>,
    /// `cells[file][episode]`, each with its signals and evidence.
    pub cells: Vec<Vec<Candidate>>,
}

/// What matching did with the disc order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscOrderUse {
    /// No disc order was given.
    Absent,
    /// The disc order was used for the disc order signal and order-preserving assignment.
    Used,
    /// The disc order was given but ignored, for this reason.
    Ignored(DiscOrderProblem),
}

/// Scores, assignment and verdicts of one matching run, with what happened to the disc order.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchOutcome {
    /// One result per input file, in input order.
    pub matches: Vec<FileMatch>,
    /// What was done with the disc order.
    pub disc_order: DiscOrderUse,
}

/// Scores every file against every episode, including the disc order signal when the disc order
/// is used (see the module documentation).
pub fn score_all(
    input: &MatchInput,
    config: &MatchConfig,
    cancel: &CancelFlag,
) -> crate::Result<ScoreMatrix> {
    Ok(evaluate(input, config, cancel)?.matrix)
}

/// Scores, assigns and classifies: one [`FileMatch`] per input file, in input order. Uses
/// order-preserving assignment when `input.disc_order` is trustworthy and agrees with the
/// dialogue, otherwise Hungarian.
pub fn match_files(
    input: &MatchInput,
    config: &MatchConfig,
    cancel: &CancelFlag,
) -> crate::Result<Vec<FileMatch>> {
    Ok(match_with_outcome(input, config, cancel)?.matches)
}

/// [`match_files`], also reporting what was done with the disc order.
pub fn match_with_outcome(
    input: &MatchInput,
    config: &MatchConfig,
    cancel: &CancelFlag,
) -> crate::Result<MatchOutcome> {
    let eval = evaluate(input, config, cancel)?;
    let no_ep = config.no_episode_score;
    let scores = &eval.scores;
    let n_eps = input.episodes.len();

    let mut assigned: Vec<Option<usize>> = vec![None; input.files.len()];
    if let Some(order) = &eval.located_in_order {
        let rows: Vec<Vec<f32>> = order
            .iter()
            .map(|&f| scores[f].iter().map(|s| s - no_ep).collect())
            .collect();
        for (k, a) in order_preserving(&rows, config.skip_file_penalty)
            .into_iter()
            .enumerate()
        {
            assigned[order[k]] = a;
        }
        let used: Vec<bool> = (0..n_eps).map(|e| assigned.contains(&Some(e))).collect();
        let rest: Vec<usize> = (0..input.files.len())
            .filter(|f| !order.contains(f))
            .collect();
        let rows: Vec<Vec<f32>> = rest
            .iter()
            .map(|&f| {
                (0..n_eps)
                    .map(|e| {
                        if used[e] {
                            f32::NEG_INFINITY
                        } else {
                            scores[f][e]
                        }
                    })
                    .collect()
            })
            .collect();
        for (k, a) in hungarian(&rows, no_ep).into_iter().enumerate() {
            assigned[rest[k]] = a;
        }
    } else {
        assigned = hungarian(scores, no_ep);
    }

    let matches = input
        .files
        .iter()
        .enumerate()
        .map(|(f, file)| {
            let row = &scores[f];
            let best_other = |except: Option<usize>| {
                row.iter()
                    .enumerate()
                    .filter(|(e, _)| Some(*e) != except)
                    .map(|(_, s)| *s)
                    .fold(f32::NEG_INFINITY, f32::max)
            };
            let (suggestion, confidence) = match assigned[f] {
                Some(e) => {
                    let mut c = classify(row[e], best_other(Some(e)).max(no_ep), false, config);
                    let s = &eval.matrix.cells[f][e].evidence.signals;
                    if c.verdict == Verdict::Confident && !has_identity_support(s, config) {
                        c.verdict = Verdict::Check;
                    }
                    (
                        Suggestion::Episode {
                            episode: input.episodes[e].episode.key,
                        },
                        c,
                    )
                }
                None => (
                    Suggestion::NotAnEpisode,
                    classify(no_ep, best_other(None).max(0.0), true, config),
                ),
            };
            let mut order: Vec<usize> = (0..n_eps).collect();
            order.sort_by(|&a, &b| row[b].total_cmp(&row[a]).then(a.cmp(&b)));
            order.truncate(config.max_candidates);
            if let Some(e) = assigned[f]
                && !order.contains(&e)
            {
                order.pop();
                order.push(e);
                order.sort_by(|&a, &b| row[b].total_cmp(&row[a]).then(a.cmp(&b)));
            }
            FileMatch {
                file_id: file.file_id.clone(),
                suggestion,
                confidence,
                candidates: order
                    .into_iter()
                    .map(|e| eval.matrix.cells[f][e].clone())
                    .collect(),
            }
        })
        .collect();
    Ok(MatchOutcome {
        matches,
        disc_order: eval.disc_order,
    })
}

/// Whether what was heard supports an episode by itself: dialogue at least at the identity floor,
/// or the title clearly heard. A suggestion without such support rests on length and disc order
/// alone, which place a file but cannot recognise it, so it is never `Confident`.
fn has_identity_support(signals: &Signals, config: &MatchConfig) -> bool {
    signals.dialogue.is_some_and(|d| d >= config.identity_floor)
        || signals.title_hook.is_some_and(|t| t >= TITLE_HEARD)
}

/// Everything computed before assignment.
struct Evaluation {
    matrix: ScoreMatrix,
    scores: Vec<Vec<f32>>,
    disc_order: DiscOrderUse,
    /// File indices in play-all order when the disc order is used.
    located_in_order: Option<Vec<usize>>,
}

/// What the dialogue signal of one cell was computed from, for the quotes.
enum TextDetail {
    Dialogue(usize, DialogueScore),
    Summary(usize, SummaryScore),
    None,
}

/// The heard side of one file.
struct Heard {
    query: crate::text::Query,
    /// The transcript, searched for titles too when embedded subtitles replaced it as dialogue.
    transcript_for_titles: Option<(crate::text::PreparedText, HeardText)>,
    words: usize,
}

fn dialogue_text(ep: &EpisodeInput) -> Option<String> {
    let parts: Vec<&str> = ep
        .texts
        .iter()
        .filter(|t| matches!(t.kind, TextKind::Subtitles | TextKind::Lyrics))
        .map(|t| t.text.trim())
        .filter(|t| !t.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn summary_text(ep: &EpisodeInput) -> Option<String> {
    let parts: Vec<&str> = ep
        .texts
        .iter()
        .filter(|t| t.kind == TextKind::Summary)
        .map(|t| t.text.trim())
        .filter(|t| !t.is_empty())
        .collect();
    if !parts.is_empty() {
        return Some(parts.join("\n"));
    }
    ep.episode
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Weighted mean of the measured signals. With `gate`, the mean is scaled down when the
/// dialogue (compared with real dialogue text) and the title hook are both weak (see
/// [`MatchConfig::identity_floor`]).
fn combine(signals: &Signals, gate: bool, mostly_music: bool, config: &MatchConfig) -> f32 {
    let w = &config.weights;
    let title_weight = if mostly_music {
        w.title_hook * config.music_title_boost
    } else {
        w.title_hook
    };
    let parts = [
        (signals.dialogue, w.dialogue),
        (signals.title_hook, title_weight),
        (signals.duration, w.duration),
        (signals.disc_order, w.disc_order),
    ];
    let (mut num, mut den) = (0.0f32, 0.0f32);
    for (value, weight) in parts {
        if let Some(v) = value {
            num += weight * v;
            den += weight;
        }
    }
    if den <= 0.0 {
        return 0.0;
    }
    let mean = num / den;
    let gate = match (gate, signals.dialogue) {
        (true, Some(d)) if config.identity_floor > 0.0 => {
            let title = signals
                .title_hook
                .filter(|t| *t >= TITLE_HEARD)
                .unwrap_or(0.0);
            let identity = d.max(title);
            (identity / config.identity_floor).min(1.0)
        }
        _ => 1.0,
    };
    (mean * gate).clamp(0.0, 1.0)
}

/// Length of the longest strictly increasing subsequence.
fn longest_increasing(values: &[usize]) -> usize {
    let mut tails: Vec<usize> = Vec::new();
    for &v in values {
        match tails.binary_search(&v) {
            Ok(_) => {}
            Err(i) if i == tails.len() => tails.push(v),
            Err(i) => tails[i] = v,
        }
    }
    tails.len()
}

/// Disc order signal for the file at play-all `rank` and episode `e`, from the anchors
/// (`(rank, episode)`, sorted by rank) other than the file itself.
///
/// Episodes that cannot lie between the nearest anchors before and after the file score 0.
/// Among those that can, the episode that continues an anchor's sequence (as many places after
/// the earlier anchor's episode as the file is after that anchor along the play-all, or as many
/// before the later anchor's) scores 1.0 and the others 0.6. `None` when
/// there are no anchors, because then the order says nothing about any particular episode.
fn order_signal(rank: usize, e: usize, anchors: &[(usize, usize)]) -> Option<f32> {
    let others = anchors.iter().filter(|(r, _)| *r != rank);
    let prev = others
        .clone()
        .filter(|(r, _)| *r < rank)
        .max_by_key(|(r, _)| *r);
    let next = others.filter(|(r, _)| *r > rank).min_by_key(|(r, _)| *r);
    if prev.is_none() && next.is_none() {
        return None;
    }
    let after_prev = prev.is_none_or(|(_, pe)| e > *pe);
    let before_next = next.is_none_or(|(_, ne)| e < *ne);
    if !(after_prev && before_next) {
        return Some(0.0);
    }
    let expected_from_prev = prev.map(|(pr, pe)| pe + (rank - pr));
    let expected_from_next = next.and_then(|(nr, ne)| ne.checked_sub(nr - rank));
    if expected_from_prev == Some(e) || expected_from_next == Some(e) {
        Some(1.0)
    } else {
        Some(0.6)
    }
}

fn evaluate(input: &MatchInput, config: &MatchConfig, cancel: &CancelFlag) -> Result<Evaluation> {
    // Validate.
    let mut ids: Vec<&FileId> = input.files.iter().map(|f| &f.file_id).collect();
    ids.sort();
    if let Some(w) = ids.windows(2).find(|w| w[0] == w[1]) {
        return Err(MatchError::InvalidInput(format!(
            "file {} is listed twice",
            w[0].0
        )));
    }
    if let Some(order) = &input.disc_order {
        for (id, _) in &order.positions {
            if !input.files.iter().any(|f| &f.file_id == id) {
                return Err(MatchError::InvalidInput(format!(
                    "disc order names unknown file {}",
                    id.0
                )));
            }
        }
    }

    // Reference texts.
    let mut dialogue_doc: Vec<Option<usize>> = Vec::new();
    let mut summary_doc: Vec<Option<usize>> = Vec::new();
    let mut dialogue_docs: Vec<(String, String)> = Vec::new();
    let mut summaries: Vec<String> = Vec::new();
    for ep in &input.episodes {
        let key = format!("S{}E{}", ep.episode.key.season, ep.episode.key.number);
        if let Some(text) = dialogue_text(ep) {
            dialogue_doc.push(Some(dialogue_docs.len()));
            summary_doc.push(None);
            dialogue_docs.push((key, text));
        } else if let Some(text) = summary_text(ep) {
            dialogue_doc.push(None);
            summary_doc.push(Some(summaries.len()));
            summaries.push(text);
        } else {
            dialogue_doc.push(None);
            summary_doc.push(None);
        }
    }
    let index = TfIdfIndex::build(&dialogue_docs);
    let summary_index = SummaryIndex::build(&summaries);

    // Positions in the play-all, for evidence.
    let positions: Vec<Option<PlayAllPosition>> = input
        .files
        .iter()
        .map(|f| match &input.disc_order {
            Some(order) => order
                .positions
                .iter()
                .find(|(id, _)| id == &f.file_id)
                .map(|(_, p)| p.clone()),
            None => f.play_all_position.clone(),
        })
        .collect();

    // Content signals.
    struct CellWork {
        signals: Signals,
        detail: TextDetail,
        hook: Option<(TitleHook, bool)>,
    }
    let mut heard_all: Vec<Heard> = Vec::with_capacity(input.files.len());
    let mut work: Vec<Vec<CellWork>> = Vec::with_capacity(input.files.len());
    for file in &input.files {
        if cancel.is_cancelled() {
            return Err(MatchError::Cancelled);
        }
        let embedded = file
            .embedded_text
            .as_deref()
            .filter(|t| !t.trim().is_empty());
        let query = index.query(embedded.unwrap_or(&file.transcript));
        let for_titles = HeardText::from_prepared(&query.prepared);
        let transcript_for_titles =
            embedded
                .filter(|_| !file.transcript.trim().is_empty())
                .map(|_| {
                    let p = crate::text::PreparedText::standalone(&file.transcript);
                    let h = HeardText::from_prepared(&p);
                    (p, h)
                });
        let words = query.prepared.len();
        let content_words = query
            .prepared
            .tokens
            .words
            .iter()
            .filter(|w| crate::normalize::is_content_word(w))
            .count();
        let enough = content_words >= config.min_heard_words;
        let mut row = Vec::with_capacity(input.episodes.len());
        for (e, ep) in input.episodes.iter().enumerate() {
            let detail = if !enough {
                TextDetail::None
            } else if let Some(d) = dialogue_doc[e] {
                TextDetail::Dialogue(d, index.score(&query, d))
            } else if let Some(s) = summary_doc[e] {
                TextDetail::Summary(s, summary_index.score(&query.prepared.tokens, s))
            } else {
                TextDetail::None
            };
            let dialogue = match &detail {
                TextDetail::Dialogue(_, d) => Some(d.similarity),
                TextDetail::Summary(_, s) => Some(s.similarity),
                TextDetail::None => None,
            };
            let main_hook = find_title(&ep.episode.title, &for_titles).map(|h| (h, false));
            let transcript_hook = transcript_for_titles
                .as_ref()
                .and_then(|(_, h)| find_title(&ep.episode.title, h))
                .map(|h| (h, true));
            let hook = match (main_hook, transcript_hook) {
                (Some(a), Some(b)) => Some(if b.0.score > a.0.score { b } else { a }),
                (a, b) => a.or(b),
            };
            let hook_score = hook.as_ref().map_or(0.0, |(h, _)| h.score);
            let title_hook = if words == 0 && transcript_for_titles.is_none() {
                None
            } else if enough || hook_score > 0.0 {
                Some(hook_score)
            } else {
                None
            };
            row.push(CellWork {
                signals: Signals {
                    dialogue,
                    title_hook,
                    duration: duration_fit(file.duration_s, ep.episode.runtime_s),
                    disc_order: None,
                },
                detail,
                hook,
            });
        }
        heard_all.push(Heard {
            query,
            transcript_for_titles,
            words,
        });
        work.push(row);
    }

    let content_scores: Vec<Vec<f32>> = input
        .files
        .iter()
        .zip(&work)
        .map(|(file, row)| {
            row.iter()
                .enumerate()
                .map(|(e, c)| {
                    combine(
                        &c.signals,
                        dialogue_doc[e].is_some(),
                        file.mostly_music,
                        config,
                    )
                })
                .collect()
        })
        .collect();

    // Disc order: decide whether to use it.
    let mut disc_use = DiscOrderUse::Absent;
    let mut located_in_order: Option<Vec<usize>> = None;
    if let Some(order) = &input.disc_order {
        let mut ranked: Vec<(u32, usize)> = order
            .positions
            .iter()
            .filter_map(|(id, p)| {
                input
                    .files
                    .iter()
                    .position(|f| &f.file_id == id)
                    .map(|f| (p.order_index, f))
            })
            .collect();
        ranked.sort();
        let located: Vec<usize> = ranked.into_iter().map(|(_, f)| f).collect();
        disc_use = if !order.trustworthy {
            DiscOrderUse::Ignored(order.problem.unwrap_or(DiscOrderProblem::TooFewLocated))
        } else if located.len() < 2 {
            DiscOrderUse::Ignored(DiscOrderProblem::TooFewLocated)
        } else {
            // Anchors: located files the content alone identifies confidently.
            let provisional = hungarian(&content_scores, config.no_episode_score);
            let anchors: Vec<(usize, usize)> = located
                .iter()
                .enumerate()
                .filter_map(|(rank, &f)| {
                    let e = provisional[f]?;
                    let row = &content_scores[f];
                    let runner_up = row
                        .iter()
                        .enumerate()
                        .filter(|(o, _)| *o != e)
                        .map(|(_, s)| *s)
                        .fold(config.no_episode_score, f32::max);
                    (row[e] - runner_up >= config.confident_margin).then_some((rank, e))
                })
                .collect();
            let episodes: Vec<usize> = anchors.iter().map(|(_, e)| *e).collect();
            let agreement = if anchors.is_empty() {
                1.0
            } else {
                longest_increasing(&episodes) as f32 / anchors.len() as f32
            };
            if anchors.len() >= 2 && agreement < config.min_order_agreement {
                DiscOrderUse::Ignored(DiscOrderProblem::ShuffledAgainstContent)
            } else {
                for (rank, &f) in located.iter().enumerate() {
                    for (e, cell) in work[f].iter_mut().enumerate() {
                        cell.signals.disc_order = order_signal(rank, e, &anchors);
                    }
                }
                located_in_order = Some(located);
                DiscOrderUse::Used
            }
        };
    }

    // Final scores and evidence.
    let mut scores = Vec::with_capacity(input.files.len());
    let mut cells = Vec::with_capacity(input.files.len());
    for (f, file) in input.files.iter().enumerate() {
        let heard = &heard_all[f];
        let mut row_scores = Vec::with_capacity(input.episodes.len());
        let mut row_cells = Vec::with_capacity(input.episodes.len());
        for (e, ep) in input.episodes.iter().enumerate() {
            let cell = &work[f][e];
            // A file found inside a used play-all belongs to the disc's main sequence, which holds
            // episodes, so weak dialogue does not scale its score down.
            let in_play_all = located_in_order.as_ref().is_some_and(|l| l.contains(&f));
            let score = combine(
                &cell.signals,
                dialogue_doc[e].is_some() && !in_play_all,
                file.mostly_music,
                config,
            );
            let (heard_quote, reference_quote) = quotes(cell, heard, &index, &summary_index, ep);
            let notes = notes(
                &cell.signals,
                file,
                heard,
                dialogue_doc[e].is_some() || summary_doc[e].is_some(),
                positions[f].as_ref(),
                disc_use,
            );
            row_scores.push(score);
            row_cells.push(Candidate {
                episode: ep.episode.key,
                title: ep.episode.title.clone(),
                score,
                evidence: Evidence {
                    signals: cell.signals,
                    heard: heard_quote,
                    reference: reference_quote,
                    play_all_position: positions[f].clone(),
                    notes,
                },
            });
        }
        scores.push(row_scores);
        cells.push(row_cells);
    }

    fn quotes(
        cell: &CellWork,
        heard: &Heard,
        index: &TfIdfIndex,
        summaries: &SummaryIndex,
        ep: &EpisodeInput,
    ) -> (Vec<QuotePart>, Vec<QuotePart>) {
        match &cell.detail {
            TextDetail::Dialogue(d, score)
                if score
                    .best_overlap
                    .as_ref()
                    .is_some_and(|o| o.ratio >= QUOTE_MIN_RATIO) =>
            {
                let o = score.best_overlap.as_ref().expect("checked");
                return overlap_quotes(
                    &heard.query.prepared,
                    &o.heard,
                    index.document(*d),
                    &o.reference,
                );
            }
            TextDetail::Summary(s, score) => {
                if let Some(first) = score.heard_matched.iter().position(|m| *m) {
                    let p = &heard.query.prepared;
                    let doc = summaries.document(*s);
                    let in_summary = score.summary_matched.iter().position(|m| *m).unwrap_or(0);
                    return (
                        marked_quote(
                            &p.text,
                            &p.tokens,
                            &(first..first + 1),
                            &score.heard_matched,
                        ),
                        marked_quote(
                            &doc.text,
                            &doc.tokens,
                            &(in_summary..in_summary + 1),
                            &score.summary_matched,
                        ),
                    );
                }
            }
            _ => {}
        }
        if let Some((hook, from_transcript)) = &cell.hook
            && hook.score > 0.0
        {
            let p = if *from_transcript {
                &heard.transcript_for_titles.as_ref().expect("hook source").0
            } else {
                &heard.query.prepared
            };
            let mut marks = vec![false; p.len()];
            for m in &mut marks[hook.heard.clone()] {
                *m = true;
            }
            return (
                marked_quote(&p.text, &p.tokens, &hook.heard, &marks),
                vec![QuotePart {
                    text: ep.episode.title.clone(),
                    matched: true,
                }],
            );
        }
        (Vec::new(), Vec::new())
    }

    Ok(Evaluation {
        matrix: ScoreMatrix {
            files: input.files.iter().map(|f| f.file_id.clone()).collect(),
            episodes: input.episodes.iter().map(|e| e.episode.key).collect(),
            cells,
        },
        scores,
        disc_order: disc_use,
        located_in_order,
    })
}

/// Character similarity a heard phrase needs with the reference for the two to be shown as
/// quotes; weaker overlaps are chance agreement on common words and would mislead.
const QUOTE_MIN_RATIO: f32 = 0.75;
/// Title hook score at or above which the note "title heard" is shown.
const TITLE_HEARD: f32 = 0.5;
/// Duration fit below which the note "length mismatch" is shown.
const LENGTH_MISMATCH: f32 = 0.3;
/// Disc order signal at or above which the order agrees.
const ORDER_AGREES: f32 = 0.5;

/// Notes for one file/episode pair, most important first.
fn notes(
    signals: &Signals,
    file: &FileInput,
    heard: &Heard,
    has_reference: bool,
    position: Option<&PlayAllPosition>,
    disc_use: DiscOrderUse,
) -> Vec<EvidenceNote> {
    let mut notes = Vec::new();
    match signals.disc_order {
        Some(s) if s >= ORDER_AGREES => notes.push(EvidenceNote::DiscOrderAgrees {
            chapter: position.and_then(|p| p.chapter),
        }),
        Some(_) => notes.push(EvidenceNote::DiscOrderDisagrees),
        None => {}
    }
    if matches!(disc_use, DiscOrderUse::Ignored(_)) {
        notes.push(EvidenceNote::PlayAllIgnored);
    }
    if signals.title_hook.is_some_and(|t| t >= TITLE_HEARD) {
        notes.push(EvidenceNote::TitleHeard);
    }
    if signals.duration.is_some_and(|d| d < LENGTH_MISMATCH) {
        notes.push(EvidenceNote::LengthMismatch);
    }
    if heard.words == 0 && heard.transcript_for_titles.is_none() {
        notes.push(EvidenceNote::NoSpeech);
    }
    if file.mostly_music {
        notes.push(EvidenceNote::MostlyMusic);
    }
    if !has_reference {
        notes.push(EvidenceNote::NoReferenceText);
    }
    if let Some(windows) = file.sampled_windows {
        notes.push(EvidenceNote::Sampled { windows });
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_increasing_run() {
        assert_eq!(longest_increasing(&[]), 0);
        assert_eq!(longest_increasing(&[0, 1, 2, 3]), 4);
        assert_eq!(longest_increasing(&[3, 2, 1, 0]), 1);
        assert_eq!(longest_increasing(&[0, 2, 1, 3]), 3);
        assert_eq!(longest_increasing(&[1, 1, 1]), 1);
    }

    #[test]
    fn order_signal_between_anchors() {
        // Anchors: rank 0 is episode 2, rank 3 is episode 6.
        let anchors = [(0, 2), (3, 6)];
        // Rank 1 continues the sequence from rank 0: episode 3.
        assert_eq!(order_signal(1, 3, &anchors), Some(1.0));
        // Counting back from the anchor at rank 3, rank 1 would be episode 4.
        assert_eq!(order_signal(1, 4, &anchors), Some(1.0));
        // Episode 5 fits between the anchors but continues neither sequence.
        assert_eq!(order_signal(1, 5, &anchors), Some(0.6));
        // Rank 2 is one before the anchor at rank 3: episode 5.
        assert_eq!(order_signal(2, 5, &anchors), Some(1.0));
        // Outside the anchors' range.
        assert_eq!(order_signal(1, 1, &anchors), Some(0.0));
        assert_eq!(order_signal(1, 7, &anchors), Some(0.0));
    }

    #[test]
    fn order_signal_ignores_the_file_itself_and_needs_anchors() {
        assert_eq!(order_signal(0, 5, &[(0, 2)]), None);
        assert_eq!(order_signal(0, 5, &[]), None);
        // An anchor judged against its neighbours.
        assert_eq!(order_signal(1, 2, &[(0, 1), (1, 2), (2, 3)]), Some(1.0));
        assert_eq!(order_signal(1, 5, &[(0, 1), (1, 5), (2, 3)]), Some(0.0));
    }

    fn signals(dialogue: Option<f32>, title: Option<f32>, duration: Option<f32>) -> Signals {
        Signals {
            dialogue,
            title_hook: title,
            duration,
            disc_order: None,
        }
    }

    #[test]
    fn missing_signals_are_left_out_not_zero() {
        let c = MatchConfig::default();
        let only_duration = combine(&signals(None, None, Some(1.0)), false, false, &c);
        assert!((only_duration - 1.0).abs() < 1e-6);
        let with_zero_title = combine(&signals(None, Some(0.0), Some(1.0)), false, false, &c);
        assert!(with_zero_title < only_duration);
        assert_eq!(combine(&Signals::default(), false, false, &c), 0.0);
    }

    #[test]
    fn weak_dialogue_scales_the_score_down() {
        let c = MatchConfig::default();
        let strong = combine(&signals(Some(0.6), Some(0.0), Some(1.0)), true, false, &c);
        let weak = combine(&signals(Some(0.05), Some(0.0), Some(1.0)), true, false, &c);
        // A perfect length cannot lift a file whose dialogue does not match.
        assert!(weak < c.no_episode_score, "{weak}");
        assert!(strong > 0.5, "{strong}");
        // A heard title counts as identity evidence.
        let titled = combine(&signals(Some(0.05), Some(0.9), Some(1.0)), true, false, &c);
        assert!(titled > c.no_episode_score, "{titled}");
        // Summaries do not scale the score down.
        let summary = combine(&signals(Some(0.05), Some(0.0), Some(1.0)), false, false, &c);
        assert!(summary > weak);
    }

    #[test]
    fn music_boosts_the_title_hook() {
        let c = MatchConfig::default();
        let s = signals(Some(0.3), Some(1.0), None);
        assert!(combine(&s, true, true, &c) > combine(&s, true, false, &c));
    }
}
