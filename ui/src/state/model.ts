// Download state of one speech model, kept current by `model-download` events.

import { useCallback, useEffect, useState } from "react";

import { toApiError, useBackend } from "../api";
import type { ModelStatus, SpeechModel } from "../types/generated";

export interface ModelHandle {
  /** Null until loaded. */
  status: ModelStatus | null;
  /** Starts or resumes the download. */
  download(): Promise<void>;
  /** Pauses it. */
  pause(): Promise<void>;
  /** The last download error, in plain words. */
  error: string | null;
}

export function useModel(model: SpeechModel | null): ModelHandle {
  const backend = useBackend();
  const [status, setStatus] = useState<ModelStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!model) return;
    let active = true;
    let unsubscribe: (() => void) | null = null;
    void backend.onModelDownload((s) => {
      if (active && s.info.model === model) setStatus(s);
    }).then((u) => {
      if (active) unsubscribe = u;
      else u();
    });
    backend
      .modelStatus(model)
      .then((s) => active && setStatus(s))
      .catch((e: unknown) => active && setError(toApiError(e).message));
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [backend, model]);

  const download = useCallback(async () => {
    if (!model) return;
    setError(null);
    try {
      await backend.downloadModel(model);
      setStatus(await backend.modelStatus(model));
    } catch (e) {
      const err = toApiError(e);
      if (err.code !== "cancelled" && err.code !== "busy") setError(err.message);
    }
  }, [backend, model]);

  const pause = useCallback(async () => {
    try {
      await backend.pauseModelDownload();
    } catch (e) {
      setError(toApiError(e).message);
    }
  }, [backend]);

  return { status, download, pause, error };
}
