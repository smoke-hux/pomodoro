// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));

import App from "./App";

beforeEach(() => {
  invoke.mockReset();
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
});
afterEach(() => {
  cleanup();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe("when the stored settings could not be read", () => {
  // The window is then holding the defaults. Anything that saves settings
  // sends the whole object, so saving from here would reset every stored
  // setting — durations, sound, the notification filter — to its default.
  beforeEach(() => {
    invoke.mockImplementation((command: string) =>
      command === "get_snapshot" ? Promise.reject("unavailable") : Promise.resolve(),
    );
  });

  it("does not let the theme button save the defaults over them", async () => {
    render(<App />);
    const theme = (await screen.findByRole("button", { name: /^Theme:/ })) as HTMLButtonElement;
    expect(theme.disabled).toBe(true);

    fireEvent.click(theme);
    expect(invoke).not.toHaveBeenCalledWith("update_settings", expect.anything());
  });

  it("does not open the settings dialog over them", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Open settings" }));

    await waitFor(() => expect(screen.getByText(/Settings are not available/)).toBeTruthy());
    expect(invoke).not.toHaveBeenCalledWith("update_settings", expect.anything());
  });
});

describe("the theme button", () => {
  it("steps on from the theme already asked for when clicked again before the save returns", async () => {
    const { defaultSnapshot } = await import("./types");
    invoke.mockImplementation((command: string) =>
      command === "get_snapshot" ? Promise.resolve(defaultSnapshot) : new Promise(() => {}),
    );
    render(<App />);
    const theme = (await screen.findByRole("button", { name: /^Theme:/ })) as HTMLButtonElement;
    await waitFor(() => expect(theme.disabled).toBe(false));

    // No broadcast arrives between the clicks, as on a fast double click.
    fireEvent.click(theme);
    fireEvent.click(theme);

    const requested = invoke.mock.calls
      .filter(([command]) => command === "update_settings")
      .map(([, args]) => (args as { settings: { theme: string } }).settings.theme);
    expect(requested).toEqual(["light", "dark"]);
  });
});
