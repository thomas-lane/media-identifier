// Start: choose or drop a folder, the one-time speech model download, and recent jobs.

import { useEffect, useRef, useState } from "react";

import { toApiError, useBackend } from "../api";
import { FolderIcon, Pill, ProgressBar } from "../components/common";
import { formatDay, formatMegabytes, formatTimeLeft, plural } from "../lib/format";
import { folderFromDrop } from "../lib/paths";
import { useIdentify } from "../state/identify";
import { useModel } from "../state/model";
import { useSettings } from "../state/settings";
import type { RecentJob, SpeechModel } from "../types/generated";

export function StartScreen() {
  const backend = useBackend();
  const { state, openFolder, openRecent, dispatch } = useIdentify();
  const [recent, setRecent] = useState<RecentJob[] | null>(null);
  const [recentError, setRecentError] = useState<string | null>(null);
  const [hover, setHover] = useState(false);

  useEffect(() => {
    let active = true;
    backend
      .recentJobs()
      .then((jobs) => active && setRecent(jobs))
      .catch((e: unknown) => active && setRecentError(toApiError(e).message));
    return () => {
      active = false;
    };
  }, [backend]);

  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | null = null;
    void backend
      .onFileDrop((event) => {
        if (event.kind === "hover") setHover(true);
        else if (event.kind === "cancel") setHover(false);
        else {
          setHover(false);
          const folder = folderFromDrop(event.paths);
          if (folder) void openFolder(folder);
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
  }, [backend, openFolder]);

  const choose = async () => {
    dispatch({ type: "clearError" });
    try {
      const folder = await backend.chooseFolder();
      if (folder) await openFolder(folder);
    } catch (e) {
      dispatch({ type: "failed", message: toApiError(e).message });
    }
  };

  return (
    <section className="screen" aria-labelledby="start-title">
      <h1 id="start-title">Identify episodes</h1>
      <div className={`drop${hover ? " hover" : ""}`}>
        <FolderIcon />
        <div className="drop-title">Drop a folder of video files here</div>
        <div className="muted">or</div>
        <button type="button" className="btn primary" onClick={() => void choose()} disabled={state.scanning}>
          {state.scanning ? "Reading folder…" : "Choose folder…"}
        </button>
        <div className="muted small">MKV, MP4, M4V, MOV, AVI, TS, M2TS, MPG and VOB files</div>
      </div>
      {state.error && (
        <p role="alert" className="alert bad">
          {state.error}
        </p>
      )}
      <ModelCard />
      <div>
        <h2 className="eyebrow">Recent</h2>
        {recentError && <p className="muted small">{recentError}</p>}
        {recent && recent.length === 0 && <p className="muted small">Folders you identify appear here.</p>}
        {recent && recent.length > 0 && (
          <table className="t">
            <thead className="visually-hidden">
              <tr>
                <th>Folder</th>
                <th>Files</th>
                <th>State</th>
                <th>Finished</th>
              </tr>
            </thead>
            <tbody>
              {recent.map((job) => (
                <tr key={job.jobId} className="clickable" onClick={() => void openRecent(job.jobId)}>
                  <td>
                    <button type="button" className="btn link" onClick={(e) => { e.stopPropagation(); void openRecent(job.jobId); }}>
                      {job.showName}
                    </button>
                  </td>
                  <td className="muted">{plural(job.fileCount, "file")}</td>
                  <td>
                    {job.toReview > 0 ? (
                      <Pill tone="warn">{job.toReview} to review</Pill>
                    ) : (
                      <Pill tone="ok">{job.saved ? "Done" : "Reviewed"}</Pill>
                    )}
                  </td>
                  <td className="muted">{formatDay(job.finishedAtMs)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </section>
  );
}

const MODEL_NAMES: Record<SpeechModel, string> = { accurate: "Accurate", fast: "Fast" };

/**
 * The one-time speech model download. It starts by itself the first time the Start screen opens
 * without the model (the app cannot listen without it); after a pause it waits for Resume.
 */
function ModelCard() {
  const { settings } = useSettings();
  const model = settings?.speechModel ?? null;
  const { status, download, pause, error } = useModel(model);
  const autoStarted = useRef(false);

  const kind = status?.state.kind;
  useEffect(() => {
    if (kind === "missing" && !autoStarted.current) {
      autoStarted.current = true;
      void download();
    }
  }, [kind, download]);

  if (!status || !model || kind === "ready") return null;
  const total = status.info.sizeBytes;
  const s = status.state;
  const done = s.kind === "downloading" || s.kind === "paused" ? s.downloaded : 0;
  return (
    <div className="card row" style={{ gap: 14, alignItems: "flex-start" }} aria-labelledby="model-title">
      <div className="grow">
        <div id="model-title" className="card-title">
          One-time download: speech model ({MODEL_NAMES[model]}, about {formatMegabytes(total)})
        </div>
        <div className="muted small">
          Needed once to listen to your files on this computer. You can choose a smaller, faster model in Settings.
        </div>
        {(s.kind === "downloading" || s.kind === "paused" || s.kind === "verifying") && (
          <div style={{ marginTop: 8 }}>
            <ProgressBar value={s.kind === "verifying" ? 1 : done / total} label="Speech model download" />
            <div className="muted small" style={{ marginTop: 4 }}>
              {s.kind === "verifying" && "Checking the download…"}
              {s.kind === "paused" && `Paused at ${formatMegabytes(done)} of ${formatMegabytes(total)}`}
              {s.kind === "downloading" &&
                `${formatMegabytes(done)} of ${formatMegabytes(total)}${
                  s.bytesPerSecond > 0 ? ` · ${formatTimeLeft((total - done) / s.bytesPerSecond).toLowerCase()}` : ""
                }`}
            </div>
          </div>
        )}
        {(s.kind === "failed" || error) && (
          <p role="alert" className="small" style={{ color: "var(--bad)", margin: "8px 0 0" }}>
            The download stopped: {s.kind === "failed" ? s.message : error}
          </p>
        )}
      </div>
      {s.kind === "downloading" && (
        <button type="button" className="btn small" onClick={() => void pause()}>
          Pause
        </button>
      )}
      {s.kind === "paused" && (
        <button type="button" className="btn small" onClick={() => void download()}>
          Resume
        </button>
      )}
      {(s.kind === "missing" || s.kind === "failed") && (
        <button type="button" className="btn small" onClick={() => void download()}>
          {s.kind === "failed" ? "Try again" : "Download"}
        </button>
      )}
    </div>
  );
}
