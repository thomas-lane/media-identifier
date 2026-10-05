// Helpers for screen tests: render the whole app against a backend, and a backend whose job
// events the test emits by hand (so mid-job states are deterministic).

import { act, render } from "@testing-library/react";

import { BackendContext, createMockBackend } from "../api";
import type { Backend } from "../api";
import type { MockOptions } from "../api/mock";
import { App } from "../App";
import type { JobEvent, UpdateInfo } from "../types/generated";

export function renderApp(backend: Backend = createMockBackend({ stepMs: 1, downloadStepMs: 1, modelReady: true })) {
  const utils = render(
    <BackendContext.Provider value={backend}>
      <App />
    </BackendContext.Provider>,
  );
  return { ...utils, backend };
}

/** A mock backend whose job events and update announcements are emitted by the test. */
export function manualBackend(options: MockOptions = {}) {
  const base = createMockBackend({ stepMs: 1, downloadStepMs: 1, modelReady: true, ...options });
  const jobListeners = new Set<(e: JobEvent) => void>();
  const announce = new Set<(i: UpdateInfo) => void>();
  const backend: Backend = {
    ...base,
    startIdentification: async () => "job-1",
    cancelIdentification: async (jobId) => {
      emitNow({ kind: "cancelled", jobId });
    },
    onJobEvent: async (listener) => {
      jobListeners.add(listener);
      return () => jobListeners.delete(listener);
    },
    onUpdateAvailable: async (listener) => {
      announce.add(listener);
      return () => announce.delete(listener);
    },
  };
  function emitNow(event: JobEvent) {
    for (const l of [...jobListeners]) l(event);
  }
  return {
    backend,
    /** Emits job events inside `act`. */
    emit: (...events: JobEvent[]) =>
      act(() => {
        for (const e of events) emitNow(e);
      }),
    /** Announces an update as the launch-time check would. */
    announce: (info: UpdateInfo) =>
      act(() => {
        for (const l of [...announce]) l(info);
      }),
  };
}
