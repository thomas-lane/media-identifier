// The update flow: an "update available" dialog (from the launch-time check or "Check now"),
// download progress after the user chooses Install, and the "Relaunch now" banner. Nothing
// downloads until the user chooses Install, and nothing interrupts a running identification:
// a background announcement waits until the job ends, and Relaunch is disabled while it runs.

import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";

import { toApiError, useBackend } from "../api";
import type { UpdateCheck, UpdateInfo } from "../types/generated";

export type UpdateDialogState =
  | { kind: "closed" }
  | { kind: "available"; info: UpdateInfo }
  | { kind: "downloading"; info: UpdateInfo; downloaded: number; total: number | null }
  | { kind: "failed"; info: UpdateInfo; message: string };

export interface UpdatesValue {
  dialog: UpdateDialogState;
  /** Version downloaded and waiting for a relaunch; null when none. */
  readyVersion: string | null;
  /** Whether the "Update downloaded" banner shows (hidden after "Later"). */
  bannerVisible: boolean;
  /** Message after a refused relaunch. */
  relaunchError: string | null;
  /** "Check now": opens the dialog when a version is available and returns the result. */
  checkNow(): Promise<UpdateCheck>;
  install(): Promise<void>;
  skip(): Promise<void>;
  remindLater(): void;
  /** Hides the download dialog; the download continues and the banner appears when done. */
  hideDownload(): void;
  /** Stops the download; nothing is kept and the dialog closes. */
  cancelDownload(): Promise<void>;
  relaunch(): Promise<void>;
  dismissBanner(): void;
}

const UpdatesContext = createContext<UpdatesValue | null>(null);

export function UpdatesProvider({ jobRunning, children }: { jobRunning: boolean; children: ReactNode }) {
  const backend = useBackend();
  const [dialog, setDialog] = useState<UpdateDialogState>({ kind: "closed" });
  const [queued, setQueued] = useState<UpdateInfo | null>(null);
  const [readyVersion, setReadyVersion] = useState<string | null>(null);
  const [bannerVisible, setBannerVisible] = useState(false);
  const [hidden, setHidden] = useState(false);
  const [relaunchError, setRelaunchError] = useState<string | null>(null);

  // Launch-time announcement: queue it, then show it when no job is running.
  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | null = null;
    void backend.onUpdateAvailable((info) => setQueued(info)).then((u) => {
      if (active) unsubscribe = u;
      else u();
    });
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [backend]);

  // The dialog on screen: an explicit state, or a queued announcement once no job runs.
  const current: UpdateDialogState = useMemo(
    () =>
      dialog.kind === "closed" && queued && !jobRunning && !readyVersion ? { kind: "available", info: queued } : dialog,
    [dialog, queued, jobRunning, readyVersion],
  );

  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | null = null;
    void backend
      .onUpdateEvent((event) => {
        if (event.kind === "downloading") {
          setDialog((d) =>
            d.kind === "downloading" || d.kind === "available"
              ? { kind: "downloading", info: d.info, downloaded: event.downloaded, total: event.total }
              : d,
          );
        } else if (event.kind === "downloaded") {
          setReadyVersion(event.version);
          setBannerVisible(true);
          setDialog({ kind: "closed" });
        } else {
          setDialog((d) => (d.kind === "closed" ? d : { kind: "failed", info: d.info, message: event.message }));
        }
      })
      .then((u) => {
        if (active) unsubscribe = u;
        else u();
      });
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [backend]);

  const checkNow = useCallback(async () => {
    let result: UpdateCheck;
    try {
      result = await backend.checkForUpdate();
    } catch (e) {
      result = { kind: "failed", message: toApiError(e).message };
    }
    if (result.kind === "available" && !readyVersion) setDialog({ kind: "available", info: result.info });
    return result;
  }, [backend, readyVersion]);

  const install = useCallback(async () => {
    if (current.kind !== "available" && current.kind !== "failed") return;
    const info = current.info;
    setQueued(null);
    setHidden(false);
    setDialog({ kind: "downloading", info, downloaded: 0, total: null });
    try {
      await backend.downloadUpdate();
    } catch (e) {
      const err = toApiError(e);
      setDialog(err.code === "cancelled" ? { kind: "closed" } : { kind: "failed", info, message: err.message });
    }
  }, [backend, current]);

  const skip = useCallback(async () => {
    if (current.kind !== "available") return;
    const version = current.info.version;
    setQueued(null);
    setDialog({ kind: "closed" });
    try {
      await backend.skipUpdateVersion(version);
    } catch {
      // Skipping only affects future announcements; a failure here changes nothing visible.
    }
  }, [backend, current]);

  const remindLater = useCallback(() => {
    setQueued(null);
    setDialog({ kind: "closed" });
  }, []);
  const hideDownload = useCallback(() => setHidden(true), []);
  const cancelDownload = useCallback(async () => {
    setDialog({ kind: "closed" });
    try {
      await backend.cancelUpdateDownload();
    } catch {
      // The download already ended; its own result decides what shows.
    }
  }, [backend]);
  const dismissBanner = useCallback(() => setBannerVisible(false), []);

  const relaunch = useCallback(async () => {
    setRelaunchError(null);
    try {
      await backend.installUpdateAndRelaunch();
    } catch (e) {
      setRelaunchError(toApiError(e).message);
    }
  }, [backend]);

  const value = useMemo(
    () => ({
      dialog: hidden && current.kind === "downloading" ? ({ kind: "closed" } as const) : current,
      readyVersion,
      bannerVisible,
      relaunchError,
      checkNow,
      install,
      skip,
      remindLater,
      hideDownload,
      cancelDownload,
      relaunch,
      dismissBanner,
    }),
    [
      hidden,
      current,
      readyVersion,
      bannerVisible,
      relaunchError,
      checkNow,
      install,
      skip,
      remindLater,
      hideDownload,
      cancelDownload,
      relaunch,
      dismissBanner,
    ],
  );
  return <UpdatesContext.Provider value={value}>{children}</UpdatesContext.Provider>;
}

export function useUpdates(): UpdatesValue {
  const value = useContext(UpdatesContext);
  if (!value) throw new Error("useUpdates must be used inside <UpdatesProvider>");
  return value;
}
