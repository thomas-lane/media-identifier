import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => "/picked") }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async () => {}) }));

const { tauriBackend } = await import("./tauri");

describe("tauri backend", () => {
  beforeEach(() => invoke.mockReset().mockResolvedValue(undefined));

  it("sends camelCase argument names that Tauri maps to the Rust parameters", async () => {
    await tauriBackend.cancelIdentification("job-1");
    expect(invoke).toHaveBeenCalledWith("cancel_identification", { jobId: "job-1" });
    await tauriBackend.setApiKey("tmdb", null);
    expect(invoke).toHaveBeenCalledWith("set_api_key", { provider: "tmdb", key: null });
  });

  it("returns the picked folder, or null when the dialog is cancelled", async () => {
    await expect(tauriBackend.chooseFolder()).resolves.toBe("/picked");
    const dialog = await import("@tauri-apps/plugin-dialog");
    vi.mocked(dialog.open).mockResolvedValueOnce(null);
    await expect(tauriBackend.chooseFolder()).resolves.toBeNull();
  });
});
