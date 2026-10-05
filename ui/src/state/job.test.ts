import { describe, expect, it } from "vitest";

import { MATCHES } from "../api/mockData";
import { identifyReducer, initialIdentifyState } from "./identify";
import { applyJobEvent, jobFromResults, newJob } from "./job";

const match = MATCHES.find((m) => m.fileId === "title_t11.mkv")!;

describe("job events", () => {
  it("tracks stages, file status, matches and the end of the job", () => {
    let job = newJob("j1");
    job = applyJobEvent(job, { kind: "started", jobId: "j1", fileIds: ["title_t11.mkv"], accelerator: "cpu" });
    expect(job.files["title_t11.mkv"]?.status).toBe("waiting");
    job = applyJobEvent(job, { kind: "stage", jobId: "j1", stage: "listening", state: { kind: "running", done: 3, total: 9 } });
    expect(job.stages.listening).toEqual({ kind: "running", done: 3, total: 9 });
    job = applyJobEvent(job, { kind: "matched", jobId: "j1", result: match });
    expect(job.files["title_t11.mkv"]).toEqual({ status: "done", bestSoFar: "Lucky Seven Sampson", verdict: "check" });
    job = applyJobEvent(job, { kind: "eta", jobId: "j1", seconds: 40 });
    expect(job.etaSeconds).toBe(40);
    job = applyJobEvent(job, { kind: "failed", jobId: "j1", message: "Disk full" });
    expect(job).toMatchObject({ phase: "failed", failure: "Disk full", etaSeconds: null });
  });

  it("ignores events of other jobs", () => {
    const job = newJob("j1");
    expect(applyJobEvent(job, { kind: "finished", jobId: "j2" })).toBe(job);
  });

  it("treats incomplete saved results as a stopped job, not a running one", () => {
    const job = jobFromResults({
      jobId: "j1",
      request: { folder: "/r", show: { showRef: { provider: "tvmaze", id: "1" }, name: "S", year: null, kind: null, seasonCount: null, episodeCount: null, url: null }, ordering: "aired", seasons: null, language: "en" },
      episodes: [],
      matches: [match],
      model: "fast",
      complete: false,
    });
    expect(job.phase).toBe("cancelled");
    expect(job.fileIds).toEqual(["title_t11.mkv"]);
  });
});

describe("identify flow", () => {
  it("keeps events that arrive before the job id is known", () => {
    let state = identifyReducer(initialIdentifyState, {
      type: "starting",
      request: { folder: "/r", show: { showRef: { provider: "tvmaze", id: "1" }, name: "S", year: null, kind: null, seasonCount: null, episodeCount: null, url: null }, ordering: "aired", seasons: null, language: "en" },
    });
    state = identifyReducer(state, { type: "event", event: { kind: "started", jobId: "j9", fileIds: ["title_t11.mkv"], accelerator: "appleGpu" } });
    state = identifyReducer(state, { type: "event", event: { kind: "matched", jobId: "j9", result: match } });
    expect(state.job).toBeNull();
    state = identifyReducer(state, { type: "started", jobId: "j9" });
    expect(state.job?.accelerator).toBe("appleGpu");
    expect(state.job?.matches["title_t11.mkv"]).toBe(match);
    expect(state.reviews["title_t11.mkv"]).toEqual({ choice: { kind: "episode", episode: { season: 2, number: 3 } }, approved: false });
  });

  it("moves to Review when a job finishes only if the progress screen is showing", () => {
    const onProgress = { ...initialIdentifyState, step: "identifying" as const };
    expect(identifyReducer(onProgress, { type: "autoReview" }).step).toBe("review");
    const onRename = { ...initialIdentifyState, step: "rename" as const };
    expect(identifyReducer(onRename, { type: "autoReview" }).step).toBe("rename");
  });
});
