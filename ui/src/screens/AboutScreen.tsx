// About: version, license, and credits for the data sources and bundled software. TVmaze's
// CC BY-SA 4.0 license (credit with links) and TMDb's terms (notice and logo) require a visible
// credit; the texts come from the app (`attributions`).

import { useEffect, useState } from "react";

import { useBackend } from "../api";
import { Credit, useAttributions } from "../components/Credits";
import { AppIcon } from "../components/common";
import type { ProviderId } from "../types/generated";

interface SoftwareCredit {
  name: string;
  url: string;
  text: string;
}

/** Software built into the app. The online sources' credits come from the app itself. */
export const SOFTWARE: SoftwareCredit[] = [
  {
    name: "whisper.cpp",
    url: "https://github.com/ggml-org/whisper.cpp",
    text: "Speech recognition by whisper.cpp (MIT License) with OpenAI's Whisper models (MIT License).",
  },
  {
    name: "FFmpeg",
    url: "https://ffmpeg.org",
    text: "Audio decoding by FFmpeg, licensed under the LGPL 2.1 or later and included as separate programs.",
  },
  {
    name: "Tauri",
    url: "https://tauri.app",
    text: "Built with Tauri (Apache 2.0 or MIT License).",
  },
  {
    name: "React",
    url: "https://react.dev",
    text: "User interface built with React (MIT License).",
  },
];

const SOURCE_NAMES: Partial<Record<ProviderId, string>> = {
  tvmaze: "TVmaze",
  tmdb: "TMDb",
  subdl: "SubDL",
  lrclib: "LRCLIB",
};

export function AboutScreen() {
  const backend = useBackend();
  const sources = useAttributions();
  const [version, setVersion] = useState<string | null>(null);
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

  return (
    <section className="screen" aria-labelledby="about-title">
      <div className="row" style={{ gap: 14 }}>
        <AppIcon />
        <div>
          <h1 id="about-title">Media Identifier</h1>
          <div className="muted">{version ? `Version ${version} · ` : ""}MIT License · Copyright © 2026 Thomas Lane</div>
        </div>
      </div>
      <div className="card stack">
        <h2>Credits</h2>
        <h3 className="eyebrow">Data</h3>
        <ul className="stack" style={{ listStyle: "none", margin: 0, padding: 0, gap: 8 }}>
          {sources.map((a) => (
            <li key={a.provider}>
              <b>{SOURCE_NAMES[a.provider] ?? a.provider}</b>
              <span className="muted"> · </span>
              <Credit attribution={a} />
              {a.provider === "tmdb" && <span className="muted small"> Used when you add a TMDb key.</span>}
            </li>
          ))}
        </ul>
        <h3 className="eyebrow">Software</h3>
        <ul className="stack" style={{ listStyle: "none", margin: 0, padding: 0, gap: 8 }}>
          {SOFTWARE.map((c) => (
            <li key={c.name}>
              <button type="button" className="btn link" onClick={() => void backend.openUrl(c.url)}>
                {c.name}
              </button>
              <span className="muted"> · {c.text}</span>
            </li>
          ))}
        </ul>
        <p className="muted small" style={{ margin: 0 }}>
          The license texts and copyright notices of all included software are installed with the app, in the
          licenses folder.
        </p>
      </div>
    </section>
  );
}
