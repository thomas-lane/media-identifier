// Rename (save the results): rename in place (the default, undoable from History), copy into a
// new folder, or export a CSV list. The play-all, extras and skipped files are never renamed.

import { useEffect, useId, useMemo, useState } from "react";

import { toApiError, useBackend } from "../api";
import { ButtonRow } from "../components/common";
import { plural } from "../lib/format";
import { baseName, joinPath } from "../lib/paths";
import { conflictText, previewTree, untouchedSummary } from "../lib/plan";
import { toDecisions } from "../lib/review";
import { useIdentify } from "../state/identify";
import { useNav } from "../state/nav";
import { useSettings } from "../state/settings";
import type { NamingScheme, RenameOutcome, RenamePlan, SaveMode, SaveModeKind } from "../types/generated";

type NamingKind = NamingScheme["kind"];

const MODES: { kind: SaveModeKind; title: string; detail: string }[] = [
  { kind: "renameInPlace", title: "Rename in place", detail: "Can be undone from History." },
  { kind: "copyToFolder", title: "Copy into a new folder", detail: "Originals untouched." },
  { kind: "exportList", title: "Only export a list", detail: "CSV of file → episode." },
];

const DEFAULT_TEMPLATE = "{show} ({year})/Season {season:02}/{show} - S{season:02}E{episode:02} - {title}.{ext}";

export function RenameScreen() {
  const backend = useBackend();
  const { state, dispatch } = useIdentify();
  const { settings, update } = useSettings();
  const nav = useNav();
  const ids = useId();
  const { job, reviews, request } = state;
  const folder = request?.folder ?? "";

  const [mode, setMode] = useState<SaveModeKind>(settings?.saveMode ?? "renameInPlace");
  const [root, setRoot] = useState(folder);
  const [copyTo, setCopyTo] = useState("");
  const [csvPath, setCsvPath] = useState(() => joinPath(folder, `${request?.show.name ?? "Episodes"} episodes.csv`));
  const [namingKind, setNamingKind] = useState<NamingKind>(settings?.naming.kind ?? "jellyfinPlex");
  const [template, setTemplate] = useState(settings?.naming.kind === "custom" ? settings.naming.template : DEFAULT_TEMPLATE);
  const [subtitles, setSubtitles] = useState(settings?.saveHeardSubtitles ?? false);
  const [currentPlan, setPlan] = useState<RenamePlan | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const [outcome, setOutcome] = useState<RenameOutcome | null>(null);
  const [applyError, setApplyError] = useState<string | null>(null);

  const order = useMemo(() => job?.fileIds ?? [], [job]);
  const decisions = useMemo(() => toDecisions(order, reviews), [order, reviews]);
  const naming: NamingScheme = useMemo(
    () => (namingKind === "custom" ? { kind: "custom", template } : { kind: namingKind }),
    [namingKind, template],
  );
  const saveMode: SaveMode | null = useMemo(() => {
    if (mode === "renameInPlace") return root ? { kind: "renameInPlace", root } : null;
    if (mode === "copyToFolder") return copyTo ? { kind: "copyToFolder", destination: copyTo } : null;
    return csvPath ? { kind: "exportList", destination: csvPath } : null;
  }, [mode, root, copyTo, csvPath]);

  useEffect(() => {
    if (!job || !saveMode) return;
    let active = true;
    backend
      .planRename({ jobId: job.jobId, decisions, mode: saveMode, naming, saveHeardSubtitles: mode !== "exportList" && subtitles })
      .then((p) => {
        if (!active) return;
        setPlan(p);
        setPlanError(null);
      })
      .catch((e: unknown) => {
        if (!active) return;
        setPlan(null);
        setPlanError(toApiError(e).message);
      });
    return () => {
      active = false;
    };
  }, [backend, job, decisions, saveMode, naming, subtitles, mode]);

  if (!job || !request) return null;

  const pickFolder = async (set: (v: string) => void) => {
    const picked = await backend.chooseFolder().catch(() => null);
    if (picked) set(picked);
  };
  const pickCsv = async () => {
    const picked = await backend.chooseSaveFile(csvPath).catch(() => null);
    if (picked) setCsvPath(picked);
  };

  // A plan for an earlier destination is not shown once the destination is cleared.
  const plan = saveMode ? currentPlan : null;
  const count = plan?.items.length ?? 0;
  const blocked = !plan || plan.conflicts.length > 0 || count === 0 || applying;
  const verb = mode === "renameInPlace" ? "Rename" : mode === "copyToFolder" ? "Copy" : "Export";
  const primaryLabel = mode === "exportList" ? `Export list of ${plural(count, "file")}` : `${verb} ${plural(count, "file")}`;

  const apply = async () => {
    if (!plan) return;
    setApplying(true);
    setApplyError(null);
    try {
      const result = await backend.applyRename(plan);
      setOutcome(result);
      void update({ saveMode: mode, naming, saveHeardSubtitles: subtitles });
    } catch (e) {
      setApplyError(toApiError(e).message);
    } finally {
      setApplying(false);
    }
  };

  if (outcome) {
    const done =
      mode === "renameInPlace" ? "Renamed" : mode === "copyToFolder" ? "Copied" : "Exported a list of";
    return (
      <section className="screen" aria-labelledby="saved-title">
        <h1 id="saved-title">
          {done} {plural(outcome.completed, "file")}
        </h1>
        {mode === "exportList" ? (
          <p className="muted" style={{ margin: 0 }}>
            Saved to <span className="mono">{csvPath}</span>.
          </p>
        ) : (
          <p className="muted" style={{ margin: 0 }}>
            You can undo this from History.
          </p>
        )}
        {outcome.failed.length > 0 && (
          <div role="alert" className="alert bad">
            <b>{plural(outcome.failed.length, "file")} couldn't be {mode === "copyToFolder" ? "copied" : "renamed"}:</b>
            <ul style={{ margin: "6px 0 0", paddingLeft: 18 }}>
              {outcome.failed.map((f) => (
                <li key={f.fileId}>
                  <span className="mono">{baseName(f.path)}</span>: {f.message}
                </li>
              ))}
            </ul>
          </div>
        )}
        <ButtonRow
          others={
            outcome.historyId
              ? [
                  <button key="history" type="button" className="btn" onClick={() => nav.go("history")}>
                    Open History
                  </button>,
                ]
              : []
          }
          primary={
            <button type="button" className="btn primary" onClick={() => dispatch({ type: "reset" })}>
              Identify another folder
            </button>
          }
        />
      </section>
    );
  }

  const lines = plan && saveMode && mode !== "exportList" ? previewTree(mode === "renameInPlace" ? root : copyTo, plan.items) : [];
  const untouched = plan ? untouchedSummary(plan.untouched, mode === "copyToFolder" ? "copied" : mode === "exportList" ? "listed" : "renamed") : null;

  return (
    <section className="screen" aria-labelledby="rename-title">
      <h1 id="rename-title">Save the results</h1>
      <div className="modes" role="radiogroup" aria-label="How to save">
        {MODES.map((m) => (
          <label key={m.kind} className={`card mode${mode === m.kind ? " selected" : ""}`}>
            <span>
              <input type="radio" name={`${ids}-mode`} checked={mode === m.kind} onChange={() => setMode(m.kind)} />
              <b>{m.title}</b>
            </span>
            <span className="muted small">{m.detail}</span>
          </label>
        ))}
      </div>
      <div className="two-col">
        {mode === "renameInPlace" && (
          <PathField label="Rename within" value={root} onChange={setRoot} onBrowse={() => void pickFolder(setRoot)} />
        )}
        {mode === "copyToFolder" && (
          <PathField
            label="Copy into"
            value={copyTo}
            placeholder="Choose a folder"
            onChange={setCopyTo}
            onBrowse={() => void pickFolder(setCopyTo)}
          />
        )}
        {mode === "exportList" && (
          <PathField label="Save list as" value={csvPath} onChange={setCsvPath} onBrowse={() => void pickCsv()} />
        )}
        <label className="label">
          Naming
          <select className="field" value={namingKind} onChange={(e) => setNamingKind(e.target.value as NamingKind)}>
            <option value="jellyfinPlex">Jellyfin / Plex</option>
            <option value="kodi">Kodi</option>
            <option value="custom">Custom…</option>
          </select>
        </label>
      </div>
      {namingKind === "custom" && (
        <label className="label">
          Custom name
          <input className="field mono" value={template} onChange={(e) => setTemplate(e.target.value)} spellCheck={false} />
          <span className="muted small">
            Placeholders: {"{show}"} {"{year}"} {"{season}"} {"{season:02}"} {"{episode}"} {"{episode:02}"} {"{title}"}{" "}
            {"{ext}"}. A / starts a new folder.
          </span>
        </label>
      )}
      <div className="card" aria-labelledby={`${ids}-preview`}>
        <div id={`${ids}-preview`} className="eyebrow">
          Preview
        </div>
        {!saveMode && <p className="muted small" style={{ margin: 0 }}>Choose where to save.</p>}
        {planError && (
          <p role="alert" className="small" style={{ color: "var(--bad)", margin: 0 }}>
            {planError}
          </p>
        )}
        {plan && count === 0 && <p className="muted small" style={{ margin: 0 }}>No approved episodes to save yet.</p>}
        {mode === "exportList" && plan && count > 0 && (
          <ul className="preview">
            {plan.items.map((item) => (
              <li key={item.fileId}>
                {baseName(item.from)} <span className="muted">→</span> {baseName(item.to)}
              </li>
            ))}
          </ul>
        )}
        {lines.length > 0 && (
          <ul className="preview" aria-label="New names">
            {lines.map((line, i) => (
              <li key={i} style={{ paddingLeft: `${line.depth * 2}ch` }}>
                {line.text}
                {line.from && <span className="muted"> ← {line.from}</span>}
              </li>
            ))}
          </ul>
        )}
        {untouched && (
          <p className="muted small" style={{ margin: "8px 0 0" }}>
            {untouched}
          </p>
        )}
      </div>
      {plan && plan.conflicts.length > 0 && (
        <div role="alert" className="alert bad">
          <b>Fix these before saving:</b>
          <ul style={{ margin: "6px 0 0", paddingLeft: 18 }}>
            {plan.conflicts.map((c, i) => (
              <li key={i}>{conflictText(c)}</li>
            ))}
          </ul>
        </div>
      )}
      {mode !== "exportList" && (
        <label className="check">
          <input type="checkbox" checked={subtitles} onChange={(e) => setSubtitles(e.target.checked)} />
          Also save subtitles (.srt) from what was heard
        </label>
      )}
      {applyError && (
        <p role="alert" className="alert bad">
          {applyError}
        </p>
      )}
      <ButtonRow
        others={[
          <button key="back" type="button" className="btn" onClick={() => dispatch({ type: "go", step: "review" })}>
            Back
          </button>,
        ]}
        primary={
          <button type="button" className="btn primary" disabled={blocked} onClick={() => void apply()}>
            {applying ? "Saving…" : primaryLabel}
          </button>
        }
      />
    </section>
  );
}

function PathField({
  label,
  value,
  placeholder,
  onChange,
  onBrowse,
}: {
  label: string;
  value: string;
  placeholder?: string;
  onChange: (v: string) => void;
  onBrowse: () => void;
}) {
  const id = useId();
  return (
    <div className="label">
      <label htmlFor={id}>{label}</label>
      <div className="row" style={{ gap: 6 }}>
        <input id={id} className="field mono" value={value} placeholder={placeholder} onChange={(e) => onChange(e.target.value)} spellCheck={false} />
        <button type="button" className="btn" onClick={onBrowse} aria-label={`Choose: ${label}`}>
          …
        </button>
      </div>
    </div>
  );
}
