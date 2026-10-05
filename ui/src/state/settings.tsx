// Settings shared by every screen: loaded once, saved on every change.

import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";

import { toApiError, useBackend } from "../api";
import type { Settings } from "../types/generated";

interface SettingsValue {
  /** Null until loaded. */
  settings: Settings | null;
  /** Merges `patch` into the settings and saves them. */
  update(patch: Partial<Settings>): Promise<void>;
  /** Reloads from the app (after the app changed them, such as an update check). */
  reload(): Promise<void>;
  error: string | null;
}

const SettingsContext = createContext<SettingsValue | null>(null);

export function SettingsProvider({ children }: { children: ReactNode }) {
  const backend = useBackend();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setSettings(await backend.getSettings());
    } catch (e) {
      setError(toApiError(e).message);
    }
  }, [backend]);

  useEffect(() => {
    let active = true;
    backend
      .getSettings()
      .then((s) => active && setSettings(s))
      .catch((e: unknown) => active && setError(toApiError(e).message));
    return () => {
      active = false;
    };
  }, [backend]);

  const update = useCallback(
    async (patch: Partial<Settings>) => {
      const current = settings ?? (await backend.getSettings());
      const next = { ...current, ...patch };
      setSettings(next);
      try {
        await backend.saveSettings(next);
        setError(null);
      } catch (e) {
        setError(toApiError(e).message);
      }
    },
    [backend, settings],
  );

  const value = useMemo(() => ({ settings, update, reload, error }), [settings, update, reload, error]);
  return <SettingsContext.Provider value={value}>{children}</SettingsContext.Provider>;
}

export function useSettings(): SettingsValue {
  const value = useContext(SettingsContext);
  if (!value) throw new Error("useSettings must be used inside <SettingsProvider>");
  return value;
}
