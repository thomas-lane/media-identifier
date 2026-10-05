import { describe, expect, it } from "vitest";

import { formatDay, formatDuration, formatEpisodeCode, formatMegabytes, formatPercent, formatTimeLeft, plural } from "./format";

describe("format", () => {
  it("shows lengths as m:ss, or h:mm:ss from an hour", () => {
    expect(formatDuration(185)).toBe("3:05");
    expect(formatDuration(8091)).toBe("2:14:51");
    expect(formatDuration(59.6)).toBe("1:00");
  });

  it("rounds sizes to whole megabytes", () => {
    expect(formatMegabytes(574_041_195)).toBe("574 MB");
    expect(formatMegabytes(1_234_567_890)).toBe("1,235 MB");
  });

  it("words the time left plainly", () => {
    expect(formatTimeLeft(30)).toBe("Less than a minute left");
    expect(formatTimeLeft(70)).toBe("About 1 minute left");
    expect(formatTimeLeft(240)).toBe("About 4 minutes left");
    expect(formatTimeLeft(3900)).toBe("About 1 h 5 min left");
  });

  it("formats episode codes, percentages and counts", () => {
    expect(formatEpisodeCode({ season: 2, number: 3 })).toBe("S02E03");
    expect(formatEpisodeCode({ season: 12, number: 104 })).toBe("S12E104");
    expect(formatPercent(0.614)).toBe("61%");
    expect(formatPercent(1.4)).toBe("100%");
    expect(plural(1, "file")).toBe("1 file");
    expect(plural(2, "copy", "copies")).toBe("2 copies");
  });

  it("names days relative to today", () => {
    const now = new Date(2026, 9, 5, 15, 0).getTime();
    expect(formatDay(new Date(2026, 9, 5, 9, 41).getTime(), now)).toBe("Today");
    expect(formatDay(new Date(2026, 9, 4, 23, 0).getTime(), now)).toBe("Yesterday");
    expect(formatDay(new Date(2026, 9, 3, 12, 0).getTime(), now)).toBe("Oct 3");
    expect(formatDay(new Date(2025, 11, 31, 12, 0).getTime(), now)).toBe("Dec 31, 2025");
  });
});
