// Review decisions: what the user has chosen for each file, how the list is counted and
// filtered, and how choices become the `ReviewDecision`s sent to the rename plan.

import type {
  Episode,
  EpisodeKey,
  FileDecision,
  FileId,
  FileMatch,
  ReviewDecision,
} from "../types/generated";

/** What the suggestion dropdown shows for a file. */
export type Choice =
  | { kind: "episode"; episode: EpisodeKey }
  | { kind: "notAnEpisode" }
  | { kind: "skip" };

/** A file's review state. `approved` is false while the file still needs checking. */
export interface FileReview {
  choice: Choice;
  approved: boolean;
}

/** How a file shows in the list. */
export type ReviewCategory = "approved" | "check" | "extra" | "skipped" | "playAll" | "waiting";

/** List filters, in the order of the segmented control. */
export type ReviewFilter = "all" | "approved" | "check" | "extras";

/**
 * The starting review for a match: confident suggestions are pre-approved, "check" suggestions
 * wait for the user, and extras start as "Not an episode". The play-all has no review.
 */
export function initialReview(match: FileMatch): FileReview | null {
  switch (match.suggestion.kind) {
    case "episode":
      return {
        choice: { kind: "episode", episode: match.suggestion.episode },
        approved: match.confidence.verdict === "confident",
      };
    case "notAnEpisode":
      return { choice: { kind: "notAnEpisode" }, approved: true };
    case "playAll":
      return null;
  }
}

/** Fills in reviews for matches that have none yet, keeping the user's existing choices. */
export function withInitialReviews(
  reviews: Record<FileId, FileReview>,
  matches: Record<FileId, FileMatch>,
): Record<FileId, FileReview> {
  let next = reviews;
  for (const [id, match] of Object.entries(matches)) {
    if (next[id]) continue;
    const review = initialReview(match);
    if (review) {
      if (next === reviews) next = { ...reviews };
      next[id] = review;
    }
  }
  return next;
}

/** The list category of a file. */
export function categoryOf(match: FileMatch | undefined, review: FileReview | undefined): ReviewCategory {
  if (!match) return "waiting";
  if (match.suggestion.kind === "playAll") return "playAll";
  if (!review) return "waiting";
  if (review.choice.kind === "skip") return "skipped";
  if (!review.approved) return "check";
  return review.choice.kind === "notAnEpisode" ? "extra" : "approved";
}

/** Whether a file belongs in the list under `filter`. */
export function matchesFilter(category: ReviewCategory, filter: ReviewFilter): boolean {
  switch (filter) {
    case "all":
      return true;
    case "approved":
      return category === "approved";
    case "check":
      return category === "check";
    case "extras":
      return category === "extra";
  }
}

/** Counts per category. */
export function countCategories(categories: ReviewCategory[]): Record<ReviewCategory, number> {
  const counts: Record<ReviewCategory, number> = {
    approved: 0,
    check: 0,
    extra: 0,
    skipped: 0,
    playAll: 0,
    waiting: 0,
  };
  for (const c of categories) counts[c] += 1;
  return counts;
}

/** The decision sent to the rename plan. */
export function toDecision(review: FileReview): ReviewDecision {
  if (review.choice.kind === "skip") return { kind: "skip" };
  if (!review.approved) return { kind: "pending" };
  return review.choice.kind === "notAnEpisode"
    ? { kind: "notAnEpisode" }
    : { kind: "approved", episode: review.choice.episode };
}

/** Decisions for every reviewed file, in list order. */
export function toDecisions(order: FileId[], reviews: Record<FileId, FileReview>): FileDecision[] {
  return order.flatMap((fileId) => {
    const review = reviews[fileId];
    return review ? [{ fileId, decision: toDecision(review) }] : [];
  });
}

/** Stable string form of an episode key, for maps and `<option>` values. */
export function episodeKeyId(key: EpisodeKey): string {
  return `${key.season}:${key.number}`;
}

/** Parses `episodeKeyId` output. */
export function parseEpisodeKeyId(id: string): EpisodeKey | null {
  const m = /^(\d+):(\d+)$/.exec(id);
  return m ? { season: Number(m[1]), number: Number(m[2]) } : null;
}

/** Same season and number. */
export function sameEpisode(a: EpisodeKey, b: EpisodeKey): boolean {
  return a.season === b.season && a.number === b.number;
}

/** The episode with this key, if listed. */
export function findEpisode(episodes: Episode[], key: EpisodeKey): Episode | undefined {
  return episodes.find((e) => sameEpisode(e.key, key));
}

/**
 * Other files whose approved or suggested choice is the same episode as `fileId`'s. Two files
 * renamed to one episode would collide, so the evidence panel warns about it.
 */
export function sharedEpisodeWith(
  fileId: FileId,
  reviews: Record<FileId, FileReview>,
): FileId[] {
  const own = reviews[fileId]?.choice;
  if (!own || own.kind !== "episode") return [];
  return Object.entries(reviews)
    .filter(([id, r]) => id !== fileId && r.choice.kind === "episode" && sameEpisode(r.choice.episode, own.episode))
    .map(([id]) => id);
}

/**
 * Index to select after approving the file at `from`: the next file still to check (wrapping
 * around), or simply the next file when none is left.
 */
export function nextIndexAfterApprove(categories: ReviewCategory[], from: number): number {
  const n = categories.length;
  for (let step = 1; step < n; step++) {
    const i = (from + step) % n;
    if (categories[i] === "check") return i;
  }
  return Math.min(from + 1, n - 1);
}
