import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { createMockBackend } from "./api";
import { MATCHES } from "./api/mockData";
import { manualBackend, renderApp } from "./test/render";

async function toConfirmScreen() {
  await userEvent.click(await screen.findByRole("button", { name: "Choose folder…" }));
  await screen.findByRole("heading", { name: "Which show is this?" });
}

async function toReview() {
  renderApp();
  await toConfirmScreen();
  const identify = await screen.findByRole("button", { name: "Identify 14 files" });
  await waitFor(() => expect(identify).toBeEnabled());
  await userEvent.click(identify);
  await screen.findByRole("heading", { name: /^Review · Schoolhouse Rock!/ }, { timeout: 3000 });
}

describe("App shell", () => {
  it("lists recent jobs and switches sections from the navigation", async () => {
    renderApp();
    expect(await screen.findByRole("button", { name: "The Clockwork Garden" })).toBeInTheDocument();
    expect(screen.getByText("2 to review")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Settings" })).toHaveAttribute("aria-current", "page");
  });

  it("opens Settings with the platform shortcut", async () => {
    renderApp();
    await userEvent.keyboard("{Meta>},{/Meta}");
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
  });

  it("credits the data sources on the About screen", async () => {
    const { backend } = renderApp();
    const open = vi.spyOn(backend, "openUrl");
    await userEvent.click(screen.getByRole("button", { name: "About" }));
    for (const name of ["Episode lists from TVmaze", "Subtitles from SubDL", "Lyrics from LRCLIB", "FFmpeg", "whisper.cpp"]) {
      expect(await screen.findByRole("button", { name })).toBeInTheDocument();
    }
    await userEvent.click(screen.getByRole("button", { name: "CC BY-SA 4.0" }));
    expect(open).toHaveBeenCalledWith("https://creativecommons.org/licenses/by-sa/4.0/");
    expect(
      screen.getByRole("button", {
        name: "This application uses TMDB and the TMDB APIs but is not endorsed, certified, or otherwise approved by TMDB.",
      }),
    ).toBeInTheDocument();
    expect(screen.getByAltText("TMDB")).toBeInTheDocument();
  });
});

describe("Start and Confirm show", () => {
  it("starts the one-time model download by itself and can pause and resume it", async () => {
    // Slow enough (20 steps of 150 ms) that the Pause click lands mid-download even when role
    // queries are slow on a busy machine; pausing before the first bytes would show "Download".
    const backend = createMockBackend({ downloadStepMs: 150 });
    renderApp(backend);
    expect(await screen.findByText(/One-time download: speech model/)).toBeInTheDocument();
    await screen.findByText(/^[1-9]\d* MB of 574 MB/);
    await userEvent.click(await screen.findByRole("button", { name: "Pause" }));
    expect(await screen.findByText(/^Paused at/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Resume" }));
    await waitFor(() => expect(screen.queryByText(/One-time download/)).not.toBeInTheDocument(), { timeout: 8000 });
  }, 15_000);

  it("summarises the folder, warns about missing short titles and guesses the show", async () => {
    renderApp();
    await toConfirmScreen();
    expect(screen.getByText("Files found: 15")).toBeInTheDocument();
    expect(screen.getByText("1 play-all title (43:05, 14 chapters)")).toBeInTheDocument();
    expect(screen.getByText("12 short files (3 min)")).toBeInTheDocument();
    expect(screen.getByText("2 longer files (8 and 23 min)")).toBeInTheDocument();
    expect(screen.getByText(/The play-all has 14 chapters but only 12 short files were found/)).toBeInTheDocument();
    const show = await screen.findByRole("radio", { name: /Schoolhouse Rock! \(1973\)/ });
    expect(show).toBeChecked();
    expect(screen.getByText("Guessed from the folder name")).toBeInTheDocument();
  });

  it("explains that a DVD folder copied from a disc is skipped", async () => {
    const backend = createMockBackend({ stepMs: 1, modelReady: true });
    renderApp({
      ...backend,
      scanFolder: async (folder) => ({
        ...(await backend.scanFolder(folder)),
        warnings: [{ kind: "unsupportedDiscFolder", folder: "VIDEO_TS", format: "dvd" }],
      }),
    });
    await toConfirmScreen();
    expect(screen.getByText(/VIDEO_TS is a DVD \(VIDEO_TS\) folder copied from a disc/)).toBeInTheDocument();
  });

  it("opens a dropped folder", async () => {
    const backend = createMockBackend({ stepMs: 1, modelReady: true });
    let drop: ((e: { kind: "drop"; paths: string[] }) => void) | null = null;
    renderApp({
      ...backend,
      onFileDrop: async (listener) => {
        drop = listener;
        return () => {};
      },
    });
    await screen.findByRole("button", { name: "Choose folder…" });
    await waitFor(() => expect(drop).not.toBeNull());
    drop!({ kind: "drop", paths: ["/Rips/DISC9/title_t00.mkv"] });
    expect(await screen.findByText("/Rips/DISC9")).toBeInTheDocument();
  });

  it("does not start until the speech model is ready", async () => {
    renderApp(createMockBackend({ stepMs: 1, downloadStepMs: 10_000 }));
    await toConfirmScreen();
    expect(await screen.findByText(/Identify is available once the speech model has downloaded/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Identify 14 files" })).toBeDisabled();
  });

  it("says why Identify waits for a stopped model download, and restarts it", async () => {
    const backend = createMockBackend({ stepMs: 1, downloadStepMs: 10_000 });
    renderApp(backend);
    await toConfirmScreen();
    await screen.findByText(/Identify is available once the speech model has downloaded/);
    // Paused before any bytes arrived, the model is simply not downloaded.
    await act(async () => backend.pauseModelDownload());
    expect(await screen.findByText(/The speech model is not downloaded yet/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    expect(await screen.findByText(/Identify is available once the speech model has downloaded/)).toBeInTheDocument();
  });

  it("warns that subtitles are off without a SubDL key, and credits TVmaze with its license", async () => {
    renderApp();
    await toConfirmScreen();
    expect(await screen.findByText(/Subtitles are off: without a SubDL key/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Add a free SubDL key in Settings" }));
    expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Identify" }));
    expect(await screen.findByRole("button", { name: "Episode lists from TVmaze" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "CC BY-SA 4.0" })).toBeInTheDocument();
    expect(screen.queryByAltText("TMDB")).not.toBeInTheDocument();
  });
});

describe("Identifying", () => {
  async function startManual() {
    const m = manualBackend();
    renderApp(m.backend);
    await toConfirmScreen();
    const identify = await screen.findByRole("button", { name: "Identify 14 files" });
    await waitFor(() => expect(identify).toBeEnabled());
    await userEvent.click(identify);
    await screen.findByRole("heading", { name: /^Identifying Schoolhouse Rock!/ });
    return m;
  }

  it("shows stage progress, file status, time left and allows early review", async () => {
    const { emit } = await startManual();
    const t11 = MATCHES.find((m) => m.fileId === "title_t11.mkv")!;
    emit(
      { kind: "started", jobId: "job-1", fileIds: ["title_t11.mkv", "title_t12.mkv", "title_t00.mkv"], accelerator: "appleGpu" },
      { kind: "stage", jobId: "job-1", stage: "episodeList", state: { kind: "done" } },
      { kind: "stage", jobId: "job-1", stage: "listening", state: { kind: "running", done: 1, total: 4 } },
      { kind: "file", jobId: "job-1", fileId: "title_t12.mkv", status: "listening", bestSoFar: null, verdict: null },
      { kind: "matched", jobId: "job-1", result: t11 },
      { kind: "eta", jobId: "job-1", seconds: 240 },
    );
    expect(screen.getByRole("heading", { name: "Identifying Schoolhouse Rock! · 2 files" })).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Listening" })).toHaveAttribute("aria-valuenow", "25");
    expect(screen.getByText("1/4")).toBeInTheDocument();
    expect(screen.getByText("About 4 minutes left · using the Apple GPU")).toBeInTheDocument();
    const row = screen.getByRole("row", { name: /title_t11\.mkv/ });
    expect(within(row).getByText("Check")).toBeInTheDocument();
    expect(within(row).getByText("Lucky Seven Sampson")).toBeInTheDocument();
    expect(within(screen.getByRole("row", { name: /title_t12\.mkv/ })).getByText("Listening")).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Main" })).toHaveTextContent("1/2");

    await userEvent.click(screen.getByRole("button", { name: "Review finished files" }));
    expect(await screen.findByText(/Still identifying: 1 of 2 files finished/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue to rename →" })).toBeDisabled();

    // Progress and Cancel stay within reach from Review.
    await userEvent.click(screen.getByRole("button", { name: "Show progress" }));
    expect(await screen.findByRole("heading", { name: /^Identifying Schoolhouse Rock!/ })).toHaveFocus();
    await userEvent.click(screen.getByRole("button", { name: "Review finished files" }));
    await userEvent.click(await screen.findByRole("button", { name: "Cancel identifying" }));
    await waitFor(() => expect(screen.queryByText(/Still identifying/)).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Continue to rename →" })).toBeEnabled();
  });

  it("cancels", async () => {
    const { emit } = await startManual();
    emit({ kind: "started", jobId: "job-1", fileIds: ["title_t11.mkv"], accelerator: "cpu" });
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(await screen.findByText("Identification was cancelled.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start over" })).toBeInTheDocument();
  });

  it("shows a failure message", async () => {
    const { emit } = await startManual();
    emit({ kind: "started", jobId: "job-1", fileIds: [], accelerator: "cpu" }, { kind: "failed", jobId: "job-1", message: "The speech model file is damaged." });
    expect(screen.getByRole("alert")).toHaveTextContent("Identification stopped: The speech model file is damaged.");
  });
});

describe("Review", () => {
  it("moves with the arrow keys and approves with Enter, then advances to the next file to check", async () => {
    await toReview();
    const list = screen.getByRole("listbox", { name: "Files" });
    expect(list).toHaveFocus();
    expect(screen.getByText(/9 approved · 3 to check · 2 extras/)).toBeInTheDocument();
    const selected = () => within(list).getAllByRole("option").find((o) => o.getAttribute("aria-selected") === "true");
    expect(selected()).toHaveTextContent("title_t01.mkv");
    await userEvent.keyboard("{ArrowDown}{ArrowDown}");
    expect(selected()).toHaveTextContent("title_t03.mkv");

    await userEvent.click(within(list).getByRole("option", { name: /title_t08\.mkv/ }));
    expect(list).toHaveFocus();
    expect(screen.getByRole("button", { name: "Approve" })).toBeEnabled();
    await userEvent.keyboard("{Enter}");
    expect(screen.getByText(/10 approved · 2 to check · 2 extras/)).toBeInTheDocument();
    expect(selected()).toHaveTextContent("title_t11.mkv");
  });

  it("shows the evidence: signals, quotes with overlaps and the play-all position", async () => {
    await toReview();
    await userEvent.click(screen.getByRole("option", { name: /title_t11\.mkv/ }));
    const panel = screen.getByLabelText("Evidence");
    expect(within(panel).getByLabelText("Suggested")).toHaveDisplayValue("S02E03 · Lucky Seven Sampson (61%)");
    expect(within(panel).getByRole("progressbar", { name: "Dialogue" })).toHaveAttribute("aria-valuenow", "48");
    expect(within(panel).getByRole("progressbar", { name: "Disc order" })).toHaveAttribute("aria-valuenow", "95");
    expect(within(panel).getByText(/Mostly music/)).toBeInTheDocument();
    expect(within(panel).getByText(/chapter 11\) fits this episode/)).toBeInTheDocument();
    expect(within(panel).getByText("lucky seven").tagName).toBe("MARK");
    expect(within(panel).getByText("Lucky Seven").tagName).toBe("MARK");
    const strip = within(panel).getByRole("list", { name: /Chapter 11 of the play-all/ });
    expect(within(strip).getByText("11")).toHaveAttribute("aria-current", "true");
  });

  it("lets the user pick another episode, mark an extra, or skip, and filters the list", async () => {
    await toReview();
    await userEvent.click(screen.getByRole("option", { name: /title_t11\.mkv/ }));
    const pick = screen.getByLabelText("Suggested");
    await userEvent.selectOptions(pick, "S02E02 · Elementary, My Dear (53%)");
    expect(screen.getByText(/Also chosen for title_t02\.mkv/)).toBeInTheDocument();
    await userEvent.selectOptions(pick, "Skip this file");
    expect(screen.getByText(/1 skipped/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("option", { name: /title_t08\.mkv/ }));
    await userEvent.click(screen.getByRole("button", { name: "Not an episode" }));
    expect(screen.getByText(/9 approved · 1 to check · 3 extras · 1 skipped/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("radio", { name: "Extras 3" }));
    expect(within(screen.getByRole("listbox", { name: "Files" })).getAllByRole("option")).toHaveLength(3);
  });

  it("asks for approval after another episode is picked, keeping the file in view", async () => {
    await toReview();
    await userEvent.click(screen.getByRole("radio", { name: "Check 3" }));
    await userEvent.click(screen.getByRole("option", { name: /title_t08\.mkv/ }));
    const pick = screen.getByLabelText("Suggested");
    await userEvent.selectOptions(pick, within(pick).getAllByRole("option")[1]!);
    // Still listed and selected, still to check, and nothing was approved.
    expect(screen.getByRole("option", { name: /title_t08\.mkv/ })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText(/9 approved · 3 to check/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Approve" }));
    expect(screen.getByText(/10 approved · 2 to check/)).toBeInTheDocument();
    // An approval can be taken back.
    await userEvent.click(screen.getByRole("radio", { name: "All 15" }));
    await userEvent.click(screen.getByRole("option", { name: /title_t08\.mkv/ }));
    await userEvent.click(screen.getByRole("button", { name: "Check again" }));
    expect(screen.getByText(/9 approved · 3 to check/)).toBeInTheDocument();
  });

  it("explains the play-all instead of offering episodes", async () => {
    await toReview();
    await userEvent.click(screen.getByRole("option", { name: /title_t00\.mkv/ }));
    expect(screen.getByText(/This is the play-all title/)).toBeInTheDocument();
    expect(screen.queryByLabelText("Suggested")).not.toBeInTheDocument();
  });

  it("opens the selected file for playback", async () => {
    const backend = createMockBackend({ stepMs: 1, modelReady: true });
    const openFile = vi.spyOn(backend, "openFile");
    renderApp(backend);
    await toConfirmScreen();
    const identify = await screen.findByRole("button", { name: "Identify 14 files" });
    await waitFor(() => expect(identify).toBeEnabled());
    await userEvent.click(identify);
    await screen.findByRole("heading", { name: /^Review/ }, { timeout: 3000 });
    await userEvent.click(screen.getByRole("button", { name: "▶ Play" }));
    expect(openFile).toHaveBeenCalledWith("/Volumes/Rips/SCHOOLHOUSE_ROCK_D1/title_t01.mkv");

    // A file that cannot be opened says so.
    openFile.mockRejectedValueOnce(new Error("forbidden path"));
    await userEvent.click(screen.getByRole("button", { name: "▶ Play" }));
    expect(await screen.findByText(/Couldn't open title_t01\.mkv/)).toBeInTheDocument();
  });
});

describe("Rename and History", () => {
  it("previews Jellyfin names, leaves the play-all and extras alone, renames, and undoes from History", async () => {
    await toReview();
    await userEvent.click(screen.getByRole("button", { name: "Continue to rename →" }));
    expect(screen.getByRole("radio", { name: /Rename in place/ })).toBeChecked();
    const preview = await screen.findByRole("list", { name: "New names" });
    expect(preview).toHaveTextContent("Schoolhouse Rock! (1973)/");
    expect(preview).toHaveTextContent("Schoolhouse Rock! (1973) - S04E01 - Conjunction Junction.mkv ← title_t03.mkv");
    expect(screen.getByText(/Not renamed: title_t00\.mkv \(play all\), 2 extras \(title_t44\.mkv, title_t45\.mkv\) and 3 skipped or unchecked files stay as they are\./)).toBeInTheDocument();

    await userEvent.selectOptions(screen.getByLabelText("Naming"), "Kodi");
    await waitFor(() => expect(preview).toHaveTextContent("Schoolhouse Rock! S04E01 - Conjunction Junction.mkv"));

    await userEvent.click(screen.getByRole("button", { name: "Rename 9 files" }));
    expect(await screen.findByRole("heading", { name: "Renamed 9 files" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open History" }));

    const entry = (await screen.findAllByRole("listitem")).find((li) => li.textContent?.includes("Renamed 9 files"))!;
    await userEvent.click(within(entry).getByRole("button", { name: "Undo…" }));
    expect(within(entry).getByText("Give these 9 files their original names back?")).toBeInTheDocument();
    expect(within(entry).getByRole("button", { name: "Undo rename" })).toHaveFocus();
    await userEvent.click(within(entry).getByRole("button", { name: "Cancel" }));
    expect(within(entry).getByRole("button", { name: "Undo…" })).toHaveFocus();
    await userEvent.click(within(entry).getByRole("button", { name: "Undo…" }));
    await userEvent.click(within(entry).getByRole("button", { name: "Undo rename" }));
    expect(await screen.findByText("Restored 9 files.")).toBeInTheDocument();
    expect(screen.getAllByText(/^Undone/).length).toBeGreaterThan(0);
  });

  it("copies into a chosen folder, and exports a CSV without a History entry", async () => {
    await toReview();
    await userEvent.click(screen.getByRole("button", { name: "Continue to rename →" }));
    await userEvent.click(screen.getByRole("radio", { name: /Copy into a new folder/ }));
    expect(screen.getByText("Choose where to save.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Choose: Copy into" }));
    expect(await screen.findByRole("button", { name: "Copy 9 files" })).toBeEnabled();

    await userEvent.click(screen.getByRole("radio", { name: /Only export a list/ }));
    expect(screen.getByLabelText("Save list as")).toHaveValue("/Volumes/Rips/SCHOOLHOUSE_ROCK_D1/Schoolhouse Rock! episodes.csv");
    expect(screen.queryByLabelText(/Also save subtitles/)).not.toBeInTheDocument();
    await userEvent.click(await screen.findByRole("button", { name: "Export list of 9 files" }));
    expect(await screen.findByRole("heading", { name: "Exported a list of 9 files" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open History" })).not.toBeInTheDocument();
  });

  it("keeps the result of a save when the user leaves and comes back", async () => {
    await toReview();
    await userEvent.click(screen.getByRole("button", { name: "Continue to rename →" }));
    await userEvent.click(await screen.findByRole("button", { name: "Rename 9 files" }));
    expect(await screen.findByRole("heading", { name: "Renamed 9 files" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open History" }));
    await userEvent.click(screen.getByRole("button", { name: "Identify" }));
    expect(await screen.findByRole("heading", { name: "Renamed 9 files" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Rename 9 files" })).not.toBeInTheDocument();
  });

  it("does not save while the preview for changed settings is still being built", async () => {
    const backend = createMockBackend({ stepMs: 1, downloadStepMs: 1, modelReady: true });
    const plan = backend.planRename;
    let release: (() => void) | null = null;
    let calls = 0;
    backend.planRename = async (request) => {
      calls += 1;
      if (calls > 1) await new Promise<void>((resolve) => (release = resolve));
      return plan(request);
    };
    const apply = vi.spyOn(backend, "applyRename");
    renderApp(backend);
    await toConfirmScreen();
    const identify = await screen.findByRole("button", { name: "Identify 14 files" });
    await waitFor(() => expect(identify).toBeEnabled());
    await userEvent.click(identify);
    await screen.findByRole("heading", { name: /^Review/ }, { timeout: 3000 });
    await userEvent.click(screen.getByRole("button", { name: "Continue to rename →" }));
    expect(await screen.findByRole("button", { name: "Rename 9 files" })).toBeEnabled();
    await userEvent.selectOptions(screen.getByLabelText("Naming"), "Kodi");
    const updating = screen.getByRole("button", { name: "Updating preview…" });
    expect(updating).toBeDisabled();
    await userEvent.click(updating);
    expect(apply).not.toHaveBeenCalled();
    await waitFor(() => expect(release).not.toBeNull());
    await act(async () => release!());
    expect(await screen.findByRole("button", { name: "Rename 9 files" })).toBeEnabled();
  });

  it("replaces an existing list only when it was chosen in the save dialog", async () => {
    const backend = createMockBackend({ stepMs: 1, downloadStepMs: 1, modelReady: true });
    const plan = vi.spyOn(backend, "planRename");
    renderApp(backend);
    await toConfirmScreen();
    const identify = await screen.findByRole("button", { name: "Identify 14 files" });
    await waitFor(() => expect(identify).toBeEnabled());
    await userEvent.click(identify);
    await screen.findByRole("heading", { name: /^Review/ }, { timeout: 3000 });
    await userEvent.click(screen.getByRole("button", { name: "Continue to rename →" }));
    await userEvent.click(screen.getByRole("radio", { name: /Only export a list/ }));
    const lastMode = () => plan.mock.calls.at(-1)?.[0].mode;
    await waitFor(() => expect(lastMode()).toMatchObject({ kind: "exportList", replace: false }));
    await userEvent.click(screen.getByRole("button", { name: "Choose: Save list as" }));
    await waitFor(() => expect(lastMode()).toMatchObject({ kind: "exportList", replace: true }));
    await userEvent.type(screen.getByLabelText("Save list as"), "x");
    await waitFor(() => expect(lastMode()).toMatchObject({ kind: "exportList", replace: false }));
  });

  it("blocks saving while two files would get the same name", async () => {
    await toReview();
    await userEvent.click(screen.getByRole("option", { name: /title_t11\.mkv/ }));
    await userEvent.selectOptions(screen.getByLabelText("Suggested"), "S02E02 · Elementary, My Dear (53%)");
    await userEvent.click(screen.getByRole("button", { name: "Approve" }));
    await userEvent.click(screen.getByRole("button", { name: "Continue to rename →" }));
    expect(await screen.findByText(/would both become/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Rename \d+ files$/ })).toBeDisabled();
  });
});

describe("Settings", () => {
  it("adds a SubDL key without ever showing it again", async () => {
    renderApp();
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByText("Episode lists (TVmaze)")).toBeInTheDocument();
    const addKeys = screen.getAllByRole("button", { name: "Add key…" });
    expect(addKeys).toHaveLength(2);
    await userEvent.click(addKeys[0]!);
    await userEvent.type(screen.getByLabelText("SubDL API key"), "secret-123");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("button", { name: "Change key…" })).toBeInTheDocument();
    expect(screen.queryByDisplayValue("secret-123")).not.toBeInTheDocument();
    expect(document.body.textContent).not.toContain("secret-123");
  });

  it("switches the speech model and the sampling option", async () => {
    renderApp();
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    const fast = await screen.findByRole("radio", { name: "Fast · 190 MB" });
    await userEvent.click(fast);
    expect(fast).toHaveAttribute("aria-checked", "true");
    expect(await screen.findByText("Not downloaded yet.")).toBeInTheDocument();
    const sample = screen.getByRole("checkbox", { name: /Listen to a sample/ });
    await userEvent.click(sample);
    expect(sample).not.toBeChecked();
  });

  it("says plainly when the update check fails, and when the app is up to date", async () => {
    renderApp(createMockBackend({ update: "failed", modelReady: true }));
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(screen.getByText("When a new version is found, you're asked before anything downloads.")).toBeInTheDocument();
    await userEvent.click(await screen.findByRole("button", { name: "Check now" }));
    expect(await screen.findByText("Couldn't check for updates.")).toBeInTheDocument();
    expect(screen.getByText(/· checked today/)).toBeInTheDocument();
  });

  it("reports up to date", async () => {
    renderApp(createMockBackend({ update: "upToDate", modelReady: true }));
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    await userEvent.click(await screen.findByRole("button", { name: "Check now" }));
    expect(await screen.findByText("Media Identifier is up to date.")).toBeInTheDocument();
  });
});

describe("Updates", () => {
  it("asks before downloading, shows progress, then offers Relaunch now", async () => {
    const backend = createMockBackend({ stepMs: 1, modelReady: true });
    const download = vi.spyOn(backend, "downloadUpdate");
    const relaunch = vi.spyOn(backend, "installUpdateAndRelaunch");
    renderApp(backend);
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    await userEvent.click(await screen.findByRole("button", { name: "Check now" }));
    const dialog = await screen.findByRole("dialog", { name: "A new version of Media Identifier is available" });
    expect(within(dialog).getByText("Version 1.3.0 is available. You have 1.2.1.")).toBeInTheDocument();
    expect(within(dialog).getByText("Kodi naming.")).toBeInTheDocument();
    expect(download).not.toHaveBeenCalled();
    await userEvent.click(within(dialog).getByRole("button", { name: "Install update" }));
    expect(download).toHaveBeenCalledOnce();
    expect(await screen.findByText(/Update downloaded\./)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Relaunch now" }));
    expect(relaunch).toHaveBeenCalledOnce();
  });

  it("cancels an update download and keeps nothing to install", async () => {
    const backend = createMockBackend({ modelReady: true, stepMs: 200 });
    const cancel = vi.spyOn(backend, "cancelUpdateDownload");
    renderApp(backend);
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    await userEvent.click(await screen.findByRole("button", { name: "Check now" }));
    await userEvent.click(await screen.findByRole("button", { name: "Install update" }));
    const progress = await screen.findByRole("dialog", { name: /Downloading Media Identifier 1\.3\.0/ });
    await userEvent.click(within(progress).getByRole("button", { name: "Cancel" }));
    expect(cancel).toHaveBeenCalledOnce();
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await new Promise((r) => setTimeout(r, 1400));
    expect(screen.queryByText(/Update downloaded\./)).not.toBeInTheDocument();
  }, 10_000);

  it("keeps a hidden download out of the way: Check now shows its progress, a failure goes to the banner", async () => {
    const backend = createMockBackend({ modelReady: true, stepMs: 200 });
    let fail: ((e: unknown) => void) | null = null;
    backend.downloadUpdate = () =>
      new Promise<void>((_, reject) => {
        fail = reject;
      });
    renderApp(backend);
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    await userEvent.click(await screen.findByRole("button", { name: "Check now" }));
    await userEvent.click(await screen.findByRole("button", { name: "Install update" }));
    let progress = await screen.findByRole("dialog", { name: /Downloading Media Identifier 1\.3\.0/ });
    await userEvent.click(within(progress).getByRole("button", { name: "Hide" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Check now" }));
    progress = await screen.findByRole("dialog", { name: /Downloading Media Identifier 1\.3\.0/ });
    expect(screen.queryByRole("dialog", { name: /A new version/ })).not.toBeInTheDocument();
    expect(screen.getByText("Version 1.3.0 is downloading.")).toBeInTheDocument();
    await userEvent.click(within(progress).getByRole("button", { name: "Hide" }));

    await act(async () => fail!({ code: "network", message: "the connection dropped" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByText(/the connection dropped/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Details…" }));
    const dialog = await screen.findByRole("dialog", { name: /A new version/ });
    expect(within(dialog).getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });

  it("skips a version", async () => {
    const backend = createMockBackend({ modelReady: true });
    const skip = vi.spyOn(backend, "skipUpdateVersion");
    renderApp(backend);
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    await userEvent.click(await screen.findByRole("button", { name: "Check now" }));
    await userEvent.click(await screen.findByRole("button", { name: "Skip this version" }));
    expect(skip).toHaveBeenCalledWith("1.3.0");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("holds a launch-time announcement until identification ends", async () => {
    const m = manualBackend();
    renderApp(m.backend);
    await toConfirmScreen();
    const identify = await screen.findByRole("button", { name: "Identify 14 files" });
    await waitFor(() => expect(identify).toBeEnabled());
    await userEvent.click(identify);
    m.emit({ kind: "started", jobId: "job-1", fileIds: ["title_t11.mkv"], accelerator: "cpu" });
    m.announce({ version: "1.3.0", currentVersion: "1.2.1", notes: "", date: null });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    m.emit({ kind: "finished", jobId: "job-1" });
    expect(await screen.findByRole("dialog", { name: "A new version of Media Identifier is available" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Remind me later" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("disables Relaunch now while identification runs", async () => {
    const m = manualBackend();
    renderApp(m.backend);
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    await userEvent.click(await screen.findByRole("button", { name: "Check now" }));
    await userEvent.click(await screen.findByRole("button", { name: "Install update" }));
    expect(await screen.findByText(/Update downloaded\./)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Identify" }));
    await toConfirmScreen();
    const identify = await screen.findByRole("button", { name: "Identify 14 files" });
    await waitFor(() => expect(identify).toBeEnabled());
    await userEvent.click(identify);
    m.emit({ kind: "started", jobId: "job-1", fileIds: ["title_t11.mkv"], accelerator: "cpu" });
    expect(screen.getByRole("button", { name: "Relaunch now" })).toBeDisabled();
    expect(screen.getByText(/the update waits until it finishes/)).toBeInTheDocument();
  });
});
