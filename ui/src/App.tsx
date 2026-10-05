// App shell: the sidebar (Identify, History, Settings, About), the update banner and dialog,
// and the Identify flow's current step.

import { useEffect, useMemo, useState } from "react";

import { UpdateBanner, UpdateDialog } from "./components/UpdateViews";
import { usePlatform } from "./components/common";
import { AboutScreen } from "./screens/AboutScreen";
import { ConfirmShowScreen } from "./screens/ConfirmShowScreen";
import { HistoryScreen } from "./screens/HistoryScreen";
import { IdentifyingScreen } from "./screens/IdentifyingScreen";
import { RenameScreen } from "./screens/RenameScreen";
import { ReviewScreen } from "./screens/ReviewScreen";
import { SettingsScreen } from "./screens/SettingsScreen";
import { StartScreen } from "./screens/StartScreen";
import { IdentifyProvider, useIdentify } from "./state/identify";
import { fileProgress } from "./state/job";
import { NavContext } from "./state/nav";
import type { Section } from "./state/nav";
import { SettingsProvider } from "./state/settings";
import { UpdatesProvider } from "./state/updates";

export function App() {
  return (
    <SettingsProvider>
      <IdentifyProvider>
        <Shell />
      </IdentifyProvider>
    </SettingsProvider>
  );
}

const SECTIONS: { id: Section; label: string }[] = [
  { id: "identify", label: "Identify" },
  { id: "history", label: "History" },
];

const BOTTOM_SECTIONS: { id: Section; label: string }[] = [
  { id: "settings", label: "Settings" },
  { id: "about", label: "About" },
];

function Shell() {
  const identify = useIdentify();
  const platform = usePlatform();
  const [section, setSection] = useState<Section>("identify");
  const nav = useMemo(() => ({ section, go: setSection }), [section]);
  const { job } = identify.state;

  useEffect(() => {
    document.documentElement.dataset.platform = platform;
  }, [platform]);

  // Settings opens with ⌘, on macOS and Ctrl+, on Windows, as in other desktop apps.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = platform === "mac" ? e.metaKey : e.ctrlKey;
      if (mod && e.key === ",") {
        e.preventDefault();
        setSection("settings");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [platform]);

  const counts = job?.phase === "running" ? fileProgress(job, identify.state.scan?.playAll?.fileId ?? null) : null;
  const progress = counts ? `${counts.done}/${counts.total || "…"}` : null;

  const item = (s: { id: Section; label: string }) => (
    <button
      key={s.id}
      type="button"
      className="nav-item"
      aria-current={section === s.id ? "page" : undefined}
      onClick={() => setSection(s.id)}
    >
      {s.label}
      {s.id === "identify" && progress && (
        <span className="nav-badge" aria-label={`Identifying, ${progress} files done`}>
          {progress}
        </span>
      )}
    </button>
  );

  return (
    <NavContext.Provider value={nav}>
      <UpdatesProvider jobRunning={identify.jobRunning}>
        <div className="shell">
          <nav className="shell-nav" aria-label="Main">
            {SECTIONS.map(item)}
            <div className="nav-spacer" />
            {BOTTOM_SECTIONS.map(item)}
          </nav>
          <main className="shell-main">
            <UpdateBanner jobRunning={identify.jobRunning} />
            {section === "identify" && <IdentifyStep />}
            {section === "history" && <HistoryScreen />}
            {section === "settings" && <SettingsScreen />}
            {section === "about" && <AboutScreen />}
          </main>
        </div>
        <UpdateDialog />
      </UpdatesProvider>
    </NavContext.Provider>
  );
}

function IdentifyStep() {
  const { state } = useIdentify();
  switch (state.step) {
    case "start":
      return <StartScreen />;
    case "confirm":
      return <ConfirmShowScreen />;
    case "identifying":
      return <IdentifyingScreen />;
    case "review":
      return <ReviewScreen />;
    case "rename":
      return <RenameScreen />;
  }
}
