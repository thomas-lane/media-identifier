// In-memory `Backend` for tests and `npm run dev:mock`. Simulates jobs, model downloads, updates
// and file drops with timers so every screen can be built and tested without the Rust app.

import type {
  Episode,
  FileId,
  HistoryEntry,
  JobEvent,
  JobResults,
  ModelStatus,
  NamingScheme,
  PlanConflict,
  RenameItem,
  RenamePlan,
  ScanSummary,
  Settings,
  SpeechModel,
  UntouchedFile,
  UpdateCheck,
  UpdateEvent,
  UpdateInfo,
} from "../types/generated";
import type { Backend, FileDropEvent, Unsubscribe } from "./backend";
import { apiError } from "./errors";
import {
  ATTRIBUTIONS,
  EPISODES,
  MATCHES,
  SAMPLE_FOLDER,
  SCAN,
  SCHOOLHOUSE_ROCK,
  SHOW_CANDIDATES,
  SOURCES,
  historyEntries,
  recentJobs,
} from "./mockData";

export interface MockOptions {
  /** Delay between simulated job steps, ms. Tests use small values with fake timers. */
  stepMs?: number;
  /** Delay between simulated model download steps, ms (defaults to `stepMs`). */
  downloadStepMs?: number;
  /** What "Check now" answers. */
  update?: "available" | "upToDate" | "failed";
  /** Announce an available update this many ms after the first `onUpdateAvailable` subscriber. */
  announceUpdateAfterMs?: number | null;
  /** Initial state of the Accurate model (Fast always starts missing). */
  modelReady?: boolean;
  /** Listen for HTML drag-and-drop on `window` and report drops as the sample folder. */
  windowDrops?: boolean;
}

const MODEL_SIZES: Record<SpeechModel, number> = { fast: 190_098_681, accurate: 574_041_195 };
const DOWNLOAD_STEPS = 20;

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

export const MOCK_UPDATE: UpdateInfo = {
  version: "1.3.0",
  currentVersion: "1.2.1",
  notes:
    "Faster listening on long files.\nKodi naming.\nFixed: very short files were skipped.",
  date: null,
};

class Emitter<T> {
  private listeners = new Set<(value: T) => void>();
  get size() {
    return this.listeners.size;
  }
  subscribe(listener: (value: T) => void): Promise<Unsubscribe> {
    this.listeners.add(listener);
    return Promise.resolve(() => {
      this.listeners.delete(listener);
    });
  }
  emit(value: T) {
    for (const listener of [...this.listeners]) listener(value);
  }
}

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

/** The sample naming: a simplified stand-in for `mi_rename::render_relative_path`. */
function relativeName(naming: NamingScheme, episode: Episode, ext: string): string {
  const show = `${SCHOOLHOUSE_ROCK.name} (${SCHOOLHOUSE_ROCK.year})`;
  const s = pad2(episode.key.season);
  const e = pad2(episode.key.number);
  switch (naming.kind) {
    case "jellyfinPlex":
      return `${show}/Season ${s}/${show} - S${s}E${e} - ${episode.title}.${ext}`;
    case "kodi":
      return `${show}/Season ${s}/${SCHOOLHOUSE_ROCK.name} S${s}E${e} - ${episode.title}.${ext}`;
    case "custom":
      return naming.template
        .replaceAll("{show}", SCHOOLHOUSE_ROCK.name)
        .replaceAll("{year}", String(SCHOOLHOUSE_ROCK.year))
        .replaceAll("{season:02}", s)
        .replaceAll("{season}", String(episode.key.season))
        .replaceAll("{episode:02}", e)
        .replaceAll("{episode}", String(episode.key.number))
        .replaceAll("{title}", episode.title)
        .replaceAll("{ext}", ext);
  }
}

function readUrlOptions(): MockOptions {
  if (typeof location === "undefined") return {};
  const q = new URLSearchParams(location.search);
  const num = (name: string) => (q.has(name) ? Number(q.get(name)) : undefined);
  const update = q.get("update");
  return {
    stepMs: num("stepMs"),
    downloadStepMs: num("downloadStepMs"),
    update: update === "upToDate" || update === "failed" || update === "available" ? update : undefined,
    announceUpdateAfterMs: num("announceUpdate") ?? null,
    modelReady: q.get("model") === "ready" ? true : undefined,
  };
}

/** Options for `npm run dev:mock`, read from the page URL (for example `?stepMs=400&update=failed`). */
export function mockOptionsFromUrl(): MockOptions {
  const o = readUrlOptions();
  return Object.fromEntries(Object.entries(o).filter(([, v]) => v !== undefined)) as MockOptions;
}

/** Creates a fresh mock backend with its own state. */
export function createMockBackend(options: MockOptions = {}): Backend {
  const stepMs = options.stepMs ?? 120;
  const downloadStepMs = options.downloadStepMs ?? stepMs;
  const jobEvents = new Emitter<JobEvent>();
  const modelEvents = new Emitter<ModelStatus>();
  const updateEvents = new Emitter<UpdateEvent>();
  const updateAvailable = new Emitter<UpdateInfo>();
  const dropEvents = new Emitter<FileDropEvent>();

  let settings: Settings = structuredClone(DEFAULT_SETTINGS);
  const keys = new Set<string>();
  let history: HistoryEntry[] = historyEntries();
  const downloaded: Record<SpeechModel, number> = {
    fast: 0,
    accurate: options.modelReady ? MODEL_SIZES.accurate : 0,
  };
  let downloading: { model: SpeechModel; timer: ReturnType<typeof setInterval>; reject: (e: unknown) => void } | null = null;
  let runningJob: string | null = null;
  let cancelled = false;
  let nextJob = 1;
  const results = new Map<string, JobResults>();
  let updateDownloaded = false;
  let updateDownloadCancelled = false;
  let announced = false;
  let windowDropsAttached = false;

  const later = (fn: () => void, steps = 1) => setTimeout(fn, stepMs * steps);

  results.set("job-disc2", {
    jobId: "job-disc2",
    request: {
      folder: "/Volumes/Rips/SCHOOLHOUSE_ROCK_D2",
      show: SCHOOLHOUSE_ROCK,
      ordering: "aired",
      seasons: null,
      language: "en",
    },
    episodes: EPISODES,
    matches: MATCHES,
    model: "accurate",
    complete: true,
  });

  function modelStatus(model: SpeechModel): ModelStatus {
    const total = MODEL_SIZES[model];
    const done = downloaded[model];
    let state: ModelStatus["state"];
    if (done >= total) state = { kind: "ready" };
    else if (downloading?.model === model) state = { kind: "downloading", downloaded: done, total, bytesPerSecond: 9_500_000 };
    else if (done > 0) state = { kind: "paused", downloaded: done, total };
    else state = { kind: "missing" };
    return {
      info: {
        model,
        fileName: model === "accurate" ? "ggml-large-v3-turbo-q5_0.bin" : "ggml-small.en-q5_1.bin",
        sizeBytes: total,
        sha256: "",
        url: "",
      },
      state,
    };
  }

  function jobEventsFor(jobId: string): JobEvent[] {
    const candidates = MATCHES.filter((m) => m.suggestion.kind !== "playAll");
    const playAll = MATCHES.filter((m) => m.suggestion.kind === "playAll");
    const fileIds: FileId[] = [...candidates, ...playAll].map((m) => m.fileId);
    const n = candidates.length;
    const events: JobEvent[] = [
      { kind: "started", jobId, fileIds, accelerator: "appleGpu" },
      { kind: "stage", jobId, stage: "episodeList", state: { kind: "running", done: 0, total: 1 } },
      { kind: "stage", jobId, stage: "episodeList", state: { kind: "done" } },
      { kind: "episodes", jobId, episodes: EPISODES },
      { kind: "eta", jobId, seconds: n * 18 + 20 },
    ];
    for (let i = 1; i <= 3; i++) {
      events.push({ kind: "stage", jobId, stage: "subtitles", state: { kind: "running", done: i * 4, total: 13 } });
    }
    events.push({ kind: "stage", jobId, stage: "subtitles", state: { kind: "done" } });
    events.push({ kind: "stage", jobId, stage: "discOrder", state: { kind: "running", done: 0, total: n } });
    for (const m of playAll) {
      events.push({ kind: "file", jobId, fileId: m.fileId, status: "done", bestSoFar: null, verdict: "playAll" });
      events.push({ kind: "matched", jobId, result: m });
    }
    events.push({ kind: "stage", jobId, stage: "discOrder", state: { kind: "done" } });
    candidates.forEach((m, i) => {
      events.push(
        { kind: "file", jobId, fileId: m.fileId, status: "listening", bestSoFar: null, verdict: null },
        { kind: "stage", jobId, stage: "listening", state: { kind: "running", done: i, total: n } },
        { kind: "file", jobId, fileId: m.fileId, status: "matching", bestSoFar: null, verdict: null },
        { kind: "stage", jobId, stage: "matching", state: { kind: "running", done: i, total: n } },
        {
          kind: "file",
          jobId,
          fileId: m.fileId,
          status: "done",
          bestSoFar: m.candidates[0]?.title ?? null,
          verdict: m.confidence.verdict,
        },
        { kind: "matched", jobId, result: m },
        { kind: "eta", jobId, seconds: (n - i - 1) * 18 },
      );
    });
    events.push(
      { kind: "stage", jobId, stage: "listening", state: { kind: "done" } },
      { kind: "stage", jobId, stage: "matching", state: { kind: "done" } },
      { kind: "finished", jobId },
    );
    return events;
  }

  function attachWindowDrops() {
    if (windowDropsAttached || typeof window === "undefined") return;
    windowDropsAttached = true;
    let depth = 0;
    window.addEventListener("dragenter", (e) => {
      e.preventDefault();
      if (depth++ === 0) dropEvents.emit({ kind: "hover" });
    });
    window.addEventListener("dragover", (e) => e.preventDefault());
    window.addEventListener("dragleave", () => {
      if (--depth <= 0) {
        depth = 0;
        dropEvents.emit({ kind: "cancel" });
      }
    });
    window.addEventListener("drop", (e) => {
      e.preventDefault();
      depth = 0;
      dropEvents.emit({ kind: "drop", paths: [SAMPLE_FOLDER] });
    });
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
      SOURCES.map((s) => (keys.has(s.provider) ? { ...s, hasKey: true, state: { kind: "ready" } } : s)),
    attributions: async () => structuredClone(ATTRIBUTIONS),

    modelStatus: async (model) => modelStatus(model),
    downloadModel: (model) =>
      new Promise<void>((resolve, reject) => {
        if (downloaded[model] >= MODEL_SIZES[model]) {
          resolve();
          return;
        }
        if (downloading) {
          reject(apiError("busy", "A model is already downloading."));
          return;
        }
        const total = MODEL_SIZES[model];
        const timer = setInterval(() => {
          downloaded[model] = Math.min(total, downloaded[model] + Math.ceil(total / DOWNLOAD_STEPS));
          if (downloaded[model] >= total) {
            clearInterval(timer);
            downloading = null;
            modelEvents.emit({ ...modelStatus(model), state: { kind: "verifying" } });
            setTimeout(() => {
              modelEvents.emit(modelStatus(model));
              resolve();
            }, downloadStepMs);
          } else {
            modelEvents.emit(modelStatus(model));
          }
        }, downloadStepMs);
        downloading = { model, timer, reject };
        modelEvents.emit(modelStatus(model));
      }),
    pauseModelDownload: async () => {
      if (!downloading) return;
      const { model, timer, reject } = downloading;
      clearInterval(timer);
      downloading = null;
      modelEvents.emit(modelStatus(model));
      reject(apiError("cancelled", "Download paused."));
    },

    chooseFolder: async () => SAMPLE_FOLDER,
    chooseSaveFile: async (defaultPath) => defaultPath,
    openUrl: async (url) => {
      if (typeof window !== "undefined" && typeof window.open === "function" && import.meta.env.MODE !== "test") {
        window.open(url, "_blank", "noopener");
      }
    },
    openFile: async () => {},

    scanFolder: async (folder): Promise<ScanSummary> => {
      if (!folder.trim()) throw apiError("invalidInput", "Choose a folder first.");
      return {
        ...structuredClone(SCAN),
        folder,
        files: SCAN.files.map((f) => ({ ...structuredClone(f), path: `${folder}/${f.fileName}` })),
      };
    },
    searchShows: async (query) =>
      SHOW_CANDIDATES.filter((c) => {
        const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
        return words.every((w) => c.show.name.toLowerCase().includes(w));
      }),

    startIdentification: async (request) => {
      if (runningJob) throw apiError("busy", "An identification is already running.");
      const jobId = `mock-job-${nextJob++}`;
      runningJob = jobId;
      cancelled = false;
      results.set(jobId, {
        jobId,
        request,
        episodes: [],
        matches: [],
        model: settings.speechModel,
        complete: false,
      });
      jobEventsFor(jobId).forEach((event, i) =>
        later(() => {
          if (cancelled || runningJob !== jobId) return;
          const r = results.get(jobId)!;
          if (event.kind === "episodes") r.episodes = event.episodes;
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
      if (!r) throw apiError("notFound", "These results are no longer available.");
      return structuredClone(r);
    },
    recentJobs: async () => recentJobs(),

    planRename: async (request): Promise<RenamePlan> => {
      const job = results.get(request.jobId);
      const folder = job?.request.folder ?? SAMPLE_FOLDER;
      const root =
        request.mode.kind === "renameInPlace" ? request.mode.root : request.mode.kind === "copyToFolder" ? request.mode.destination : folder;
      const items: RenameItem[] = [];
      const untouched: UntouchedFile[] = [];
      for (const m of job?.matches ?? MATCHES) {
        if (m.suggestion.kind === "playAll") {
          untouched.push({ fileId: m.fileId, path: `${folder}/${m.fileId}`, reason: "playAll" });
        }
      }
      for (const d of request.decisions) {
        const from = `${folder}/${d.fileId}`;
        if (d.decision.kind !== "approved") {
          untouched.push({ fileId: d.fileId, path: from, reason: d.decision.kind === "notAnEpisode" ? "extra" : "skipped" });
          continue;
        }
        const key = d.decision.episode;
        const ep = EPISODES.find((e) => e.key.season === key.season && e.key.number === key.number);
        if (!ep) continue;
        const ext = d.fileId.split(".").pop() ?? "mkv";
        const to = `${root}/${relativeName(request.naming, ep, ext)}`;
        items.push({
          fileId: d.fileId,
          from,
          to,
          episode: ep.key,
          title: ep.title,
          heardSubtitlesTo: request.saveHeardSubtitles ? to.replace(/\.[^.]+$/, ".srt") : null,
          sizeBytes: 0,
        });
      }
      const byTarget = new Map<string, FileId[]>();
      for (const item of items) byTarget.set(item.to, [...(byTarget.get(item.to) ?? []), item.fileId]);
      const conflicts: PlanConflict[] = [...byTarget.entries()]
        .filter(([, ids]) => ids.length > 1)
        .map(([path, fileIds]) => ({ kind: "duplicateTarget", fileIds, path }));
      return { jobId: request.jobId, request: structuredClone(request), mode: request.mode, items, untouched, conflicts };
    },
    applyRename: async (plan) => {
      if (plan.conflicts.length > 0) throw apiError("conflict", "Resolve the conflicts first.");
      // The app builds the plan again from its request and refuses one that differs.
      const rebuilt = await backend.planRename(plan.request);
      if (JSON.stringify(rebuilt.items) !== JSON.stringify(plan.items) || JSON.stringify(rebuilt.mode) !== JSON.stringify(plan.mode)) {
        throw apiError("invalidInput", "The rename preview no longer matches the identified files or the chosen settings. Check the preview and try again.");
      }
      if (plan.mode.kind === "exportList") return { historyId: null, completed: plan.items.length, failed: [] };
      const id = `hist-${history.length + 1}-${Date.now()}`;
      const job = results.get(plan.jobId);
      history = [
        {
          id,
          createdAtMs: Date.now(),
          showName: job?.request.show.name ?? SCHOOLHOUSE_ROCK.name,
          folder: job?.request.folder ?? SAMPLE_FOLDER,
          mode: plan.mode.kind,
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
      if (!entry) throw apiError("notFound", "This History entry no longer exists.");
      if (entry.undoneAtMs !== null) throw apiError("conflict", "This was already undone.");
      entry.undoneAtMs = Date.now();
      return { restored: entry.items.length, failed: [] };
    },

    checkForUpdate: async (): Promise<UpdateCheck> => {
      settings = { ...settings, lastUpdateCheckMs: Date.now() };
      switch (options.update ?? "available") {
        case "available":
          return { kind: "available", info: MOCK_UPDATE };
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
        [0.1, 0.3, 0.57, 0.8, 1].forEach((f, i) =>
          later(() => {
            if (!updateDownloadCancelled) {
              updateEvents.emit({ kind: "downloading", downloaded: Math.round(total * f), total });
            }
          }, i + 1),
        );
        later(() => {
          if (updateDownloadCancelled) {
            reject(apiError("cancelled", "The download was cancelled."));
            return;
          }
          updateDownloaded = true;
          updateEvents.emit({ kind: "downloaded", version: MOCK_UPDATE.version });
          resolve();
        }, 6);
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
      if (typeof location !== "undefined" && import.meta.env.MODE !== "test") location.reload();
    },
    skipUpdateVersion: async (version) => {
      settings = { ...settings, skippedUpdateVersion: version };
    },

    onJobEvent: (listener) => jobEvents.subscribe(listener),
    onModelDownload: (listener) => modelEvents.subscribe(listener),
    onUpdateEvent: (listener) => updateEvents.subscribe(listener),
    onUpdateAvailable: (listener) => {
      const sub = updateAvailable.subscribe(listener);
      const after = options.announceUpdateAfterMs;
      if (after !== null && after !== undefined && !announced) {
        announced = true;
        setTimeout(() => {
          if (settings.skippedUpdateVersion !== MOCK_UPDATE.version) updateAvailable.emit(MOCK_UPDATE);
        }, after);
      }
      return sub;
    },
    onFileDrop: (listener) => {
      if (options.windowDrops) attachWindowDrops();
      return dropEvents.subscribe(listener);
    },
  };
  return backend;
}
