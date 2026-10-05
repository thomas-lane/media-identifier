// Review (layout A): the file list on the left, the evidence for the selected file on the right.
// ↑/↓ move through the list and Enter approves the selected file's suggestion.

import { useEffect, useId, useMemo, useRef, useState } from "react";
import type { KeyboardEvent, ReactNode } from "react";

import { useBackend } from "../api";
import { ButtonRow, Pill, ProgressBar, Segmented, scoreTone } from "../components/common";
import { formatDuration, formatEpisodeCode, formatPercent, plural } from "../lib/format";
import { baseName, joinPath } from "../lib/paths";
import {
  categoryOf,
  countCategories,
  episodeKeyId,
  findEpisode,
  matchesFilter,
  nextIndexAfterApprove,
  parseEpisodeKeyId,
  sameEpisode,
  sharedEpisodeWith,
} from "../lib/review";
import type { Choice, FileReview, ReviewCategory, ReviewFilter } from "../lib/review";
import { useIdentify } from "../state/identify";
import { fileProgress } from "../state/job";
import type { Candidate, Episode, EvidenceNote, FileId, FileMatch, QuotePart } from "../types/generated";

export function ReviewScreen() {
  const { state, dispatch, fileInfo } = useIdentify();
  const { job, reviews, request } = state;
  const [filter, setFilter] = useState<ReviewFilter>("all");
  const listRef = useRef<HTMLDivElement>(null);
  const ids = useId();

  // Files in list order: everything identified so far, play-all last.
  const order = useMemo(() => {
    if (!job) return [];
    const done = job.fileIds.filter((id) => job.matches[id]);
    const isPlayAll = (id: FileId) => job.matches[id]?.suggestion.kind === "playAll";
    return [...done.filter((id) => !isPlayAll(id)), ...done.filter(isPlayAll)];
  }, [job]);
  const categories = useMemo(
    () => new Map(order.map((id) => [id, categoryOf(job?.matches[id], reviews[id])])),
    [order, job, reviews],
  );
  const counts = countCategories([...categories.values()]);
  const visible = order.filter((id) => matchesFilter(categories.get(id) ?? "waiting", filter));
  const selected = state.selected && visible.includes(state.selected) ? state.selected : (visible[0] ?? null);

  useEffect(() => {
    listRef.current?.focus();
  }, []);

  if (!job || !request) return null;
  const running = job.phase === "running";
  const progress = fileProgress(job, state.scan?.playAll?.fileId ?? null);

  const select = (id: FileId | null) => dispatch({ type: "select", fileId: id });

  const approve = (id: FileId) => {
    const review = reviews[id];
    const visibleCats = visible.map((v) => categories.get(v) ?? "waiting");
    const index = visible.indexOf(id);
    if (review && !review.approved) dispatch({ type: "review", fileId: id, review: { ...review, approved: true } });
    // Move to the next file still to check (computed before this one changes).
    const nextCats = visibleCats.map((c, i) => (i === index ? "approved" : c)) as ReviewCategory[];
    const next = visible[nextIndexAfterApprove(nextCats, index)];
    if (next) select(next);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (visible.length === 0) return;
    const index = selected ? visible.indexOf(selected) : -1;
    const go = (i: number) => {
      e.preventDefault();
      const id = visible[Math.max(0, Math.min(visible.length - 1, i))];
      if (id) {
        select(id);
        document.getElementById(`${ids}-${id}`)?.scrollIntoView?.({ block: "nearest" });
      }
    };
    if (e.key === "ArrowDown") go(index + 1);
    else if (e.key === "ArrowUp") go(index - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(visible.length - 1);
    else if (e.key === "Enter" && selected && categories.get(selected) !== "playAll") {
      e.preventDefault();
      approve(selected);
    }
  };

  const filterOptions: { value: ReviewFilter; label: string }[] = [
    { value: "all", label: `All ${order.length}` },
    { value: "approved", label: `Approved ${counts.approved}` },
    { value: "check", label: `Check ${counts.check}` },
    { value: "extras", label: `Extras ${counts.extra}` },
  ];

  return (
    <section className="screen" aria-labelledby="review-title">
      <div className="screen-head">
        <h1 id="review-title">
          Review · {request.show.name} <span className="muted small mono">{baseName(request.folder)}</span>
        </h1>
        <Segmented label="Show" options={filterOptions} value={filter} onChange={setFilter} />
      </div>
      {running && (
        <p className="alert warn small" role="status">
          Still identifying: {progress.done} of {progress.total} files finished. New results
          appear here as they finish.
        </p>
      )}
      <div className="review">
        <div>
          <div
            ref={listRef}
            className="file-list"
            role="listbox"
            aria-label="Files"
            tabIndex={0}
            aria-activedescendant={selected ? `${ids}-${selected}` : undefined}
            onKeyDown={onKeyDown}
          >
            <div className="file-list-head" aria-hidden="true">
              <span>File</span>
              <span>Episode</span>
              <span>Result</span>
            </div>
            {visible.length === 0 && <div className="file-row muted">No files in this group.</div>}
            {visible.map((id) => (
              <FileRow
                key={id}
                domId={`${ids}-${id}`}
                fileId={id}
                selected={id === selected}
                category={categories.get(id) ?? "waiting"}
                match={job.matches[id]}
                review={reviews[id]}
                episodes={job.episodes}
                durationS={fileInfo(id)?.probe?.durationS ?? null}
                onSelect={() => {
                  select(id);
                  listRef.current?.focus();
                }}
              />
            ))}
          </div>
          <p className="muted small" style={{ margin: "8px 0 0" }}>
            <kbd>↑</kbd> <kbd>↓</kbd> to move through the list, <kbd>Enter</kbd> to approve.
          </p>
        </div>
        {selected && job.matches[selected] ? (
          <EvidencePanel
            key={selected}
            fileId={selected}
            match={job.matches[selected]!}
            review={reviews[selected]}
            reviews={reviews}
            episodes={job.episodes}
            folder={request.folder}
            onChange={(review) => dispatch({ type: "review", fileId: selected, review })}
            onApprove={() => approve(selected)}
          />
        ) : (
          <div className="card muted">Select a file to see why it was matched.</div>
        )}
      </div>
      <div className="review-foot">
        <span className="muted small">
          {counts.approved} approved · {counts.check} to check · {plural(counts.extra, "extra")}
          {counts.skipped > 0 ? ` · ${counts.skipped} skipped` : ""}
        </span>
        <ButtonRow
          others={[]}
          primary={
            <button
              type="button"
              className="btn primary"
              disabled={running}
              title={running ? "Available when identification finishes" : undefined}
              onClick={() => dispatch({ type: "go", step: "rename" })}
            >
              Continue to rename →
            </button>
          }
        />
      </div>
    </section>
  );
}

function episodeLabel(episodes: Episode[], choice: Choice, fallbackTitle?: string): string {
  if (choice.kind !== "episode") return choice.kind === "skip" ? "Skipped" : "Not an episode";
  const title = findEpisode(episodes, choice.episode)?.title ?? fallbackTitle ?? "";
  return `${formatEpisodeCode(choice.episode)} ${title}`.trim();
}

function chosenCandidate(match: FileMatch, review: FileReview | undefined): Candidate | undefined {
  const choice = review?.choice;
  if (choice?.kind === "episode") return match.candidates.find((c) => sameEpisode(c.episode, choice.episode));
  return undefined;
}

function FileRow({
  domId,
  fileId,
  selected,
  category,
  match,
  review,
  episodes,
  durationS,
  onSelect,
}: {
  domId: string;
  fileId: FileId;
  selected: boolean;
  category: ReviewCategory;
  match: FileMatch | undefined;
  review: FileReview | undefined;
  episodes: Episode[];
  durationS: number | null;
  onSelect: () => void;
}) {
  const candidate = match && chosenCandidate(match, review);
  let episode: ReactNode;
  if (category === "playAll") episode = <span className="muted">Play all (disc order)</span>;
  else if (!review) episode = <span className="muted">—</span>;
  else if (review.choice.kind !== "episode") episode = <span className="muted">{episodeLabel(episodes, review.choice)}</span>;
  else episode = episodeLabel(episodes, review.choice, candidate?.title);

  let result: ReactNode;
  switch (category) {
    case "approved":
      result = <Pill tone="ok">✓ {candidate ? formatPercent(candidate.score) : "Chosen"}</Pill>;
      break;
    case "check":
      result = <Pill tone="warn">Check {candidate ? formatPercent(candidate.score) : ""}</Pill>;
      break;
    case "extra":
      result = <Pill tone="gray">Extra</Pill>;
      break;
    case "skipped":
      result = <Pill tone="gray">Skip</Pill>;
      break;
    case "playAll":
      result = <Pill tone="info">Play all</Pill>;
      break;
    case "waiting":
      result = <Pill tone="gray">Waiting</Pill>;
      break;
  }
  return (
    <div id={domId} className="file-row" role="option" aria-selected={selected} onClick={onSelect}>
      <span className="mono">
        {baseName(fileId)}
        {durationS !== null && (
          <>
            <br />
            <span className="muted">{formatDuration(durationS)}</span>
          </>
        )}
      </span>
      <span>{episode}</span>
      <span>{result}</span>
    </div>
  );
}

function noteText(note: EvidenceNote): string {
  switch (note.kind) {
    case "mostlyMusic":
      return "Mostly music, so there is little dialogue to compare.";
    case "noSpeech":
      return "No speech was heard in the file.";
    case "noReferenceText":
      return "No subtitles or lyrics were found for this episode, so its dialogue couldn't be compared.";
    case "discOrderAgrees":
      return note.chapter !== null
        ? `Its place on the disc (chapter ${note.chapter + 1}) fits this episode.`
        : "Its place on the disc fits this episode.";
    case "discOrderDisagrees":
      return "Its place on the disc doesn't fit this episode.";
    case "playAllIgnored":
      return "The play-all's order looked shuffled or incomplete, so disc order wasn't used.";
    case "titleHeard":
      return "The episode title was heard in the file.";
    case "lengthMismatch":
      return "The file's length doesn't fit this episode's listed runtime.";
    case "sampled":
      return `Listened to ${plural(note.windows, "sample")} of the file.`;
  }
}

function EvidencePanel({
  fileId,
  match,
  review,
  reviews,
  episodes,
  folder,
  onChange,
  onApprove,
}: {
  fileId: FileId;
  match: FileMatch;
  review: FileReview | undefined;
  reviews: Record<FileId, FileReview>;
  episodes: Episode[];
  folder: string;
  onChange: (review: FileReview) => void;
  onApprove: () => void;
}) {
  const backend = useBackend();
  const { fileInfo } = useIdentify();
  const info = fileInfo(fileId);
  const ids = useId();
  const path = info?.path ?? joinPath(folder, fileId);
  const meta = [
    info?.probe ? formatDuration(info.probe.durationS) : null,
    info?.probe?.video ? `${info.probe.video.width}×${info.probe.video.height}` : null,
  ].filter(Boolean);

  const header = (
    <div className="row" style={{ gap: 10 }}>
      <div className="grow">
        <div className="mono">{baseName(fileId)}</div>
        {meta.length > 0 && <div className="muted small">{meta.join(" · ")}</div>}
      </div>
      <button type="button" className="btn small" onClick={() => void backend.openFile(path).catch(() => {})}>
        ▶ Play
      </button>
    </div>
  );

  if (match.suggestion.kind === "playAll" || !review) {
    return (
      <div className="card evidence" aria-label="Evidence">
        {header}
        <p style={{ margin: 0 }}>
          This is the play-all title: one long title containing the episodes in disc order. It is used to work out the
          order of the other files and is left as it is.
        </p>
      </div>
    );
  }

  const candidate = chosenCandidate(match, review) ?? (review.choice.kind === "episode" ? undefined : match.candidates[0]);
  const showingChosen = review.choice.kind === "episode";
  const candidateIds = new Set(match.candidates.map((c) => episodeKeyId(c.episode)));
  const others = episodes.filter((e) => !candidateIds.has(episodeKeyId(e.key)));
  const value =
    review.choice.kind === "episode" ? `ep:${episodeKeyId(review.choice.episode)}` : review.choice.kind === "skip" ? "skip" : "extra";
  const shared = sharedEpisodeWith(fileId, reviews);

  const setChoice = (v: string) => {
    if (v === "extra") onChange({ choice: { kind: "notAnEpisode" }, approved: true });
    else if (v === "skip") onChange({ choice: { kind: "skip" }, approved: true });
    else {
      const key = parseEpisodeKeyId(v.slice(3));
      if (key) onChange({ choice: { kind: "episode", episode: key }, approved: true });
    }
  };

  const signals = candidate?.evidence.signals;
  const evidence = candidate?.evidence;
  const runnerUp = match.confidence.margin;

  return (
    <div className="card evidence" aria-label="Evidence">
      {header}
      <div>
        <label className="eyebrow" htmlFor={`${ids}-pick`} style={{ display: "block" }}>
          Suggested
        </label>
        <select id={`${ids}-pick`} className="field" value={value} onChange={(e) => setChoice(e.target.value)}>
          {match.candidates.length > 0 && (
            <optgroup label="Best matches">
              {match.candidates.map((c) => (
                <option key={episodeKeyId(c.episode)} value={`ep:${episodeKeyId(c.episode)}`}>
                  {formatEpisodeCode(c.episode)} · {c.title} ({formatPercent(c.score)})
                </option>
              ))}
            </optgroup>
          )}
          {others.length > 0 && (
            <optgroup label="Other episodes">
              {others.map((e) => (
                <option key={episodeKeyId(e.key)} value={`ep:${episodeKeyId(e.key)}`}>
                  {formatEpisodeCode(e.key)} · {e.title}
                </option>
              ))}
            </optgroup>
          )}
          <option value="extra">Not an episode (extra)</option>
          <option value="skip">Skip this file</option>
        </select>
        {shared.length > 0 && (
          <p className="small warn-text" style={{ margin: "6px 0 0" }}>
            ⚠︎ Also chosen for {shared.map(baseName).join(", ")}. Only one file can be renamed to an episode.
          </p>
        )}
      </div>
      {review.choice.kind === "notAnEpisode" && match.suggestion.kind === "notAnEpisode" && (
        <p style={{ margin: 0 }}>No episode's dialogue matches this file well. It is probably a bonus feature, so it is left as it is.</p>
      )}
      {review.choice.kind === "skip" && <p style={{ margin: 0 }}>This file is left as it is.</p>}
      {showingChosen && !candidate && (
        <p className="muted" style={{ margin: 0 }}>
          You chose this episode. The app found no evidence for it, so it has nothing to show here.
        </p>
      )}
      {signals && (showingChosen || match.suggestion.kind === "notAnEpisode") && (
        <div>
          <div className="eyebrow">Why</div>
          <div className="stack" style={{ gap: 6 }}>
            <SignalMeter label="Dialogue" value={signals.dialogue} />
            <SignalMeter label="Title heard" value={signals.titleHook} />
            <SignalMeter label="Disc order" value={signals.discOrder} />
            <SignalMeter label="Length" value={signals.duration} />
          </div>
          {showingChosen && candidate === match.candidates[0] && (
            <p className="muted small" style={{ margin: "6px 0 0" }}>
              Overall {formatPercent(match.confidence.score)}, {Math.round(runnerUp * 100)} points ahead of the next best
              choice.
            </p>
          )}
          {evidence && evidence.notes.length > 0 && (
            <p className="muted small" style={{ margin: "6px 0 0" }}>
              {evidence.notes.map(noteText).join(" ")}
            </p>
          )}
        </div>
      )}
      {evidence && showingChosen && (evidence.heard.length > 0 || evidence.reference.length > 0) && (
        <div className="quotes">
          <Quote label="Heard in the file" parts={evidence.heard} />
          <Quote label="Episode subtitles or lyrics" parts={evidence.reference} />
        </div>
      )}
      {evidence?.playAllPosition && showingChosen && <PlayAllStrip position={evidence.playAllPosition} />}
      <ButtonRow
        others={[
          <button
            key="extra"
            type="button"
            className="btn"
            onClick={() => onChange({ choice: { kind: "notAnEpisode" }, approved: true })}
          >
            Not an episode
          </button>,
        ]}
        primary={
          <button type="button" className="btn primary" onClick={onApprove} disabled={review.approved}>
            {review.approved ? "Approved" : "Approve"}
          </button>
        }
      />
    </div>
  );
}

function SignalMeter({ label, value }: { label: string; value: number | null }) {
  if (value === null) {
    return (
      <div className="meter">
        <span>{label}</span>
        <span className="muted small">Not measured</span>
        <span />
      </div>
    );
  }
  return (
    <div className="meter">
      <span>{label}</span>
      <ProgressBar value={value} label={label} tone={scoreTone(value)} />
      <span>{formatPercent(value)}</span>
    </div>
  );
}

function Quote({ label, parts }: { label: string; parts: QuotePart[] }) {
  return (
    <figure className="quote">
      <figcaption className="eyebrow" style={{ fontSize: 11.5, marginBottom: 4 }}>
        {label}
      </figcaption>
      {parts.length === 0 ? (
        <span className="muted">Nothing to show.</span>
      ) : (
        <blockquote style={{ margin: 0 }}>
          “{parts.map((p, i) => (p.matched ? <mark key={i}>{p.text}</mark> : <span key={i}>{p.text}</span>))}”
        </blockquote>
      )}
    </figure>
  );
}

/** Up to three chapters either side of the one the file was found in. */
function PlayAllStrip({ position }: { position: NonNullable<Candidate["evidence"]["playAllPosition"]> }) {
  const { state } = useIdentify();
  const chapterCount = state.scan?.playAll?.chapterCount ?? null;
  if (position.chapter === null) {
    return (
      <div>
        <div className="eyebrow">Position in play-all</div>
        <p className="small" style={{ margin: 0 }}>
          Found at {formatDuration(position.startS)}–{formatDuration(position.endS)} in the play-all.
        </p>
      </div>
    );
  }
  const hit = position.chapter;
  const last = chapterCount !== null ? chapterCount - 1 : hit + 3;
  const from = Math.max(0, Math.min(hit - 3, last - 6));
  const to = Math.min(last, from + 6);
  const chapters = Array.from({ length: to - from + 1 }, (_, i) => from + i);
  return (
    <div>
      <div className="eyebrow">Position in play-all</div>
      <ol className="timeline" aria-label={`Chapter ${hit + 1} of the play-all, at ${formatDuration(position.startS)}`}>
        {chapters.map((c) => (
          <li key={c} className={c === hit ? "hit" : undefined} aria-current={c === hit ? "true" : undefined}>
            {c + 1}
          </li>
        ))}
      </ol>
    </div>
  );
}
