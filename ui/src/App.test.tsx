import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { BackendContext, createMockBackend } from "./api";
import { App } from "./App";

function renderApp() {
  return render(
    <BackendContext.Provider value={createMockBackend()}>
      <App />
    </BackendContext.Provider>,
  );
}

describe("App", () => {
  it("lists recent jobs from the backend", async () => {
    renderApp();
    expect(await screen.findByText(/Married\.\.\. with Children · 22 files · 2 to review/)).toBeInTheDocument();
  });

  it("switches sections from the navigation", async () => {
    renderApp();
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Settings" })).toHaveAttribute("aria-current", "page");
  });
});
