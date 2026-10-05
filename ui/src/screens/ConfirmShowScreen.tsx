// Confirm show: search the show, see what was found in the folder (including the play-all and
// the MakeMKV short-title warning), narrow seasons/order/language, and start identifying.

import { useEffect, useId, useState } from "react";

import { toApiError, useBackend } from "../api";
import { ButtonRow, Pill } from "../components/common";
import { formatDuration, plural } from "../lib/format";
import { baseName } from "../lib/paths";
import { useIdentify } from "../state/identify";
import { useModel } from "../state/model";
import { useSettings } from "../state/settings";
import type { EpisodeOrdering, ScanSummary, ShowCandidate } from "../types/generated";

/** Files at most this long are "short" (one episode of a musical short, or a sitcom act). */
const SHORT_FILE_S = 6 * 60;

export const LANGUAGES: { code: string; name: string }[] = [
  { code: "en", name: "English" },
  { code: "es", name: "Spanish" },
  { code: "fr", name: "French" },
  { code: "de", name: "German" },
  { code: "it", name: "Italian" },
  { code: "pt", name: "Portuguese" },
  { code: "nl", name: "Dutch" },
  { code: "ja", name: "Japanese" },
];

export function ConfirmShowScreen() {
  const backend = useBackend();
  const { state, dispatch, start } = useIdentify();
  const { settings } = useSettings();
  const model = useModel(settings?.speechModel ?? null);
  const scan = state.scan;
  const [query, setQuery] = useState(scan?.showGuess ?? "");
  const [candidates, setCandidates] = useState<ShowCandidate[] | null>(null);
  const [searching, setSearching] = useState(Boolean(scan?.showGuess));
  const [searchError, setSearchError] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [season, setSeason] = useState<string>("all");
  const [ordering, setOrdering] = useState<EpisodeOrdering>("aired");
  const [language, setLanguage] = useState<string>(settings?.language ?? "en");
  const ids = useId();

  const search = async (q: string) => {
    if (!q.trim()) return;
    setSearching(true);
    setSearchError(null);
    try {
      const found = await backend.searchShows(q);
      setCandidates(found);
      setPicked(found[0] ? key(found[0]) : null);
    } catch (e) {
      setSearchError(toApiError(e).message);
    } finally {
      setSearching(false);
    }
  };

  // Search once for the folder's guess; later searches are the user's.
  const guess = scan?.showGuess ?? null;
  useEffect(() => {
    if (!guess) return;
    let active = true;
    backend
      .searchShows(guess)
      .then((found) => {
        if (!active) return;
        setCandidates(found);
        setPicked(found[0] ? key(found[0]) : null);
      })
      .catch((e: unknown) => active && setSearchError(toApiError(e).message))
      .finally(() => active && setSearching(false));
    return () => {
      active = false;
    };
  }, [backend, guess]);

  if (!scan) return null;
  const chosen = candidates?.find((c) => key(c) === picked) ?? null;
  const modelReady = model.status?.state.kind === "ready";
  const count = scan.candidateCount;
  const seasonCount = chosen?.show.seasonCount ?? 0;

  const identify = () => {
    if (!chosen) return;
    void start({
      folder: scan.folder,
      show: chosen.show,
      ordering,
      seasons: season === "all" ? null : [Number(season)],
      language,
    });
  };

  return (
    <section className="screen" aria-labelledby="show-title">
      <div>
        <div className="muted small mono">{scan.folder}</div>
        <h1 id="show-title">Which show is this?</h1>
      </div>
      <form
        className="row"
        role="search"
        onSubmit={(e) => {
          e.preventDefault();
          void search(query);
        }}
      >
        <label htmlFor={`${ids}-q`} className="visually-hidden">
          Show name
        </label>
        <input id={`${ids}-q`} className="field" value={query} onChange={(e) => setQuery(e.target.value)} />
        <button type="submit" className="btn" disabled={searching}>
          {searching ? "Searching…" : "Search"}
        </button>
      </form>
      {searchError && (
        <p role="alert" className="alert bad">
          Couldn't search: {searchError}
        </p>
      )}
      {candidates && candidates.length === 0 && <p className="muted">No shows found. Try a shorter or different name.</p>}
      {candidates && candidates.length > 0 && (
        <fieldset className="stack" style={{ border: 0, padding: 0, margin: 0, gap: 8 }}>
          <legend className="visually-hidden">Show</legend>
          {candidates.map((c) => {
            const k = key(c);
            const facts = [
              c.show.seasonCount ? plural(c.show.seasonCount, "season") : null,
              c.show.episodeCount ? plural(c.show.episodeCount, "episode") : null,
              c.show.kind && !c.show.seasonCount ? c.show.kind.toLowerCase() : null,
            ].filter(Boolean);
            return (
              <label key={k} className={`card show-option${picked === k ? " selected" : ""}`}>
                <input type="radio" name="show" checked={picked === k} onChange={() => setPicked(k)} />
                <span className="grow">
                  <b>{c.show.name}</b>
                  {c.show.year ? ` (${c.show.year})` : ""}
                  {facts.length > 0 && <span className="muted"> · {facts.join(" · ")}</span>}
                  {c.guessedFromFolder && (
                    <>
                      <br />
                      <span className="muted small">Guessed from the folder name</span>
                    </>
                  )}
                </span>
              </label>
            );
          })}
        </fieldset>
      )}
      <FilesFound scan={scan} />
      <details className="card">
        <summary style={{ cursor: "default", fontWeight: 600 }}>Narrow the search (optional)</summary>
        <div className="two-col" style={{ marginTop: 10, gridTemplateColumns: "repeat(3, minmax(0, 1fr))" }}>
          <label className="label">
            Seasons
            <select className="field" value={season} onChange={(e) => setSeason(e.target.value)}>
              <option value="all">All seasons</option>
              {Array.from({ length: seasonCount }, (_, i) => (
                <option key={i + 1} value={String(i + 1)}>
                  Season {i + 1}
                </option>
              ))}
            </select>
          </label>
          <label className="label">
            Episode order
            <select className="field" value={ordering} onChange={(e) => setOrdering(e.target.value as EpisodeOrdering)}>
              <option value="aired">As aired</option>
              <option value="dvd">DVD order</option>
            </select>
          </label>
          <label className="label">
            Language
            <select className="field" value={language} onChange={(e) => setLanguage(e.target.value)}>
              {LANGUAGES.map((l) => (
                <option key={l.code} value={l.code} disabled={settings?.speechModel === "fast" && l.code !== "en"}>
                  {l.name}
                </option>
              ))}
            </select>
          </label>
        </div>
        {settings?.speechModel === "fast" && (
          <p className="muted small" style={{ margin: "8px 0 0" }}>
            The Fast speech model understands English only. Choose Accurate in Settings for other languages.
          </p>
        )}
      </details>
      {state.error && (
        <p role="alert" className="alert bad">
          {state.error}
        </p>
      )}
      <div className="row-between">
        <span className="muted small">
          Show information from{" "}
          <button type="button" className="btn link" onClick={() => void backend.openUrl("https://www.tvmaze.com")}>
            TVmaze
          </button>
          {!modelReady && " · The speech model is still downloading."}
        </span>
        <ButtonRow
          others={[
            <button key="back" type="button" className="btn" onClick={() => dispatch({ type: "reset" })}>
              Back
            </button>,
          ]}
          primary={
            <button type="button" className="btn primary" disabled={!chosen || !modelReady || count === 0} onClick={identify}>
              Identify {plural(count, "file")}
            </button>
          }
        />
      </div>
    </section>
  );
}

function key(c: ShowCandidate): string {
  return `${c.show.showRef.provider}:${c.show.showRef.id}`;
}

function minutes(seconds: number): number {
  return Math.max(1, Math.round(seconds / 60));
}

/** The "Files found" summary: play-all, short and longer files, and scan warnings. */
export function FilesFound({ scan }: { scan: ScanSummary }) {
  const candidates = scan.files.filter((f) => f.role === "candidate");
  const lengths = candidates.map((f) => f.probe?.durationS ?? 0);
  const short = lengths.filter((d) => d <= SHORT_FILE_S);
  const long = lengths.filter((d) => d > SHORT_FILE_S).sort((a, b) => a - b);
  const range = (ds: number[]) => {
    const lo = minutes(Math.min(...ds));
    const hi = minutes(Math.max(...ds));
    return lo === hi ? `${lo} min` : `${lo}–${hi} min`;
  };
  const longText =
    long.length <= 3 ? `${long.map(minutes).join(long.length === 2 ? " and " : ", ")} min` : range(long);
  return (
    <div className="card" aria-labelledby="found-title">
      <div id="found-title" className="card-title" style={{ marginBottom: 8 }}>
        Files found: {scan.files.length}
      </div>
      <div className="row" style={{ flexWrap: "wrap" }}>
        {scan.playAll && (
          <Pill tone="info">
            1 play-all title ({formatDuration(scan.playAll.durationS)}, {plural(scan.playAll.chapterCount, "chapter")})
          </Pill>
        )}
        {short.length > 0 && (
          <Pill tone="gray">
            {plural(short.length, "short file")} ({range(short)})
          </Pill>
        )}
        {long.length > 0 && (
          <Pill tone="gray">
            {plural(long.length, "longer file")} ({longText})
          </Pill>
        )}
      </div>
      {scan.playAll && (
        <p className="muted small" style={{ margin: "10px 0 0" }}>
          The play-all title will be used to work out the disc order. It won't be renamed.
        </p>
      )}
      {scan.warnings.map((w, i) => (
        <ScanWarningText key={i} warning={w} />
      ))}
    </div>
  );
}

function ScanWarningText({ warning }: { warning: ScanSummary["warnings"][number] }) {
  switch (warning.kind) {
    case "missingShortTitles": {
      const missing = warning.chapters - warning.shortFiles;
      return (
        <div className="small warn-text" style={{ marginTop: 8 }}>
          <p style={{ margin: 0 }}>
            ⚠︎ The play-all has {warning.chapters} chapters but only {warning.shortFiles} short files were found. MakeMKV skips
            titles under 2 minutes by default; re-rip with a lower minimum length to get the missing {missing}.
          </p>
          <details style={{ marginTop: 4, color: "var(--text)" }}>
            <summary style={{ cursor: "default", color: "var(--accent)" }}>How?</summary>
            <p className="muted" style={{ margin: "4px 0 0" }}>
              In MakeMKV, open Preferences, choose the Video tab, set “Minimum title length (seconds)” to a smaller number
              (for example 10), then open the disc again and rip the titles that were missing.
            </p>
          </details>
        </div>
      );
    }
    case "unreadable":
      return (
        <p className="small warn-text" style={{ margin: "8px 0 0" }}>
          ⚠︎ {baseName(warning.fileId)} couldn't be read: {warning.reason}
        </p>
      );
    case "noAudio":
      return (
        <p className="small warn-text" style={{ margin: "8px 0 0" }}>
          ⚠︎ {baseName(warning.fileId)} has no audio, so it can't be identified by listening.
        </p>
      );
  }
}
