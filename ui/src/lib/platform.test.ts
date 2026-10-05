import { describe, expect, it } from "vitest";

import { detectPlatform, orderButtons } from "./platform";

describe("platform", () => {
  it("detects Windows from the WebView2 user agent and macOS otherwise", () => {
    expect(detectPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Edg/129.0", "")).toBe("windows");
    expect(detectPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15", "")).toBe("mac");
    expect(detectPlatform("Mozilla/5.0 (Macintosh)", "?platform=windows")).toBe("windows");
  });

  it("puts the default button last on macOS and first on Windows", () => {
    expect(orderButtons("mac", ["Cancel"], "OK")).toEqual(["Cancel", "OK"]);
    expect(orderButtons("windows", ["Cancel"], "OK")).toEqual(["OK", "Cancel"]);
  });
});
