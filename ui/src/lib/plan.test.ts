import { describe, expect, it } from "vitest";

import type { RenameItem } from "../types/generated";
import { conflictText, previewTree, untouchedSummary } from "./plan";

function item(fileId: string, to: string): RenameItem {
  return { fileId, from: `/Rips/${fileId}`, to, episode: { season: 1, number: 1 }, title: "", heardSubtitlesTo: null, sizeBytes: 0 };
}

describe("plan preview", () => {
  it("draws the new names as a folder tree with the old names", () => {
    const lines = previewTree("/Rips", [
      item("t03.mkv", "/Rips/Show (1973)/Season 04/Show (1973) - S04E01 - B.mkv"),
      item("t11.mkv", "/Rips/Show (1973)/Season 02/Show (1973) - S02E03 - A.mkv"),
      item("t12.mkv", "/Rips/Show (1973)/Season 02/Show (1973) - S02E04 - C.mkv"),
    ]);
    expect(lines.map((l) => `${"  ".repeat(l.depth)}${l.text}${l.from ? ` <- ${l.from}` : ""}`)).toEqual([
      "Show (1973)/",
      "  Season 02/",
      "    Show (1973) - S02E03 - A.mkv <- t11.mkv",
      "    Show (1973) - S02E04 - C.mkv <- t12.mkv",
      "  Season 04/",
      "    Show (1973) - S04E01 - B.mkv <- t03.mkv",
    ]);
  });

  it("summarises the files that are left alone", () => {
    expect(
      untouchedSummary([
        { fileId: "t00.mkv", path: "/Rips/t00.mkv", reason: "playAll" },
        { fileId: "t44.mkv", path: "/Rips/t44.mkv", reason: "extra" },
        { fileId: "t45.mkv", path: "/Rips/t45.mkv", reason: "extra" },
        { fileId: "t08.mkv", path: "/Rips/t08.mkv", reason: "skipped" },
      ]),
    ).toBe(
      "Not renamed: t00.mkv (play all), 2 extras (t44.mkv, t45.mkv) and 1 skipped or unchecked file stay as they are.",
    );
    expect(untouchedSummary([{ fileId: "t00.mkv", path: "/Rips/t00.mkv", reason: "playAll" }])).toBe(
      "Not renamed: t00.mkv (play all) stays as it is.",
    );
    expect(untouchedSummary([])).toBeNull();
  });

  it("explains conflicts", () => {
    expect(conflictText({ kind: "duplicateTarget", fileIds: ["a.mkv", "b.mkv"], path: "/R/x.mkv" })).toMatch(
      /^a\.mkv and b\.mkv would both become x\.mkv/,
    );
    expect(conflictText({ kind: "targetExists", fileId: "a.mkv", path: "/R/x.mkv" })).toMatch(/^x\.mkv already exists/);
    expect(conflictText({ kind: "sourceChanged", fileId: "a.mkv", path: "/R/a.mkv" })).toMatch(/^a\.mkv was moved, renamed or replaced/);
    expect(conflictText({ kind: "listExists", path: "/R/list.csv" })).toMatch(/^list\.csv already exists/);
  });
});
