import { describe, expect, it } from "vitest";

import { isApiError, toApiError } from "./errors";

describe("errors", () => {
  it("passes ApiError through and wraps anything else as internal", () => {
    expect(isApiError({ code: "busy", message: "x" })).toBe(true);
    expect(toApiError({ code: "busy", message: "x" })).toEqual({ code: "busy", message: "x" });
    expect(toApiError(new Error("boom"))).toEqual({ code: "internal", message: "boom" });
    expect(toApiError("text")).toEqual({ code: "internal", message: "text" });
  });
});
