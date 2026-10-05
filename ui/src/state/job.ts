// The Identifying screen's view of a job, built by folding `JobEvent`s. Pure, so the event
// handling is tested without a backend or timers.

import type {
  Accelerator,
  Episode,
  FileId,
  FileMatch,
  FileStatus,
  JobEvent,
  JobId,
  JobResults,
  Stage,
  StageState,
  Verdict,
} from "../types/generated";

/** Stages in the order the Identifying screen lists them. */
export const STAGES: Stage[] = ["episodeList", "subtitles", "discOrder", "listening", "matching"];

/** Per-file progress. */
export interface FileProgress {
  status: FileStatus;
  bestSoFar: string | null;
  verdict: Verdict | null;
}

/** Where a job is. */
export type JobPhase = "running" | "finished" | "cancelled" | "failed";

/** Everything the UI knows about one job. */
export interface JobView {
  jobId: JobId;
  phase: JobPhase;
  failure: string | null;
  accelerator: Accelerator | null;
  fileIds: FileId[];
  stages: Record<Stage, StageState>;
  files: Record<FileId, FileProgress>;
  episodes: Episode[];
  matches: Record<FileId, FileMatch>;
  etaSeconds: number | null;
}

function waitingStages(): Record<Stage, StageState> {
  return Object.fromEntries(STAGES.map((s) => [s, { kind: "waiting" }])) as Record<Stage, StageState>;
}

/** A job that has just been started. */
export function newJob(jobId: JobId): JobView {
  return {
    jobId,
    phase: "running",
    failure: null,
    accelerator: null,
    fileIds: [],
    stages: waitingStages(),
    files: {},
    episodes: [],
    matches: {},
    etaSeconds: null,
  };
}

/** A finished job rebuilt from saved results (reopened from Recent). */
export function jobFromResults(results: JobResults): JobView {
  const job = newJob(results.jobId);
  // Saved results that are not complete belong to a job that stopped early.
  job.phase = results.complete ? "finished" : "cancelled";
  job.episodes = results.episodes;
  for (const m of results.matches) {
    job.fileIds.push(m.fileId);
    job.matches[m.fileId] = m;
    job.files[m.fileId] = {
      status: "done",
      bestSoFar: m.candidates[0]?.title ?? null,
      verdict: m.confidence.verdict,
    };
  }
  if (results.complete) job.stages = Object.fromEntries(STAGES.map((s) => [s, { kind: "done" }])) as Record<Stage, StageState>;
  return job;
}

/** Applies one event. Events of other jobs are ignored. */
export function applyJobEvent(job: JobView, event: JobEvent): JobView {
  if (event.jobId !== job.jobId) return job;
  switch (event.kind) {
    case "started": {
      const files = { ...job.files };
      for (const id of event.fileIds) {
        files[id] ??= { status: "waiting", bestSoFar: null, verdict: null };
      }
      return { ...job, fileIds: event.fileIds, files, accelerator: event.accelerator };
    }
    case "stage":
      return { ...job, stages: { ...job.stages, [event.stage]: event.state } };
    case "file":
      return {
        ...job,
        fileIds: job.fileIds.includes(event.fileId) ? job.fileIds : [...job.fileIds, event.fileId],
        files: {
          ...job.files,
          [event.fileId]: { status: event.status, bestSoFar: event.bestSoFar, verdict: event.verdict },
        },
      };
    case "episodes":
      return { ...job, episodes: event.episodes };
    case "matched": {
      const m = event.result;
      const prev = job.files[m.fileId];
      return {
        ...job,
        fileIds: job.fileIds.includes(m.fileId) ? job.fileIds : [...job.fileIds, m.fileId],
        matches: { ...job.matches, [m.fileId]: m },
        files: {
          ...job.files,
          [m.fileId]: {
            status: "done",
            bestSoFar: m.candidates[0]?.title ?? prev?.bestSoFar ?? null,
            verdict: m.confidence.verdict,
          },
        },
      };
    }
    case "eta":
      return { ...job, etaSeconds: event.seconds };
    case "finished":
      return { ...job, phase: "finished", etaSeconds: null };
    case "cancelled":
      return { ...job, phase: "cancelled", etaSeconds: null };
    case "failed":
      return { ...job, phase: "failed", failure: event.message, etaSeconds: null };
  }
}

/** True for the play-all title, known from the scan or from its result. */
export function isPlayAll(job: JobView, fileId: FileId, scanPlayAllId: FileId | null): boolean {
  return fileId === scanPlayAllId || job.files[fileId]?.verdict === "playAll" || job.matches[fileId]?.suggestion.kind === "playAll";
}

/** Progress over the files being identified (the play-all is not one of them). */
export function fileProgress(job: JobView, scanPlayAllId: FileId | null): { done: number; total: number } {
  const ids = job.fileIds.filter((id) => !isPlayAll(job, id, scanPlayAllId));
  return { done: ids.filter((id) => job.matches[id]).length, total: ids.length };
}
