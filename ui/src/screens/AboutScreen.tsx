// About: version, license, and credits for the data sources and bundled software. TVmaze's
// CC BY-SA 4.0 license and TMDb's terms require a visible credit.

import { useEffect, useState } from "react";

import { useBackend } from "../api";
import { AppIcon } from "../components/common";

interface Credit {
  name: string;
  url: string;
  text: string;
}

export const CREDITS: Credit[] = [
  {
    name: "TVmaze",
    url: "https://www.tvmaze.com",
    text: "Show and episode information from TVmaze, licensed under CC BY-SA 4.0.",
  },
  {
    name: "TMDb",
    url: "https://www.themoviedb.org",
    text: "When you add a TMDb key: this product uses the TMDB API but is not endorsed or certified by TMDB.",
  },
  {
    name: "SubDL",
    url: "https://subdl.com",
    text: "Subtitles from SubDL, used with your own API key.",
  },
  {
    name: "LRCLIB",
    url: "https://lrclib.net",
    text: "Song lyrics from LRCLIB.",
  },
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
    text: "Built with Tauri (Apache 2.0 or MIT License) and React (MIT License).",
  },
];

export function AboutScreen() {
  const backend = useBackend();
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
        <ul className="stack" style={{ listStyle: "none", margin: 0, padding: 0, gap: 8 }}>
          {CREDITS.map((c) => (
            <li key={c.name}>
              <button type="button" className="btn link" onClick={() => void backend.openUrl(c.url)}>
                {c.name}
              </button>
              <span className="muted"> · {c.text}</span>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
