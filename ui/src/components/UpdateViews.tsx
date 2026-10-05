// The Sparkle-style update dialog (available → downloading → failed) and the
// "Update downloaded" banner.

import { formatMegabytes } from "../lib/format";
import { useUpdates } from "../state/updates";
import { AppIcon, ButtonRow, Modal, ProgressBar } from "./common";

/** Release notes as plain lines; a leading "- " or "* " is dropped. */
export function noteLines(notes: string): string[] {
  return notes
    .split(/\r?\n/)
    .map((l) => l.replace(/^\s*[-*]\s+/, "").trim())
    .filter((l) => l.length > 0);
}

export function UpdateDialog() {
  const updates = useUpdates();
  const d = updates.dialog;
  if (d.kind === "closed") return null;

  if (d.kind === "downloading") {
    const pct = d.total ? d.downloaded / d.total : 0;
    return (
      <Modal labelledBy="update-dl-title" onEscape={updates.hideDownload}>
        <div id="update-dl-title" style={{ fontWeight: 600 }}>
          Downloading Media Identifier {d.info.version}…
        </div>
        <ProgressBar value={pct} label="Update download" />
        <div className="row-between">
          <span className="muted small">
            {d.total ? `${formatMegabytes(d.downloaded)} of ${formatMegabytes(d.total)}` : formatMegabytes(d.downloaded)}
          </span>
          <button type="button" className="btn small" onClick={updates.hideDownload}>
            Hide
          </button>
        </div>
        <p className="muted small" style={{ margin: 0 }}>
          The download continues in the background. You're asked before the app relaunches.
        </p>
      </Modal>
    );
  }

  const lines = noteLines(d.info.notes);
  return (
    <Modal labelledBy="update-title" describedBy="update-versions" onEscape={updates.remindLater}>
      <div className="dialog-head">
        <AppIcon />
        <div>
          <div id="update-title" style={{ fontWeight: 650, fontSize: 16 }}>
            A new version of Media Identifier is available
          </div>
          <div id="update-versions" className="muted small">
            Version {d.info.version} is available. You have {d.info.currentVersion}.
          </div>
        </div>
      </div>
      {lines.length > 0 && (
        <div className="notes" tabIndex={0} aria-label="Release notes">
          <b>What's new in {d.info.version}</b>
          <ul>
            {lines.map((l, i) => (
              <li key={i}>{l}</li>
            ))}
          </ul>
        </div>
      )}
      {d.kind === "failed" && (
        <p role="alert" className="small" style={{ margin: 0, color: "var(--bad)" }}>
          The update couldn't be downloaded: {d.message}
        </p>
      )}
      <ButtonRow
        leading={
          d.kind === "available" ? (
            <button type="button" className="btn" onClick={() => void updates.skip()}>
              Skip this version
            </button>
          ) : undefined
        }
        others={[
          <button key="later" type="button" className="btn" onClick={updates.remindLater}>
            Remind me later
          </button>,
        ]}
        primary={
          <button type="button" className="btn primary" onClick={() => void updates.install()} data-autofocus>
            {d.kind === "failed" ? "Try again" : "Install update"}
          </button>
        }
      />
    </Modal>
  );
}

export function UpdateBanner({ jobRunning }: { jobRunning: boolean }) {
  const updates = useUpdates();
  if (!updates.readyVersion || !updates.bannerVisible) return null;
  return (
    <div className="banner" role="status">
      <span aria-hidden="true">⬆︎</span>
      <span className="grow">
        <b>Update downloaded.</b> Relaunch to finish installing Media Identifier {updates.readyVersion}.
        {jobRunning && <span className="muted"> Identification is running; the update waits until it finishes.</span>}
        {updates.relaunchError && <span style={{ color: "var(--bad)" }}> {updates.relaunchError}</span>}
      </span>
      <ButtonRow
        others={[
          <button key="later" type="button" className="btn small" onClick={updates.dismissBanner}>
            Later
          </button>,
        ]}
        primary={
          <button type="button" className="btn small primary" disabled={jobRunning} onClick={() => void updates.relaunch()}>
            Relaunch now
          </button>
        }
      />
    </div>
  );
}
