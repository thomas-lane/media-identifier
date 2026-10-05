// Path helpers for display. Paths come from the app as native strings: `/` on macOS, `\` (or a
// mix) on Windows, so both separators are accepted.

const SEPARATOR = /[\\/]/;

/** Last component of a path or file id. */
export function baseName(path: string): string {
  const parts = path.split(SEPARATOR).filter((p) => p.length > 0);
  return parts.at(-1) ?? path;
}

/** Everything before the last component, without a trailing separator; "" when there is none. */
export function dirName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const index = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return index <= 0 ? (index === 0 ? trimmed.slice(0, 1) : "") : trimmed.slice(0, index);
}

/** The separator a path uses (Windows paths with a drive letter or backslashes use `\`). */
export function separatorOf(path: string): "/" | "\\" {
  return /^[A-Za-z]:\\/.test(path) || (path.includes("\\") && !path.includes("/")) ? "\\" : "/";
}

/** Joins a folder and a name with the folder's separator. */
export function joinPath(folder: string, name: string): string {
  const sep = separatorOf(folder);
  return folder.endsWith(sep) ? `${folder}${name}` : `${folder}${sep}${name}`;
}

/**
 * The path below `root` split into components, or null when `path` is not inside `root`.
 * Comparison ignores the separator style so mixed Windows paths still match.
 */
export function relativeParts(root: string, path: string): string[] | null {
  const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");
  const r = norm(root);
  const p = norm(path);
  if (!p.startsWith(`${r}/`)) return null;
  return p.slice(r.length + 1).split("/").filter((s) => s.length > 0);
}

const VIDEO_EXTENSIONS = [".mkv", ".mp4", ".m4v", ".mov", ".avi", ".ts", ".m2ts", ".mpg", ".mpeg", ".vob"];

/** True when the path names a video file (by extension). */
export function isVideoFile(path: string): boolean {
  const lower = path.toLowerCase();
  return VIDEO_EXTENSIONS.some((ext) => lower.endsWith(ext));
}

/**
 * The folder to scan for a drop: a dropped folder as is, or the folder containing a dropped
 * video file. Null when nothing was dropped.
 */
export function folderFromDrop(paths: string[]): string | null {
  const first = paths[0];
  if (!first) return null;
  return isVideoFile(first) ? dirName(first) || first : first;
}
