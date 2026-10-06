// Credits for the online sources whose data the app shows. TVmaze's data is CC BY-SA 4.0, which
// asks for a credit with links to TVmaze and the license; TMDb's API terms ask for its notice and
// logo wherever its data is used. The texts and links come from the app (`attributions`), so the
// window and `mi_sources::attribution` cannot disagree.

import { useEffect, useState } from "react";

import { useBackend } from "../api";
import tmdbLogo from "../assets/tmdb-logo.svg";
import type { Attribution, ProviderId } from "../types/generated";

/** The credits the app returns, or an empty list until they arrive. */
export function useAttributions(): Attribution[] {
  const backend = useBackend();
  const [list, setList] = useState<Attribution[]>([]);
  useEffect(() => {
    let active = true;
    backend
      .attributions()
      .then((a) => active && setList(a))
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [backend]);
  return list;
}

/** TMDb's logo, as its attribution rules ask. */
export function TmdbLogo({ height = 12 }: { height?: number }) {
  return <img src={tmdbLogo} alt="TMDB" height={height} style={{ verticalAlign: "middle" }} />;
}

/** One source's credit: its sentence linked to its site, and its license linked when it has one. */
export function Credit({ attribution }: { attribution: Attribution }) {
  const backend = useBackend();
  return (
    <span>
      {attribution.provider === "tmdb" && (
        <>
          <TmdbLogo />{" "}
        </>
      )}
      <button type="button" className="btn link" onClick={() => void backend.openUrl(attribution.url)}>
        {attribution.text}
      </button>
      {attribution.license && attribution.licenseUrl && (
        <>
          {" ("}
          <button type="button" className="btn link" onClick={() => void backend.openUrl(attribution.licenseUrl!)}>
            {attribution.license}
          </button>
          {")"}
        </>
      )}
    </span>
  );
}

/**
 * The credit line for show and episode information: TVmaze always (search and episode lists),
 * and TMDb's notice and logo when its numbering is used.
 */
export function EpisodeDataCredit({ tmdb }: { tmdb: boolean }) {
  const all = useAttributions();
  const pick = (p: ProviderId) => all.find((a) => a.provider === p);
  const tvmaze = pick("tvmaze");
  const tmdbCredit = tmdb ? pick("tmdb") : undefined;
  if (!tvmaze) return null;
  return (
    <span className="muted small credit-line">
      <Credit attribution={tvmaze} />
      {tmdbCredit && (
        <>
          {" · "}
          <Credit attribution={tmdbCredit} />
        </>
      )}
    </span>
  );
}
