// Identifying: stage progress, per-file status, time left, Cancel, and a way to start reviewing
// files that are already finished.

import type { ReactNode } from "react";

import { ButtonRow, Pill, ProgressBar } from "../components/common";
import type { Tone } from "../components/common";
import { formatTimeLeft, plural } from "../lib/format";
import { baseName } from "../lib/paths";
import { useIdentify } from "../state/identify";
import { STAGES, fileProgress } from "../state/job";
import type { FileProgress } from "../state/job";
import type { Accelerator, Stage, StageState } from "../types/generated";

const STAGE_LABELS: Record<Stage, string> = {
  episodeList: "Episode list",
  subtitles: "Subtitles",
  discOrder: "Disc order",
  listening: "Listening",
  matching: "Matching",
};

const ACCELERATORS: Record<Accelerator, string> = {
  appleGpu: "using the Apple GPU",
  vulkan: "using the graphics card",
  cpu: "using the processor",
};

export function IdentifyingScreen() {
  const { state, dispatch, cancel } = useIdentify();
  const { job, request } = state;
  const showName = request?.show.name ?? "";
  const { done: finished, total } = job ? fileProgress(job, state.scan?.playAll?.fileId ?? null) : { done: 0, total: 0 };

  return (
    <section className="screen" aria-labelledby="progress-title">
      <h1 id="progress-title">
        Identifying {showName}
        {total > 0 ? ` · ${plural(total, "file")}` : ""}
      </h1>
      <div className="card stack" aria-label="Progress">
        {STAGES.map((stage) => (
          <StageRow key={stage} stage={stage} state={job?.stages[stage] ?? { kind: "waiting" }} />
        ))}
        <div className="muted small" aria-live="polite">
          {job?.phase === "running" && (
            <>
              {job.etaSeconds !== null ? formatTimeLeft(job.etaSeconds) : "Working out how long this takes…"}
              {job.accelerator && ` · ${ACCELERATORS[job.accelerator]}`}
            </>
          )}
          {!job && state.starting && "Starting…"}
          {job?.phase === "cancelled" && "Identification was cancelled."}
          {job?.phase === "finished" && "Identification finished."}
        </div>
      </div>
      {job?.phase === "failed" && (
        <p role="alert" className="alert bad">
          Identification stopped: {job.failure}
        </p>
      )}
      {job && job.fileIds.length > 0 && (
        <table className="t">
          <thead>
            <tr>
              <th>File</th>
              <th>Status</th>
              <th>Best match so far</th>
            </tr>
          </thead>
          <tbody>
            {job.fileIds.map((id) => {
              const progress = job.files[id];
              return (
                <tr key={id}>
                  <td className="mono">{baseName(id)}</td>
                  <td>{progress && <FileStatusPill progress={progress} />}</td>
                  <td className={progress?.bestSoFar ? "" : "muted"}>
                    {progress?.verdict === "playAll" ? "Play all (disc order)" : progress?.bestSoFar ?? "—"}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      <div className="row-between">
        <span className="muted small">
          {job?.phase === "running" && finished > 0 ? "You can start reviewing finished files now." : ""}
        </span>
        {job?.phase === "running" || state.starting ? (
          <ButtonRow
            others={[
              <button key="cancel" type="button" className="btn" onClick={() => void cancel()} disabled={!job}>
                Cancel
              </button>,
            ]}
            primary={
              <button
                type="button"
                className="btn primary"
                disabled={finished === 0}
                onClick={() => dispatch({ type: "go", step: "review" })}
              >
                Review finished files
              </button>
            }
          />
        ) : (
          <ButtonRow
            others={[
              <button key="over" type="button" className="btn" onClick={() => dispatch({ type: "reset" })}>
                Start over
              </button>,
            ]}
            primary={
              <button
                type="button"
                className="btn primary"
                disabled={finished === 0}
                onClick={() => dispatch({ type: "go", step: "review" })}
              >
                Review finished files
              </button>
            }
          />
        )}
      </div>
    </section>
  );
}

function StageRow({ stage, state }: { stage: Stage; state: StageState }) {
  const label = STAGE_LABELS[stage];
  let value = 0;
  let tone: "ok" | "bad" | undefined;
  let status: ReactNode;
  switch (state.kind) {
    case "waiting":
      status = <Pill tone="gray">Waiting</Pill>;
      break;
    case "running":
      value = state.total > 0 ? state.done / state.total : 0;
      status = (
        <span className="muted small">
          {state.done}/{state.total}
        </span>
      );
      break;
    case "done":
      value = 1;
      tone = "ok";
      status = <Pill tone="ok">Done</Pill>;
      break;
    case "skipped":
      status = <Pill tone="gray">Skipped</Pill>;
      break;
    case "failed":
      value = 1;
      tone = "bad";
      status = <Pill tone="bad">Failed</Pill>;
      break;
  }
  return (
    <div className="meter" title={state.kind === "failed" ? state.message : undefined}>
      <span>{label}</span>
      <ProgressBar value={value} label={label} tone={tone} />
      {status}
    </div>
  );
}

const VERDICT_PILLS: Record<string, [Tone, string]> = {
  confident: ["ok", "Matched"],
  check: ["warn", "Check"],
  extra: ["gray", "Extra"],
  playAll: ["info", "Play all"],
};

function FileStatusPill({ progress }: { progress: FileProgress }) {
  switch (progress.status) {
    case "waiting":
      return <Pill tone="gray">Waiting</Pill>;
    case "listening":
      return (
        <Pill tone="info" spinner>
          Listening
        </Pill>
      );
    case "matching":
      return (
        <Pill tone="info" spinner>
          Matching
        </Pill>
      );
    case "failed":
      return <Pill tone="bad">Failed</Pill>;
    case "done": {
      const [tone, label] = VERDICT_PILLS[progress.verdict ?? "confident"] ?? ["ok", "Done"];
      return <Pill tone={tone}>{label}</Pill>;
    }
  }
}
