// Settings: speech model, online sources and their keys, and updates.

import { useCallback, useEffect, useId, useState } from "react";

import { toApiError, useBackend } from "../api";
import { ButtonRow, Pill, ProgressBar, Segmented } from "../components/common";
import { formatMegabytes, formatWhen } from "../lib/format";
import { useModel } from "../state/model";
import { useSettings } from "../state/settings";
import { useUpdates } from "../state/updates";
import type { ApiKeyProvider, ProviderId, SourceStatus, SpeechModel, UpdateCheck } from "../types/generated";

const MODEL_SIZES: Record<SpeechModel, number> = { fast: 190_098_681, accurate: 574_041_195 };

export function SettingsScreen() {
  const { settings, update, error } = useSettings();
  if (!settings) return <section className="screen">{error && <p role="alert">{error}</p>}</section>;
  return (
    <section className="screen" aria-labelledby="settings-title">
      <h1 id="settings-title">Settings</h1>
      {error && (
        <p role="alert" className="alert bad">
          Couldn't save settings: {error}
        </p>
      )}
      <SpeechCard model={settings.speechModel} sample={settings.sampleLongFiles} onChange={update} />
      <SourcesCard />
      <UpdatesCard
        automatic={settings.checkUpdatesAutomatically}
        lastCheckMs={settings.lastUpdateCheckMs}
        onAutomatic={(v) => void update({ checkUpdatesAutomatically: v })}
      />
    </section>
  );
}

function SpeechCard({
  model,
  sample,
  onChange,
}: {
  model: SpeechModel;
  sample: boolean;
  onChange: (patch: { speechModel?: SpeechModel; sampleLongFiles?: boolean }) => Promise<void>;
}) {
  const { status, download, pause } = useModel(model);
  const s = status?.state;
  return (
    <div className="card stack" aria-labelledby="speech-title">
      <h2 id="speech-title">Speech model</h2>
      <Segmented
        label="Speech model"
        value={model}
        onChange={(m) => void onChange({ speechModel: m })}
        options={[
          { value: "fast", label: `Fast · ${formatMegabytes(MODEL_SIZES.fast)}` },
          { value: "accurate", label: `Accurate · ${formatMegabytes(MODEL_SIZES.accurate)}` },
        ]}
      />
      <div className="muted small">
        Accurate is the default. Fast suits older computers and understands English only. Long episodes are sampled in a
        few short windows, so even Accurate stays quick.
      </div>
      {s && (
        <div className="row small" aria-live="polite">
          {s.kind === "ready" && <Pill tone="ok">Downloaded</Pill>}
          {s.kind === "missing" && (
            <>
              <span className="muted grow">Not downloaded yet.</span>
              <button type="button" className="btn small" onClick={() => void download()}>
                Download
              </button>
            </>
          )}
          {(s.kind === "downloading" || s.kind === "paused") && (
            <>
              <span className="grow">
                <ProgressBar value={s.downloaded / s.total} label="Speech model download" />
              </span>
              <span className="muted">
                {formatMegabytes(s.downloaded)} of {formatMegabytes(s.total)}
              </span>
              <button type="button" className="btn small" onClick={() => void (s.kind === "paused" ? download() : pause())}>
                {s.kind === "paused" ? "Resume" : "Pause"}
              </button>
            </>
          )}
          {s.kind === "verifying" && <span className="muted">Checking the download…</span>}
          {s.kind === "failed" && (
            <>
              <span className="grow" style={{ color: "var(--bad)" }}>
                The download stopped: {s.message}
              </span>
              <button type="button" className="btn small" onClick={() => void download()}>
                Try again
              </button>
            </>
          )}
        </div>
      )}
      <label className="check">
        <input type="checkbox" checked={sample} onChange={(e) => void onChange({ sampleLongFiles: e.target.checked })} />
        Listen to a sample of each file (faster), not the whole file
      </label>
    </div>
  );
}

interface SourceInfo {
  name: string;
  detail: string | null;
  keyProvider: ApiKeyProvider | null;
  keyUrl: string | null;
}

const SOURCE_INFO: Partial<Record<ProviderId, SourceInfo>> = {
  tvmaze: { name: "Episode lists (TVmaze)", detail: null, keyProvider: null, keyUrl: null },
  lrclib: {
    name: "Song lyrics (LRCLIB)",
    detail: "Used for musical shorts like Schoolhouse Rock.",
    keyProvider: null,
    keyUrl: null,
  },
  subdl: {
    name: "Subtitles (SubDL)",
    detail: "Needs your own free SubDL API key. Whole seasons come in one download.",
    keyProvider: "subdl",
    keyUrl: "https://subdl.com/api-doc",
  },
  tmdb: {
    name: "Episode numbering (TMDb, optional)",
    detail: "Add your own TMDb key to name episodes the way Jellyfin numbers them.",
    keyProvider: "tmdb",
    keyUrl: "https://www.themoviedb.org/settings/api",
  },
};

const SOURCE_ORDER: ProviderId[] = ["tvmaze", "lrclib", "subdl", "tmdb"];

function SourcesCard() {
  const backend = useBackend();
  const [sources, setSources] = useState<SourceStatus[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setSources(await backend.sourceStatus());
    } catch (e) {
      setError(toApiError(e).message);
    }
  }, [backend]);

  useEffect(() => {
    let active = true;
    backend
      .sourceStatus()
      .then((list) => active && setSources(list))
      .catch((e: unknown) => active && setError(toApiError(e).message));
    return () => {
      active = false;
    };
  }, [backend]);

  const ordered = SOURCE_ORDER.flatMap((p) => sources?.filter((s) => s.provider === p) ?? []);
  return (
    <div className="card stack" aria-labelledby="sources-title">
      <h2 id="sources-title">Online sources</h2>
      {error && <p className="small" style={{ color: "var(--bad)", margin: 0 }}>{error}</p>}
      {ordered.map((s) => (
        <SourceRow key={s.provider} source={s} onChanged={load} />
      ))}
      <div className="muted small">
        Only show and episode details (names, numbers, titles and ids) and your keys are sent online. Your video and
        audio stay on this computer.
      </div>
    </div>
  );
}

function SourceRow({ source, onChanged }: { source: SourceStatus; onChanged: () => Promise<void> }) {
  const backend = useBackend();
  const info = SOURCE_INFO[source.provider];
  const [editing, setEditing] = useState(false);
  const [key, setKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  const id = useId();
  if (!info) return null;
  const provider = info.keyProvider;

  const save = async (value: string | null) => {
    if (!provider) return;
    try {
      await backend.setApiKey(provider, value);
      setEditing(false);
      setKey("");
      setError(null);
      await onChanged();
    } catch (e) {
      setError(toApiError(e).message);
    }
  };

  let status;
  switch (source.state.kind) {
    case "ready":
      status = <Pill tone="ok">{provider ? "Ready" : "Ready, no account needed"}</Pill>;
      break;
    case "needsKey":
      status = <Pill tone="gray">Not set up</Pill>;
      break;
    case "keyRejected":
      status = <Pill tone="bad">Key not accepted</Pill>;
      break;
    case "unavailable":
      status = <Pill tone="warn">Unavailable</Pill>;
      break;
  }

  const providerName = provider === "subdl" ? "SubDL" : "TMDb";
  return (
    <div>
      <div className="source-row">
        <span>
          {info.name}
          {info.detail && (
            <>
              <br />
              <span className="muted small">{info.detail}</span>
            </>
          )}
          {source.state.kind === "unavailable" && (
            <>
              <br />
              <span className="small warn-text">{source.state.message}</span>
            </>
          )}
        </span>
        <span className="row" style={{ flex: "none" }}>
          {status}
          {provider && !editing && (
            <button type="button" className="btn small" onClick={() => setEditing(true)}>
              {source.hasKey ? "Change key…" : "Add key…"}
            </button>
          )}
          {provider && !editing && source.hasKey && (
            <button type="button" className="btn small" onClick={() => void save(null)}>
              Remove
            </button>
          )}
        </span>
      </div>
      {editing && provider && (
        <form
          className="key-form"
          onSubmit={(e) => {
            e.preventDefault();
            if (key.trim()) void save(key.trim());
          }}
        >
          <label htmlFor={id} className="visually-hidden">
            {providerName} API key
          </label>
          <input
            id={id}
            className="field mono"
            type="password"
            autoComplete="off"
            spellCheck={false}
            placeholder={`${providerName} API key`}
            value={key}
            onChange={(e) => setKey(e.target.value)}
            autoFocus
          />
          <ButtonRow
            others={[
              <button key="cancel" type="button" className="btn small" onClick={() => setEditing(false)}>
                Cancel
              </button>,
            ]}
            primary={
              <button type="submit" className="btn small primary" disabled={!key.trim()}>
                Save
              </button>
            }
          />
        </form>
      )}
      {editing && info.keyUrl && (
        <p className="muted small" style={{ margin: "4px 0 0" }}>
          Get a free key from{" "}
          <button type="button" className="btn link" onClick={() => void backend.openUrl(info.keyUrl!)}>
            {providerName}
          </button>
          . The key is stored on this computer only.
        </p>
      )}
      {error && <p className="small" style={{ color: "var(--bad)", margin: "4px 0 0" }}>{error}</p>}
    </div>
  );
}

function UpdatesCard({
  automatic,
  lastCheckMs,
  onAutomatic,
}: {
  automatic: boolean;
  lastCheckMs: number | null;
  onAutomatic: (value: boolean) => void;
}) {
  const backend = useBackend();
  const updates = useUpdates();
  const { reload } = useSettings();
  const [version, setVersion] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<UpdateCheck | null>(null);

  useEffect(() => {
    let active = true;
    backend
      .appVersion()
      .then((v) => active && setVersion(v))
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [backend]);

  const check = async () => {
    setChecking(true);
    setResult(null);
    const r = await updates.checkNow();
    setResult(r);
    setChecking(false);
    await reload();
  };

  return (
    <div className="card stack" style={{ gap: 8 }} aria-labelledby="updates-title">
      <h2 id="updates-title">Updates</h2>
      <label className="check">
        <input type="checkbox" checked={automatic} onChange={(e) => onAutomatic(e.target.checked)} />
        Check for updates automatically
      </label>
      <div className="muted small">When a new version is found, you're asked before anything downloads.</div>
      <div className="row" style={{ gap: 10 }}>
        <span className="muted small">
          {version ? `Version ${version}` : ""}
          {lastCheckMs !== null ? ` · checked ${formatWhen(lastCheckMs)}` : ""}
        </span>
        <button type="button" className="btn small" onClick={() => void check()} disabled={checking}>
          {checking ? "Checking…" : "Check now"}
        </button>
      </div>
      <div aria-live="polite" className="small">
        {result?.kind === "upToDate" && <span>Media Identifier is up to date.</span>}
        {result?.kind === "failed" && <span style={{ color: "var(--bad)" }}>Couldn't check for updates.</span>}
        {result?.kind === "available" && updates.readyVersion && (
          <span>Version {updates.readyVersion} is downloaded and installs when you relaunch.</span>
        )}
        {result?.kind === "available" && !updates.readyVersion && updates.downloadingVersion && (
          <span>Version {updates.downloadingVersion} is downloading.</span>
        )}
      </div>
    </div>
  );
}
