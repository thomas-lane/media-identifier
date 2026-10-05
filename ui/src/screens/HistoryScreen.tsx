// History: every rename or copy, newest first, each with Undo.

import { useCallback, useEffect, useState } from "react";

import { toApiError, useBackend } from "../api";
import { ButtonRow, Pill } from "../components/common";
import { formatWhen, plural } from "../lib/format";
import { baseName } from "../lib/paths";
import type { HistoryEntry, SaveModeKind, UndoOutcome } from "../types/generated";

const MODE_LABELS: Record<SaveModeKind, string> = {
  renameInPlace: "Renamed",
  copyToFolder: "Copied",
  exportList: "Exported list",
};

export function HistoryScreen() {
  const backend = useBackend();
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setEntries(await backend.listHistory());
      setError(null);
    } catch (e) {
      setError(toApiError(e).message);
    }
  }, [backend]);

  useEffect(() => {
    let active = true;
    backend
      .listHistory()
      .then((list) => active && setEntries(list))
      .catch((e: unknown) => active && setError(toApiError(e).message));
    return () => {
      active = false;
    };
  }, [backend]);

  return (
    <section className="screen" aria-labelledby="history-title">
      <h1 id="history-title">History</h1>
      {error && (
        <p role="alert" className="alert bad">
          {error}
        </p>
      )}
      {entries && entries.length === 0 && <p className="muted">Renamed and copied files are listed here, so you can undo them.</p>}
      {entries && entries.length > 0 && (
        <ul className="history-list">
          {entries.map((entry) => (
            <HistoryCard key={entry.id} entry={entry} onChanged={load} />
          ))}
        </ul>
      )}
    </section>
  );
}

function HistoryCard({ entry, onChanged }: { entry: HistoryEntry; onChanged: () => Promise<void> }) {
  const backend = useBackend();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<UndoOutcome | null>(null);
  const [error, setError] = useState<string | null>(null);
  const undoable = entry.mode !== "exportList" && entry.undoneAtMs === null;
  const count = entry.items.length;
  const question =
    entry.mode === "copyToFolder"
      ? `Delete the ${plural(count, "copy", "copies")}? The original files are not touched.`
      : `Give ${count === 1 ? "this file its" : `these ${count} files their`} original ${count === 1 ? "name" : "names"} back?`;

  const undo = async () => {
    setBusy(true);
    setError(null);
    try {
      setResult(await backend.undoHistory(entry.id));
      setConfirming(false);
      await onChanged();
    } catch (e) {
      setError(toApiError(e).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <li className="card stack" style={{ gap: 6 }}>
      <div className="row-between">
        <div className="grow">
          <b>{entry.showName}</b> <span className="muted">· {MODE_LABELS[entry.mode]} {plural(count, "file")}</span>
          <div className="muted small">
            {formatWhen(entry.createdAtMs)} · <span className="mono">{entry.folder}</span>
          </div>
        </div>
        {entry.undoneAtMs !== null && <Pill tone="gray">Undone {formatWhen(entry.undoneAtMs)}</Pill>}
        {undoable && !confirming && (
          <button type="button" className="btn small" onClick={() => setConfirming(true)}>
            Undo…
          </button>
        )}
      </div>
      {confirming && (
        <div className="alert warn row-between">
          <span>{question}</span>
          <ButtonRow
            others={[
              <button key="cancel" type="button" className="btn small" onClick={() => setConfirming(false)} disabled={busy}>
                Cancel
              </button>,
            ]}
            primary={
              <button type="button" className="btn small primary" onClick={() => void undo()} disabled={busy} data-autofocus>
                {entry.mode === "copyToFolder" ? "Delete copies" : "Undo rename"}
              </button>
            }
          />
        </div>
      )}
      {result && (
        <p role="status" className="small" style={{ margin: 0 }}>
          {entry.mode === "copyToFolder" ? "Removed" : "Restored"} {plural(result.restored, "file")}.
          {result.failed.length > 0 && ` ${plural(result.failed.length, "file")} couldn't be changed back:`}
        </p>
      )}
      {result && result.failed.length > 0 && (
        <ul className="small" style={{ margin: 0, paddingLeft: 18, color: "var(--bad)" }}>
          {result.failed.map((f) => (
            <li key={f.path}>
              <span className="mono">{baseName(f.path)}</span>: {f.message}
            </li>
          ))}
        </ul>
      )}
      {error && (
        <p role="alert" className="small" style={{ margin: 0, color: "var(--bad)" }}>
          {error}
        </p>
      )}
      <details>
        <summary className="small" style={{ cursor: "default", color: "var(--accent)" }}>
          Show files
        </summary>
        <ul className="history-items">
          {entry.items.map((item) => (
            <li key={item.from}>
              {baseName(item.from)} <span className="muted">→</span> {baseName(item.to)}
            </li>
          ))}
        </ul>
      </details>
    </li>
  );
}
