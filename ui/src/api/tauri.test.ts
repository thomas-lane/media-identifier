import { beforeEach, describe, expect, it, vi } from "vitest";

import type { FileDropEvent } from "./backend";

const invoke = vi.fn();
type DragHandler = (event: { payload: unknown }) => void;
let dragHandler: DragHandler | null = null;
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async (handler: DragHandler) => {
      dragHandler = handler;
      return () => {};
    },
  }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => "/picked"),
  save: vi.fn(async () => "/picked/list.csv"),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async () => {}), openPath: vi.fn(async () => {}) }));

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

  it("asks for a CSV path with a CSV filter", async () => {
    const dialog = await import("@tauri-apps/plugin-dialog");
    await expect(tauriBackend.chooseSaveFile("/r/list.csv")).resolves.toBe("/picked/list.csv");
    expect(dialog.save).toHaveBeenCalledWith({ defaultPath: "/r/list.csv", filters: [{ name: "CSV", extensions: ["csv"] }] });
  });

  it("opens files with the opener plugin", async () => {
    const opener = await import("@tauri-apps/plugin-opener");
    await tauriBackend.openFile("/r/title_t03.mkv");
    expect(opener.openPath).toHaveBeenCalledWith("/r/title_t03.mkv");
  });

  it("maps the web view's drag-and-drop events to file drop events", async () => {
    const seen: FileDropEvent[] = [];
    await tauriBackend.onFileDrop((e) => seen.push(e));
    dragHandler!({ payload: { type: "enter", paths: ["/r"], position: { x: 0, y: 0 } } });
    dragHandler!({ payload: { type: "over", position: { x: 1, y: 1 } } });
    dragHandler!({ payload: { type: "drop", paths: ["/r"], position: { x: 1, y: 1 } } });
    dragHandler!({ payload: { type: "leave" } });
    expect(seen).toEqual([{ kind: "hover" }, { kind: "drop", paths: ["/r"] }, { kind: "cancel" }]);
  });
});
