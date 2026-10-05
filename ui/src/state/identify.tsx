// The Identify flow: Start → Confirm show → Identifying → Review → Rename. Holds the scan, the
// running job (folded from job events), and the user's review choices, so the user can leave
// the flow (History, Settings) and come back without losing anything.

import { createContext, useCallback, useContext, useEffect, useMemo, useReducer } from "react";
import type { ReactNode } from "react";

import { toApiError, useBackend } from "../api";
import type { FileId, JobEvent, JobId, JobRequest, JobResults, MediaFile, ScanSummary } from "../types/generated";
import { withInitialReviews } from "../lib/review";
import type { FileReview } from "../lib/review";
import { applyJobEvent, jobFromResults, newJob } from "./job";
import type { JobView } from "./job";

export type Step = "start" | "confirm" | "identifying" | "review" | "rename";

export interface IdentifyState {
  step: Step;
  /** A folder scan in progress. */
  scanning: boolean;
  scan: ScanSummary | null;
  request: JobRequest | null;
  /** Waiting for `startIdentification` to return the job id. */
  starting: boolean;
  /** Events that arrived before the job id was known. */
  pending: JobEvent[];
  job: JobView | null;
  reviews: Record<FileId, FileReview>;
  /** The file shown in the Review evidence panel. */
  selected: FileId | null;
  error: string | null;
}

export type IdentifyAction =
  | { type: "scanStarted" }
  | { type: "scanned"; scan: ScanSummary }
  | { type: "failed"; message: string }
  | { type: "startFailed"; message: string }
  | { type: "clearError" }
  | { type: "starting"; request: JobRequest }
  | { type: "started"; jobId: JobId }
  | { type: "event"; event: JobEvent }
  | { type: "results"; results: JobResults }
  | { type: "opened"; results: JobResults; scan: ScanSummary | null }
  | { type: "review"; fileId: FileId; review: FileReview }
  | { type: "select"; fileId: FileId | null }
  | { type: "go"; step: Step }
  | { type: "autoReview" }
  | { type: "reset" };

export const initialIdentifyState: IdentifyState = {
  step: "start",
  scanning: false,
  scan: null,
  request: null,
  starting: false,
  pending: [],
  job: null,
  reviews: {},
  selected: null,
  error: null,
};

function withJob(state: IdentifyState, job: JobView): IdentifyState {
  return { ...state, job, reviews: withInitialReviews(state.reviews, job.matches) };
}

export function identifyReducer(state: IdentifyState, action: IdentifyAction): IdentifyState {
  switch (action.type) {
    case "scanStarted":
      return { ...state, scanning: true, error: null };
    case "scanned":
      return { ...initialIdentifyState, step: "confirm", scan: action.scan };
    case "failed":
      return { ...state, scanning: false, starting: false, error: action.message };
    case "startFailed":
      return { ...state, step: "confirm", starting: false, pending: [], error: action.message };
    case "clearError":
      return { ...state, error: null };
    case "starting":
      return {
        ...state,
        step: "identifying",
        request: action.request,
        starting: true,
        pending: [],
        job: null,
        reviews: {},
        selected: null,
        error: null,
      };
    case "started": {
      let job = newJob(action.jobId);
      for (const event of state.pending) job = applyJobEvent(job, event);
      return withJob({ ...state, starting: false, pending: [] }, job);
    }
    case "event":
      if (state.starting) return { ...state, pending: [...state.pending, action.event] };
      if (!state.job) return state;
      return withJob(state, applyJobEvent(state.job, action.event));
    case "results": {
      if (!state.job || state.job.jobId !== action.results.jobId) return state;
      const fromResults = jobFromResults(action.results);
      const job: JobView = {
        ...state.job,
        episodes: action.results.episodes.length ? action.results.episodes : state.job.episodes,
        matches: { ...state.job.matches, ...fromResults.matches },
        fileIds: [
          ...state.job.fileIds,
          ...fromResults.fileIds.filter((id) => !state.job!.fileIds.includes(id)),
        ],
        files: { ...state.job.files, ...fromResults.files },
      };
      return withJob(state, job);
    }
    case "opened":
      return withJob(
        {
          ...initialIdentifyState,
          step: "review",
          scan: action.scan,
          request: action.results.request,
        },
        jobFromResults(action.results),
      );
    case "review":
      return { ...state, reviews: { ...state.reviews, [action.fileId]: action.review } };
    case "select":
      return { ...state, selected: action.fileId };
    case "go":
      return { ...state, step: action.step, error: null };
    case "autoReview":
      return state.step === "identifying" ? { ...state, step: "review" } : state;
    case "reset":
      return initialIdentifyState;
  }
}

export interface IdentifyValue {
  state: IdentifyState;
  dispatch: (action: IdentifyAction) => void;
  /** True while a job is running (or starting). */
  jobRunning: boolean;
  /** Scans a folder and moves to Confirm show. */
  openFolder(folder: string): Promise<void>;
  /** Starts identifying with the confirmed show. */
  start(request: JobRequest): Promise<void>;
  /** Cancels the running job. */
  cancel(): Promise<void>;
  /** Reopens a recent job's results in Review. */
  openRecent(jobId: JobId): Promise<void>;
  /** Details of a scanned file, when the scan is known. */
  fileInfo(fileId: FileId): MediaFile | undefined;
}

const IdentifyContext = createContext<IdentifyValue | null>(null);

export function IdentifyProvider({ children }: { children: ReactNode }) {
  const backend = useBackend();
  const [state, dispatch] = useReducer(identifyReducer, initialIdentifyState);

  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | null = null;
    void backend.onJobEvent((event) => dispatch({ type: "event", event })).then((u) => {
      if (active) unsubscribe = u;
      else u();
    });
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [backend]);

  // When a job finishes, fetch its saved results (authoritative), and move from the progress
  // screen to Review.
  const phase = state.job?.phase;
  const jobId = state.job?.jobId;
  useEffect(() => {
    if (phase !== "finished" || !jobId) return;
    let active = true;
    backend
      .jobResults(jobId)
      .then((results) => active && dispatch({ type: "results", results }))
      .catch(() => {});
    dispatch({ type: "autoReview" });
    return () => {
      active = false;
    };
  }, [backend, phase, jobId]);

  const openFolder = useCallback(
    async (folder: string) => {
      dispatch({ type: "scanStarted" });
      try {
        dispatch({ type: "scanned", scan: await backend.scanFolder(folder) });
      } catch (e) {
        dispatch({ type: "failed", message: toApiError(e).message });
      }
    },
    [backend],
  );

  const start = useCallback(
    async (request: JobRequest) => {
      dispatch({ type: "starting", request });
      try {
        dispatch({ type: "started", jobId: await backend.startIdentification(request) });
      } catch (e) {
        dispatch({ type: "startFailed", message: toApiError(e).message });
      }
    },
    [backend],
  );

  const runningId = state.job?.jobId ?? null;
  const cancel = useCallback(async () => {
    const id = runningId;
    if (!id) return;
    try {
      await backend.cancelIdentification(id);
    } catch (e) {
      dispatch({ type: "failed", message: toApiError(e).message });
    }
  }, [backend, runningId]);

  const openRecent = useCallback(
    async (id: JobId) => {
      try {
        const results = await backend.jobResults(id);
        // File details (length, size) come from the folder; the results alone have ids only.
        const scan = await backend.scanFolder(results.request.folder).catch(() => null);
        dispatch({ type: "opened", results, scan });
      } catch (e) {
        dispatch({ type: "failed", message: toApiError(e).message });
      }
    },
    [backend],
  );

  const files = useMemo(() => new Map((state.scan?.files ?? []).map((f) => [f.id, f])), [state.scan]);
  const fileInfo = useCallback((id: FileId) => files.get(id), [files]);

  const jobRunning = state.starting || state.job?.phase === "running";

  const value = useMemo(
    () => ({ state, dispatch, jobRunning, openFolder, start, cancel, openRecent, fileInfo }),
    [state, jobRunning, openFolder, start, cancel, openRecent, fileInfo],
  );
  return <IdentifyContext.Provider value={value}>{children}</IdentifyContext.Provider>;
}

export function useIdentify(): IdentifyValue {
  const value = useContext(IdentifyContext);
  if (!value) throw new Error("useIdentify must be used inside <IdentifyProvider>");
  return value;
}
