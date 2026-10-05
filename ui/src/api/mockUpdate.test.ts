import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { UpdateEvent } from "../types/generated";
import { createMockBackend } from "./mock";

describe("mock backend update download", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("downloads with progress and then reports the downloaded version", async () => {
    const backend = createMockBackend({ stepMs: 1 });
    const events: UpdateEvent[] = [];
    await backend.onUpdateEvent((e) => events.push(e));

    const download = backend.downloadUpdate();
    await vi.runAllTimersAsync();
    await download;

    expect(events[0]).toMatchObject({ kind: "downloading" });
    expect(events.at(-1)).toMatchObject({ kind: "downloaded" });
  });

  it("stops when cancelled and keeps nothing to install", async () => {
    const backend = createMockBackend({ stepMs: 1 });
    const events: UpdateEvent[] = [];
    await backend.onUpdateEvent((e) => events.push(e));

    const download = backend.downloadUpdate();
    const outcome = expect(download).rejects.toMatchObject({ code: "cancelled" });
    await backend.cancelUpdateDownload();
    await vi.runAllTimersAsync();
    await outcome;

    expect(events).toEqual([]);
    await expect(backend.installUpdateAndRelaunch()).rejects.toMatchObject({ code: "notFound" });
  });
});
