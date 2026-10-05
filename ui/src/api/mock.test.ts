import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { JobEvent, JobRequest } from "../types/generated";
import { createMockBackend } from "./mock";
import { SCHOOLHOUSE_ROCK } from "./mockData";

const request: JobRequest = {
  folder: "/rips/disc1",
  show: SCHOOLHOUSE_ROCK,
  ordering: "aired",
  seasons: null,
  language: "en",
};

describe("mock backend", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("runs a job from started to finished and keeps the results", async () => {
    const backend = createMockBackend({ stepMs: 1 });
    const events: JobEvent[] = [];
    await backend.onJobEvent((e) => events.push(e));

    const jobId = await backend.startIdentification(request);
    await vi.runAllTimersAsync();

    expect(events[0]).toMatchObject({ kind: "started", jobId });
    expect(events.at(-1)).toEqual({ kind: "finished", jobId });
    const results = await backend.jobResults(jobId);
    expect(results.complete).toBe(true);
    expect(results.matches.map((m) => m.confidence.verdict)).toEqual([
      "confident",
      "check",
      "extra",
      "playAll",
    ]);
  });

  it("refuses a second job and relaunching for an update while a job runs", async () => {
    const backend = createMockBackend({ stepMs: 1 });
    await backend.startIdentification(request);
    await expect(backend.startIdentification(request)).rejects.toMatchObject({ code: "busy" });

    const download = backend.downloadUpdate();
    await vi.advanceTimersByTimeAsync(5);
    await download;
    await expect(backend.installUpdateAndRelaunch()).rejects.toMatchObject({ code: "busy" });

    await vi.runAllTimersAsync();
    await expect(backend.installUpdateAndRelaunch()).resolves.toBeUndefined();
  });

  it("stops emitting after cancel and ends with cancelled", async () => {
    const backend = createMockBackend({ stepMs: 1 });
    const events: JobEvent[] = [];
    await backend.onJobEvent((e) => events.push(e));
    const jobId = await backend.startIdentification(request);
    await vi.advanceTimersByTimeAsync(2);
    await backend.cancelIdentification(jobId);
    await vi.runAllTimersAsync();
    expect(events.at(-1)).toEqual({ kind: "cancelled", jobId });
    expect(events.some((e) => e.kind === "finished")).toBe(false);
  });

  it("reports a stored key as ready without ever returning the key", async () => {
    const backend = createMockBackend();
    await backend.setApiKey("subdl", "my-key");
    const subdl = (await backend.sourceStatus()).find((s) => s.provider === "subdl");
    expect(subdl).toEqual({ provider: "subdl", state: { kind: "ready" }, hasKey: true });
    expect(JSON.stringify(await backend.sourceStatus())).not.toContain("my-key");
  });

  it("leaves the play-all and extras out of the rename plan", async () => {
    const backend = createMockBackend();
    const plan = await backend.planRename({
      jobId: "j",
      decisions: [
        { fileId: "title_t03.mkv", decision: { kind: "approved", episode: { season: 4, number: 1 } } },
        { fileId: "title_t44.mkv", decision: { kind: "notAnEpisode" } },
      ],
      mode: { kind: "renameInPlace", root: "/rips" },
      naming: { kind: "jellyfinPlex" },
      saveHeardSubtitles: false,
    });
    expect(plan.items).toHaveLength(1);
    expect(plan.items[0]?.to).toMatch(
      /Schoolhouse Rock! \(1973\)\/Season 04\/Schoolhouse Rock! \(1973\) - S04E01 - Conjunction Junction\.mkv$/,
    );
    expect(plan.untouched.map((u) => u.reason).sort()).toEqual(["extra", "playAll"]);
  });
});
