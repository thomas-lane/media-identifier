// App shell: navigation between Identify, History and Settings. The screens themselves
// (Start, Confirm show, Identifying, Review, Rename, History, Settings, Updates) are built by
// the UI module from the approved mockup; this shell only proves the backend wiring.

import { useEffect, useState } from "react";

import { toApiError, useBackend } from "./api";
import type { RecentJob } from "./types/generated";

type Section = "identify" | "history" | "settings";

const SECTIONS: { id: Section; label: string }[] = [
  { id: "identify", label: "Identify" },
  { id: "history", label: "History" },
  { id: "settings", label: "Settings" },
];

export function App() {
  const backend = useBackend();
  const [section, setSection] = useState<Section>("identify");
  const [recent, setRecent] = useState<RecentJob[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    backend
      .recentJobs()
      .then((jobs) => active && setRecent(jobs))
      .catch((e: unknown) => active && setError(toApiError(e).message));
    return () => {
      active = false;
    };
  }, [backend]);

  return (
    <div className="app">
      <nav className="nav" aria-label="Main">
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            type="button"
            className="nav-item"
            aria-current={section === s.id ? "page" : undefined}
            onClick={() => setSection(s.id)}
          >
            {s.label}
          </button>
        ))}
      </nav>
      <main className="content">
        {section === "identify" && (
          <section>
            <h1>Identify episodes</h1>
            {error && <p role="alert">{error}</p>}
            <h2 className="section-label">Recent</h2>
            <ul className="recent">
              {recent?.map((job) => (
                <li key={job.jobId}>
                  {job.showName} · {job.fileCount} files
                  {job.toReview > 0 ? ` · ${job.toReview} to review` : " · Done"}
                </li>
              ))}
            </ul>
          </section>
        )}
        {section === "history" && <h1>History</h1>}
        {section === "settings" && <h1>Settings</h1>}
      </main>
    </div>
  );
}
