// Plain-language formatting shared by the screens. Every function is pure so it can be tested
// without rendering.

import type { EpisodeKey } from "../types/generated";

/** `h:mm:ss` for an hour or more, otherwise `m:ss` (as video players show lengths). */
export function formatDuration(seconds: number): string {
  const total = Math.max(0, Math.round(seconds));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** Whole megabytes (1 MB = 1,000,000 bytes, as Finder and Explorer's size column round). */
export function formatMegabytes(bytes: number): string {
  return `${Math.round(bytes / 1_000_000).toLocaleString("en-US")} MB`;
}

/** "About 4 minutes left", "About 1 minute left", "Less than a minute left". */
export function formatTimeLeft(seconds: number): string {
  if (seconds < 60) return "Less than a minute left";
  if (seconds < 3600) {
    const minutes = Math.round(seconds / 60);
    return `About ${minutes} minute${minutes === 1 ? "" : "s"} left`;
  }
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.round((seconds % 3600) / 60);
  return `About ${hours} h ${minutes} min left`;
}

/** `S02E03`. */
export function formatEpisodeCode(key: EpisodeKey): string {
  return `S${String(key.season).padStart(2, "0")}E${String(key.number).padStart(2, "0")}`;
}

/** Whole percent, `61%`. */
export function formatPercent(score: number): string {
  return `${Math.round(Math.min(1, Math.max(0, score)) * 100)}%`;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function startOfDay(ms: number): number {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** "Today", "Yesterday", "Oct 3", or "Oct 3, 2025" for another year (local time). */
export function formatDay(ms: number, nowMs: number = Date.now()): string {
  const days = Math.round((startOfDay(nowMs) - startOfDay(ms)) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  const d = new Date(ms);
  const sameYear = d.getFullYear() === new Date(nowMs).getFullYear();
  return `${MONTHS[d.getMonth()]} ${d.getDate()}${sameYear ? "" : `, ${d.getFullYear()}`}`;
}

/** Local `9:41` style clock time. */
export function formatClock(ms: number): string {
  const d = new Date(ms);
  return `${d.getHours()}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** "today 9:41", "yesterday 9:41", "Oct 3 9:41". */
export function formatWhen(ms: number, nowMs: number = Date.now()): string {
  const day = formatDay(ms, nowMs);
  const lead = day === "Today" || day === "Yesterday" ? day.toLowerCase() : day;
  return `${lead} ${formatClock(ms)}`;
}

/** "1 file", "2 files". */
export function plural(count: number, one: string, many = `${one}s`): string {
  return `${count.toLocaleString("en-US")} ${count === 1 ? one : many}`;
}
