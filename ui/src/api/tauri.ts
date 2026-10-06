// Tauri implementation of `Backend`. Command names must match `COMMANDS` in
// src-tauri/src/commands.rs (a Rust test checks every name appears here as a string literal).

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";

import {
  JOB_EVENT,
  MODEL_DOWNLOAD_EVENT,
  UPDATE_AVAILABLE_EVENT,
  UPDATE_EVENT,
} from "../types/generated";
import type {
  JobEvent,
  ModelStatus,
  UpdateEvent,
  UpdateInfo,
} from "../types/generated";
import type { Backend, FileDropEvent, Unsubscribe } from "./backend";

async function subscribe<T>(channel: string, listener: (payload: T) => void): Promise<Unsubscribe> {
  return listen<T>(channel, (event) => listener(event.payload));
}

export const tauriBackend: Backend = {
  appVersion: () => invoke("app_version"),

  getSettings: () => invoke("get_settings"),
  saveSettings: (settings) => invoke("save_settings", { settings }),
  setApiKey: (provider, key) => invoke("set_api_key", { provider, key }),
  sourceStatus: () => invoke("source_status"),
  attributions: () => invoke("attributions"),

  modelStatus: (model) => invoke("model_status", { model }),
  downloadModel: (model) => invoke("download_model", { model }),
  pauseModelDownload: () => invoke("pause_model_download"),

  chooseFolder: async () => {
    const picked = await open({ directory: true, multiple: false });
    return typeof picked === "string" ? picked : null;
  },
  chooseSaveFile: async (defaultPath) => {
    const picked = await save({ defaultPath, filters: [{ name: "CSV", extensions: ["csv"] }] });
    return typeof picked === "string" ? picked : null;
  },
  openUrl: (url) => openUrl(url),
  openFile: (path) => openPath(path),

  scanFolder: (folder) => invoke("scan_folder", { folder }),
  searchShows: (query) => invoke("search_shows", { query }),
  startIdentification: (request) => invoke("start_identification", { request }),
  cancelIdentification: (jobId) => invoke("cancel_identification", { jobId }),
  jobResults: (jobId) => invoke("job_results", { jobId }),
  recentJobs: () => invoke("recent_jobs"),

  planRename: (request) => invoke("plan_rename", { request }),
  applyRename: (plan) => invoke("apply_rename", { plan }),
  listHistory: () => invoke("list_history"),
  undoHistory: (id) => invoke("undo_history", { id }),

  checkForUpdate: () => invoke("check_for_update"),
  downloadUpdate: () => invoke("download_update"),
  cancelUpdateDownload: () => invoke("cancel_update_download"),
  installUpdateAndRelaunch: () => invoke("install_update_and_relaunch"),
  skipUpdateVersion: (version) => invoke("skip_update_version", { version }),

  onJobEvent: (listener) => subscribe<JobEvent>(JOB_EVENT, listener),
  onModelDownload: (listener) => subscribe<ModelStatus>(MODEL_DOWNLOAD_EVENT, listener),
  onUpdateEvent: (listener) => subscribe<UpdateEvent>(UPDATE_EVENT, listener),
  onUpdateAvailable: (listener) => subscribe<UpdateInfo>(UPDATE_AVAILABLE_EVENT, listener),
  onFileDrop: (listener) =>
    getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      let mapped: FileDropEvent | null = null;
      if (payload.type === "enter") mapped = { kind: "hover" };
      else if (payload.type === "drop") mapped = { kind: "drop", paths: payload.paths };
      else if (payload.type === "leave") mapped = { kind: "cancel" };
      if (mapped) listener(mapped);
    }),
};
