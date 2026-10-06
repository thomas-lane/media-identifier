// Turning a rename plan into the preview lines and summaries shown on the Rename screen.

import type { PlanConflict, RenameItem, UntouchedFile } from "../types/generated";
import { baseName, relativeParts } from "./paths";
import { plural } from "./format";

/** One line of the folder-tree preview. */
export interface PreviewLine {
  depth: number;
  text: string;
  /** Original file name, for file lines. */
  from: string | null;
}

/**
 * The preview as an indented folder tree below `root`, folders first in path order, each file
 * annotated with its current name. Items outside `root` are listed with their full path.
 */
export function previewTree(root: string, items: RenameItem[]): PreviewLine[] {
  const rows = items
    .map((item) => ({ item, parts: relativeParts(root, item.to) ?? [item.to] }))
    .sort((a, b) => a.parts.join("/").localeCompare(b.parts.join("/")));
  const lines: PreviewLine[] = [];
  let previous: string[] = [];
  for (const { item, parts } of rows) {
    const dirs = parts.slice(0, -1);
    let shared = 0;
    while (shared < dirs.length && shared < previous.length && dirs[shared] === previous[shared]) shared++;
    for (let d = shared; d < dirs.length; d++) lines.push({ depth: d, text: `${dirs[d]}/`, from: null });
    lines.push({ depth: dirs.length, text: parts.at(-1) ?? item.to, from: baseName(item.from) });
    previous = dirs;
  }
  return lines;
}

/**
 * "Not renamed: title_t00.mkv (play all) and 2 extras (title_t44.mkv, title_t45.mkv) stay as
 * they are."
 * Null when nothing is left untouched.
 */
export function untouchedSummary(untouched: UntouchedFile[], verb = "renamed"): string | null {
  if (untouched.length === 0) return null;
  const playAll = untouched.filter((u) => u.reason === "playAll").map((u) => `${baseName(u.path)} (play all)`);
  const extras = untouched.filter((u) => u.reason === "extra").map((u) => baseName(u.path));
  const skipped = untouched.filter((u) => u.reason === "skipped");
  const parts = [...playAll];
  if (extras.length === 1) parts.push(`${extras[0]} (extra)`);
  else if (extras.length > 1) {
    const names = extras.length > 3 ? `${extras.slice(0, 3).join(", ")}, …` : extras.join(", ");
    parts.push(`${extras.length} extras (${names})`);
  }
  if (skipped.length > 0) parts.push(`${plural(skipped.length, "skipped or unchecked file")}`);
  const list = parts.length > 1 ? `${parts.slice(0, -1).join(", ")} and ${parts.at(-1)}` : parts[0];
  return `Not ${verb}: ${list} ${untouched.length === 1 ? "stays" : "stay"} as ${untouched.length === 1 ? "it is" : "they are"}.`;
}

/** A conflict in plain words. */
export function conflictText(conflict: PlanConflict): string {
  switch (conflict.kind) {
    case "targetExists":
      return `${baseName(conflict.path)} already exists, so ${baseName(conflict.fileId)} can't take that name.`;
    case "duplicateTarget":
      return `${conflict.fileIds.map(baseName).join(" and ")} would both become ${baseName(conflict.path)}. Choose a different episode for one of them on the Review screen.`;
    case "sourceChanged":
      return `${baseName(conflict.path)} was moved, renamed or replaced since it was identified. Skip it on the Review screen, or identify the folder again.`;
    case "listExists":
      return `${baseName(conflict.path)} already exists. Choose it with … to replace it, or type another name.`;
  }
}
