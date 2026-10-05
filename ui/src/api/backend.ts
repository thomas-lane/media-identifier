// The typed boundary between the UI and the app. Screens use only this interface (through
// `useBackend()` / `getBackend()`); `tauri.ts` implements it with Tauri commands and events,
// `mock.ts` with in-memory sample data for tests and `npm run dev:mock`.

import type {
  ApiKeyProvider,
  HistoryEntry,
  HistoryId,
  JobEvent,
  JobId,
  JobRequest,
  JobResults,
  ModelStatus,
  RecentJob,
  RenameOutcome,
  RenamePlan,
  RenamePlanRequest,
  ScanSummary,
  Settings,
  ShowCandidate,
  SourceStatus,
  SpeechModel,
  UndoOutcome,
  UpdateCheck,
  UpdateEvent,
  UpdateInfo,
} from "../types/generated";

/** Stops a subscription. */
export type Unsubscribe = () => void;

/** Everything the UI can ask of the app. Every method rejects with an `ApiError` on failure. */
export interface Backend {
  /** Running app version. */
  appVersion(): Promise<string>;

  /** Current settings. */
  getSettings(): Promise<Settings>;
  /** Saves settings. */
  saveSettings(settings: Settings): Promise<void>;
  /** Sets (or clears with null) a user API key. Keys are never read back. */
  setApiKey(provider: ApiKeyProvider, key: string | null): Promise<void>;
  /** Status of each online source. */
  sourceStatus(): Promise<SourceStatus[]>;

  /** Download state of a speech model. */
  modelStatus(model: SpeechModel): Promise<ModelStatus>;
  /** Downloads or resumes a model; progress through `onModelDownload`. Resolves when done. */
  downloadModel(model: SpeechModel): Promise<void>;
  /** Pauses the running model download. */
  pauseModelDownload(): Promise<void>;

  /** Opens the system folder picker; null when cancelled. */
  chooseFolder(): Promise<string | null>;
  /** Opens a web link in the default browser (attribution links). */
  openUrl(url: string): Promise<void>;

  /** Scans a folder of video files. */
  scanFolder(folder: string): Promise<ScanSummary>;
  /** Searches shows. */
  searchShows(query: string): Promise<ShowCandidate[]>;
  /** Starts identifying; progress through `onJobEvent`. */
  startIdentification(request: JobRequest): Promise<JobId>;
  /** Cancels a job. */
  cancelIdentification(jobId: JobId): Promise<void>;
  /** Results of a job (partial while it runs). */
  jobResults(jobId: JobId): Promise<JobResults>;
  /** Recent jobs, newest first. */
  recentJobs(): Promise<RecentJob[]>;

  /** Builds a rename preview. */
  planRename(request: RenamePlanRequest): Promise<RenamePlan>;
  /** Applies a rename plan. */
  applyRename(plan: RenamePlan): Promise<RenameOutcome>;
  /** History entries, newest first. */
  listHistory(): Promise<HistoryEntry[]>;
  /** Undoes a History entry. */
  undoHistory(id: HistoryId): Promise<UndoOutcome>;

  /** "Check now". A failed check resolves to `{ kind: "failed" }`; it does not reject. */
  checkForUpdate(): Promise<UpdateCheck>;
  /** "Install update": downloads; progress through `onUpdateEvent`. */
  downloadUpdate(): Promise<void>;
  /** "Cancel" while an update downloads; `downloadUpdate` then rejects with code `cancelled`. */
  cancelUpdateDownload(): Promise<void>;
  /**
   * "Relaunch now". Rejects with code `busy` while identification runs; the app then installs
   * and relaunches by itself when the job ends.
   */
  installUpdateAndRelaunch(): Promise<void>;
  /** "Skip this version". */
  skipUpdateVersion(version: string): Promise<void>;

  /** Job progress events. */
  onJobEvent(listener: (event: JobEvent) => void): Promise<Unsubscribe>;
  /** Model download progress. */
  onModelDownload(listener: (status: ModelStatus) => void): Promise<Unsubscribe>;
  /** Update download progress. */
  onUpdateEvent(listener: (event: UpdateEvent) => void): Promise<Unsubscribe>;
  /** A background check found a new version. */
  onUpdateAvailable(listener: (info: UpdateInfo) => void): Promise<Unsubscribe>;
}
