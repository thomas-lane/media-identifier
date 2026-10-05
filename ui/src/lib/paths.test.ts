import { describe, expect, it } from "vitest";

import { baseName, dirName, folderFromDrop, joinPath, relativeParts } from "./paths";

describe("paths", () => {
  it("handles macOS and Windows separators", () => {
    expect(baseName("/Volumes/Rips/title_t03.mkv")).toBe("title_t03.mkv");
    expect(baseName("C:\\Rips\\Disc 1\\title_t03.mkv")).toBe("title_t03.mkv");
    expect(baseName("VIDEO_TS/VTS_01_1.VOB")).toBe("VTS_01_1.VOB");
    expect(dirName("C:\\Rips\\title.mkv")).toBe("C:\\Rips");
    expect(dirName("/Rips/title.mkv")).toBe("/Rips");
    expect(joinPath("C:\\Rips", "list.csv")).toBe("C:\\Rips\\list.csv");
    expect(joinPath("/Rips/", "list.csv")).toBe("/Rips/list.csv");
  });

  it("splits a path below a root and rejects paths outside it", () => {
    expect(relativeParts("/Rips", "/Rips/Show (1973)/Season 02/a.mkv")).toEqual(["Show (1973)", "Season 02", "a.mkv"]);
    expect(relativeParts("C:\\Rips", "C:\\Rips\\Show\\a.mkv")).toEqual(["Show", "a.mkv"]);
    expect(relativeParts("/Rips", "/RipsOther/a.mkv")).toBeNull();
  });

  it("scans the folder that was dropped, or the folder of a dropped video file", () => {
    expect(folderFromDrop(["/Rips/DISC1"])).toBe("/Rips/DISC1");
    expect(folderFromDrop(["/Rips/DISC1/title_t00.mkv", "/Rips/DISC1/title_t01.mkv"])).toBe("/Rips/DISC1");
    expect(folderFromDrop(["C:\\Rips\\DISC1\\title_t00.MKV"])).toBe("C:\\Rips\\DISC1");
    expect(folderFromDrop([])).toBeNull();
  });
});
