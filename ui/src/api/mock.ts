// In-memory `Backend` for tests and `npm run dev:mock`. Simulates jobs, model downloads and
// updates with timers so screens can be built and tested without the Rust app.

import type {
  HistoryEntry,
  JobEvent,
  JobResults,
  ModelStatus,
  RenamePlan,
  Settings,
  SpeechModel,
  UntouchedFile,
  UpdateCheck,
  UpdateEvent,
  UpdateInfo,
} from "../types/generated";
import type { Backend, Unsubscribe } from "./backend";
import { apiError } from "./errors";
import {
  EPISODES,
  HISTORY,
  MATCHES,
  RECENT,
  SAMPLE_FOLDER,
  SCAN,
  SHOW_CANDIDATES,
  SOURCES,
} from "./mockData";

export interface MockOptions {
  /** Delay between simulated progress steps, ms. Tests use 0 with fake timers. */
  stepMs?: number;
  /** What "Check now" answers. */
  update?: "available" | "upToDate" | "failed";
  /** Initial model state. */
  modelReady?: boolean;
}

const MODEL_SIZES: Record<SpeechModel, number> = { fast: 190_098_681, accurate: 574_041_195 };

const DEFAULT_SETTINGS: Settings = {
  speechModel: "accurate",
  sampleLongFiles: true,
  language: "en",
  checkUpdatesAutomatically: true,
  lastUpdateCheckMs: null,
  skippedUpdateVersion: null,
  naming: { kind: "jellyfinPlex" },
  saveMode: "renameInPlace",
  saveHeardSubtitles: false,
};

const UPDATE: UpdateInfo = {
  version: "1.3.0",
  currentVersion: "1.2.1",
  notes: "Disc order is now read from VIDEO_TS folders.\nKodi naming.\nFixed: very short files were skipped.",
  date: null,
};

class Emitter<T> {
  private listeners = new Set<(value: T) => void>();
  subscribe(listener: (value: T) => void): Promise<Unsubscribe> {
    this.listeners.add(listener);
    return Promise.resolve(() => this.listeners.delete(listener));
  }
  emit(value: T) {
    for (const listener of [...this.listeners]) listener(value);
  }
}

/** Creates a fresh mock backend with its own state. */
export function createMockBackend(options: MockOptions = {}): Backend {
  const stepMs = options.stepMs ?? 120;
  const jobEvents = new Emitter<JobEvent>();
  const modelEvents = new Emitter<ModelStatus>();
  const updateEvents = new Emitter<UpdateEvent>();
  const updateAvailable = new Emitter<UpdateInfo>();

  let settings: Settings = structuredClone(DEFAULT_SETTINGS);
  const keys = new Set<string>();
  let history: HistoryEntry[] = structuredClone(HISTORY);
  const modelReady: Record<SpeechModel, boolean> = {
    fast: false,
    accurate: options.modelReady ?? false,
  };
  let runningJob: string | null = null;
  let cancelled = false;
  let nextJob = 1;
  const results = new Map<string, JobResults>();
  let updateDownloaded = false;
  let updateDownloadCancelled = false;

  const later = (fn: () => void, steps = 1) => setTimeout(fn, stepMs * steps);

  function modelStatus(model: SpeechModel): ModelStatus {
    const total = MODEL_SIZES[model];
    return {
      info: {
        model,
        fileName: model === "accurate" ? "ggml-large-v3-turbo-q5_0.bin" : "ggml-small.en-q5_1.bin",
        sizeBytes: total,
        sha256: "",
        url: "",
      },
      state: modelReady[model] ? { kind: "ready" } : { kind: "missing" },
    };
  }

  const backend: Backend = {
    appVersion: async () => "1.2.1",

    getSettings: async () => structuredClone(settings),
    saveSettings: async (next) => {
      settings = structuredClone(next);
    },
    setApiKey: async (provider, key) => {
      if (key && key.trim()) keys.add(provider);
      else keys.delete(provider);
    },
    sourceStatus: async () =>
      SOURCES.map((s) =>
        keys.has(s.provider) ? { ...s, hasKey: true, state: { kind: "ready" } } : s,
      ),

    modelStatus: async (model) => modelStatus(model),
    downloadModel: (model) =>
      new Promise<void>((resolve) => {
        const total = MODEL_SIZES[model];
        const base = modelStatus(model);
        const steps = 4;
        for (let i = 1; i <= steps; i++) {
          later(() => {
            modelEvents.emit({
              ...base,
              state: {
                kind: "downloading",
                downloaded: Math.round((total * i) / steps),
                total,
                bytesPerSecond: 8_000_000,
              },
            });
          }, i);
        }
        later(() => {
          modelReady[model] = true;
          modelEvents.emit(modelStatus(model));
          resolve();
        }, steps + 1);
      }),
    pauseModelDownload: async () => {},

    chooseFolder: async () => SAMPLE_FOLDER,
    openUrl: async () => {},

    scanFolder: async (folder) => ({ ...structuredClone(SCAN), folder }),
    searchShows: async (query) =>
      SHOW_CANDIDATES.filter((c) =>
        c.show.name.toLowerCase().includes(query.trim().toLowerCase()),
      ),

    startIdentification: async (request) => {
      if (runningJob) throw apiError("busy", "An identification is already running.");
      const jobId = `mock-job-${nextJob++}`;
      runningJob = jobId;
      cancelled = false;
      const fileIds = SCAN.files.map((f) => f.id);
      results.set(jobId, {
        jobId,
        request,
        episodes: EPISODES,
        matches: [],
        model: settings.speechModel,
        complete: false,
      });
      const events: JobEvent[] = [
        { kind: "started", jobId, fileIds, accelerator: "appleGpu" },
        { kind: "stage", jobId, stage: "episodeList", state: { kind: "done" } },
        { kind: "episodes", jobId, episodes: EPISODES },
        { kind: "stage", jobId, stage: "subtitles", state: { kind: "done" } },
        { kind: "stage", jobId, stage: "discOrder", state: { kind: "done" } },
        ...MATCHES.flatMap((m): JobEvent[] => [
          { kind: "file", jobId, fileId: m.fileId, status: "listening", bestSoFar: null, verdict: null },
          {
            kind: "file",
            jobId,
            fileId: m.fileId,
            status: "done",
            bestSoFar: m.candidates[0]?.title ?? null,
            verdict: m.confidence.verdict,
          },
          { kind: "matched", jobId, result: m },
        ]),
        { kind: "stage", jobId, stage: "listening", state: { kind: "done" } },
        { kind: "stage", jobId, stage: "matching", state: { kind: "done" } },
        { kind: "finished", jobId },
      ];
      events.forEach((event, i) =>
        later(() => {
          if (cancelled || runningJob !== jobId) return;
          const r = results.get(jobId)!;
          if (event.kind === "matched") r.matches = [...r.matches, event.result];
          if (event.kind === "finished") {
            r.complete = true;
            runningJob = null;
          }
          jobEvents.emit(event);
        }, i + 1),
      );
      return jobId;
    },
    cancelIdentification: async (jobId) => {
      if (runningJob !== jobId) return;
      cancelled = true;
      runningJob = null;
      later(() => jobEvents.emit({ kind: "cancelled", jobId }));
    },
    jobResults: async (jobId) => {
      const r = results.get(jobId);
      if (!r) throw apiError("notFound", `Job ${jobId} not found.`);
      return structuredClone(r);
    },
    recentJobs: async () => structuredClone(RECENT),

    planRename: async (request): Promise<RenamePlan> => {
      const show = "Schoolhouse Rock! (1973)";
      const items = request.decisions.flatMap((d) => {
        if (d.decision.kind !== "approved") return [];
        const ep = EPISODES.find(
          (e) =>
            d.decision.kind === "approved" &&
            e.key.season === d.decision.episode.season &&
            e.key.number === d.decision.episode.number,
        );
        if (!ep) return [];
        const s = String(ep.key.season).padStart(2, "0");
        const n = String(ep.key.number).padStart(2, "0");
        return [
          {
            fileId: d.fileId,
            from: `${SAMPLE_FOLDER}/${d.fileId}`,
            to: `${SAMPLE_FOLDER}/${show}/Season ${s}/${show} - S${s}E${n} - ${ep.title}.mkv`,
            episode: ep.key,
            title: ep.title,
            heardSubtitlesTo: null,
          },
        ];
      });
      const untouched: UntouchedFile[] = [
        { fileId: "title_t00.mkv", path: `${SAMPLE_FOLDER}/title_t00.mkv`, reason: "playAll" },
        ...request.decisions
          .filter((d) => d.decision.kind !== "approved")
          .map(
            (d): UntouchedFile => ({
              fileId: d.fileId,
              path: `${SAMPLE_FOLDER}/${d.fileId}`,
              reason: d.decision.kind === "notAnEpisode" ? "extra" : "skipped",
            }),
          ),
      ];
      return { jobId: request.jobId, mode: request.mode, items, untouched, conflicts: [] };
    },
    applyRename: async (plan) => {
      const id = `hist-${history.length + 1}`;
      history = [
        {
          id,
          createdAtMs: Date.now(),
          showName: "Schoolhouse Rock!",
          folder: SAMPLE_FOLDER,
          mode: "renameInPlace",
          items: plan.items.map((i) => ({ from: i.from, to: i.to })),
          undoneAtMs: null,
        },
        ...history,
      ];
      return { historyId: id, completed: plan.items.length, failed: [] };
    },
    listHistory: async () => structuredClone(history),
    undoHistory: async (id) => {
      const entry = history.find((h) => h.id === id);
      if (!entry) throw apiError("notFound", `History entry ${id} not found.`);
      entry.undoneAtMs = Date.now();
      return { restored: entry.items.length, failed: [] };
    },

    checkForUpdate: async (): Promise<UpdateCheck> => {
      switch (options.update ?? "available") {
        case "available":
          return { kind: "available", info: UPDATE };
        case "upToDate":
          return { kind: "upToDate", currentVersion: "1.2.1" };
        case "failed":
          return { kind: "failed", message: "release feed unreachable" };
      }
    },
    downloadUpdate: () =>
      new Promise<void>((resolve, reject) => {
        const total = 84_000_000;
        updateDownloadCancelled = false;
        [0.25, 0.5, 0.75, 1].forEach((f, i) =>
          later(() => {
            if (!updateDownloadCancelled) {
              updateEvents.emit({ kind: "downloading", downloaded: total * f, total });
            }
          }, i + 1),
        );
        later(() => {
          if (updateDownloadCancelled) {
            reject(apiError("cancelled", "The download was cancelled."));
            return;
          }
          updateDownloaded = true;
          updateEvents.emit({ kind: "downloaded", version: UPDATE.version });
          resolve();
        }, 5);
      }),
    cancelUpdateDownload: async () => {
      updateDownloadCancelled = true;
    },
    installUpdateAndRelaunch: async () => {
      if (runningJob) {
        throw apiError(
          "busy",
          "Media Identifier will relaunch to finish the update when identification finishes.",
        );
      }
      if (!updateDownloaded) throw apiError("notFound", "No update has been downloaded.");
    },
    skipUpdateVersion: async (version) => {
      settings = { ...settings, skippedUpdateVersion: version };
    },

    onJobEvent: (listener) => jobEvents.subscribe(listener),
    onModelDownload: (listener) => modelEvents.subscribe(listener),
    onUpdateEvent: (listener) => updateEvents.subscribe(listener),
    onUpdateAvailable: (listener) => updateAvailable.subscribe(listener),
  };
  return backend;
}
