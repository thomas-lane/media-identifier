import { describe, expect, it } from "vitest";

import { MATCHES } from "../api/mockData";
import type { FileMatch } from "../types/generated";
import {
  categoryOf,
  countCategories,
  initialReview,
  nextIndexAfterApprove,
  sharedEpisodeWith,
  toDecision,
  toDecisions,
  withInitialReviews,
} from "./review";
import type { FileReview } from "./review";

const byId = (id: string) => MATCHES.find((m) => m.fileId === id)!;

describe("review", () => {
  it("pre-approves confident matches, holds checks and marks extras", () => {
    expect(initialReview(byId("title_t03.mkv"))).toEqual({ choice: { kind: "episode", episode: { season: 4, number: 1 } }, approved: true });
    expect(initialReview(byId("title_t11.mkv"))).toEqual({ choice: { kind: "episode", episode: { season: 2, number: 3 } }, approved: false });
    expect(initialReview(byId("title_t44.mkv"))).toEqual({ choice: { kind: "notAnEpisode" }, approved: true });
    expect(initialReview(byId("title_t00.mkv"))).toBeNull();
  });

  it("keeps the user's choices when new matches arrive", () => {
    const mine: FileReview = { choice: { kind: "skip" }, approved: true };
    const matches: Record<string, FileMatch> = { "title_t03.mkv": byId("title_t03.mkv"), "title_t11.mkv": byId("title_t11.mkv") };
    const reviews = withInitialReviews({ "title_t03.mkv": mine }, matches);
    expect(reviews["title_t03.mkv"]).toBe(mine);
    expect(reviews["title_t11.mkv"]?.approved).toBe(false);
  });

  it("categorises, counts and converts to decisions", () => {
    const check = initialReview(byId("title_t11.mkv"))!;
    expect(categoryOf(byId("title_t11.mkv"), check)).toBe("check");
    expect(categoryOf(byId("title_t00.mkv"), undefined)).toBe("playAll");
    expect(categoryOf(undefined, undefined)).toBe("waiting");
    expect(toDecision(check)).toEqual({ kind: "pending" });
    expect(toDecision({ ...check, approved: true })).toEqual({ kind: "approved", episode: { season: 2, number: 3 } });
    expect(toDecision({ choice: { kind: "notAnEpisode" }, approved: true })).toEqual({ kind: "notAnEpisode" });
    expect(toDecision({ choice: { kind: "skip" }, approved: true })).toEqual({ kind: "skip" });
    expect(countCategories(["approved", "check", "check", "extra"])).toMatchObject({ approved: 1, check: 2, extra: 1 });
    expect(toDecisions(["title_t00.mkv", "title_t11.mkv"], { "title_t11.mkv": check })).toEqual([
      { fileId: "title_t11.mkv", decision: { kind: "pending" } },
    ]);
  });

  it("finds files sharing an episode", () => {
    const ep = { kind: "episode" as const, episode: { season: 2, number: 3 } };
    const reviews: Record<string, FileReview> = {
      a: { choice: ep, approved: true },
      b: { choice: ep, approved: false },
      c: { choice: { kind: "notAnEpisode" }, approved: true },
    };
    expect(sharedEpisodeWith("a", reviews)).toEqual(["b"]);
    expect(sharedEpisodeWith("c", reviews)).toEqual([]);
  });

  it("moves to the next file still to check after approving, wrapping around", () => {
    expect(nextIndexAfterApprove(["approved", "approved", "check", "approved"], 0)).toBe(2);
    expect(nextIndexAfterApprove(["check", "approved", "approved", "approved"], 2)).toBe(0);
    expect(nextIndexAfterApprove(["approved", "approved", "approved"], 1)).toBe(2);
    expect(nextIndexAfterApprove(["approved", "approved"], 1)).toBe(1);
  });
});
