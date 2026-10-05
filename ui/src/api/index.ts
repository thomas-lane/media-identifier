import { createContext, useContext } from "react";

import type { Backend } from "./backend";
import { createMockBackend, mockOptionsFromUrl } from "./mock";
import { tauriBackend } from "./tauri";

export type { Backend, FileDropEvent, Unsubscribe } from "./backend";
export { isApiError, toApiError } from "./errors";
export { createMockBackend, MOCK_UPDATE } from "./mock";

/** True inside the Tauri window (Tauri injects `__TAURI_INTERNALS__`). */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * The backend for this run: the mock when built with `VITE_BACKEND=mock` (`npm run dev:mock`) or
 * when not running inside Tauri (a plain browser), otherwise Tauri. In the browser the mock reads
 * simulation options from the page URL (see `mockOptionsFromUrl`) and treats any drop on the
 * window as the sample folder.
 */
export function getBackend(): Backend {
  if (import.meta.env.VITE_BACKEND === "mock" || !isTauri()) {
    return createMockBackend({ stepMs: 250, downloadStepMs: 400, ...mockOptionsFromUrl(), windowDrops: true });
  }
  return tauriBackend;
}

/** React context carrying the backend; tests provide a mock. */
export const BackendContext = createContext<Backend | null>(null);

/** The backend from context. */
export function useBackend(): Backend {
  const backend = useContext(BackendContext);
  if (!backend) throw new Error("useBackend must be used inside <BackendContext.Provider>");
  return backend;
}
