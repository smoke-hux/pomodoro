// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
let broadcast: (payload: unknown) => void = () => {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (_event: string, handler: (event: { payload: unknown }) => void) => {
    broadcast = (payload) => handler({ payload });
    return Promise.resolve(() => {});
  },
}));

import App from "./App";
import { defaultSnapshot } from "./types";
import type { AppSnapshot, SessionRecord } from "./types";

/** Enough of the Web Audio API for the chime to run to the end quietly. */
function quietAudioContext() {
  const param = { setValueAtTime: vi.fn(), exponentialRampToValueAtTime: vi.fn() };
  const node = () => ({
    type: "sine",
    frequency: { value: 0 },
    gain: param,
    connect: vi.fn(),
    start: vi.fn(),
    stop: vi.fn(),
    onended: null,
  });
  return {
    state: "running",
    currentTime: 0,
    destination: {},
    createOscillator: node,
    createGain: node,
    resume: vi.fn(),
    close: vi.fn(),
  };
}

function session(id: string, endedAt = Date.now() - 86_400_000): SessionRecord {
  return {
    id,
    phase: "focus",
    taskId: null,
    taskTitle: "Write the report",
    durationSeconds: 1_500,
    startedAt: Date.now() - 1_500_000,
    endedAt,
    outcome: "completed",
  };
}

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

describe("the theme button after a burst of clicks", () => {
  it("steps from the theme on screen, not from a request that is long over", async () => {
    invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "get_snapshot" ? defaultSnapshot : undefined),
    );
    render(<App />);
    const theme = (await screen.findByRole("button", { name: /^Theme:/ })) as HTMLButtonElement;
    await waitFor(() => expect(theme.disabled).toBe(false));

    // System → Light → Dark → System, the broadcasts landing as one: the theme
    // is never seen to change, so nothing but the saves themselves ends the request.
    fireEvent.click(theme);
    fireEvent.click(theme);
    fireEvent.click(theme);
    await act(async () => {});

    // Later the theme is changed elsewhere, in Settings.
    const dark: AppSnapshot = {
      ...defaultSnapshot,
      settings: { ...defaultSnapshot.settings, theme: "dark" },
    };
    await act(async () => broadcast(dark));
    invoke.mockClear();
    fireEvent.click(theme);

    expect(invoke).toHaveBeenCalledWith(
      "update_settings",
      expect.objectContaining({ settings: expect.objectContaining({ theme: "system" }) }),
    );
  });
});

describe("the completion chime when the first read failed", () => {
  it("treats the first broadcast as history and chimes for what completes after it", async () => {
    const AudioContext = vi.fn(() => quietAudioContext());
    vi.stubGlobal("AudioContext", AudioContext);
    invoke.mockImplementation((command: string) =>
      command === "get_snapshot" ? Promise.reject("unavailable") : Promise.resolve(),
    );
    try {
      render(<App />);
      await screen.findByRole("button", { name: "Open settings" });

      await act(async () => broadcast({ ...defaultSnapshot, sessions: [session("old")] }));
      expect(AudioContext).not.toHaveBeenCalled();

      await act(async () =>
        broadcast({ ...defaultSnapshot, sessions: [session("old"), session("new")] }),
      );
      expect(AudioContext).toHaveBeenCalledTimes(1);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("still chimes when that first broadcast is itself a session completing", async () => {
    const AudioContext = vi.fn(() => quietAudioContext());
    vi.stubGlobal("AudioContext", AudioContext);
    invoke.mockImplementation((command: string) =>
      command === "get_snapshot" ? Promise.reject("unavailable") : Promise.resolve(),
    );
    try {
      render(<App />);
      await screen.findByRole("button", { name: "Open settings" });

      await act(async () =>
        broadcast({
          ...defaultSnapshot,
          sessions: [session("old"), session("just-finished", Date.now() + 1_000)],
        }),
      );
      expect(AudioContext).toHaveBeenCalledTimes(1);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe("a row menu left open from the keyboard", () => {
  it("is closed when a shortcut opens a dialog over it", async () => {
    // The browser preview has tasks to open a menu on.
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    render(<App />);
    const menu = (await screen.findByLabelText(/More actions for Outline/)).closest("details")!;
    menu.open = true;

    fireEvent.keyDown(window, { key: "i", ctrlKey: true });
    expect(menu.open).toBe(false);
  });
});

describe("when the saved data could not be read", () => {
  it("says so, and says where the old file was kept", async () => {
    invoke.mockImplementation((command: string) =>
      Promise.resolve(
        command === "get_snapshot"
          ? { ...defaultSnapshot, recoveredStore: "pomodoro.unreadable-1234.json" }
          : undefined,
      ),
    );
    render(<App />);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("could not read its saved data");
    expect(alert.textContent).toContain("pomodoro.unreadable-1234.json");

    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("does not send the user to Quit when the old file could not be moved aside", async () => {
    // Quit saves. The notice used to recommend the one action that would
    // replace the file it was warning about; the backend now saves nothing on
    // this path, and the notice says so.
    invoke.mockImplementation((command: string) =>
      Promise.resolve(
        command === "get_snapshot" ? { ...defaultSnapshot, recoveredStore: "" } : undefined,
      ),
    );
    render(<App />);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("nothing is being saved");
    expect(alert.textContent).not.toMatch(/quit from the tray/i);
  });

  it("says nothing on a normal launch", async () => {
    invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "get_snapshot" ? defaultSnapshot : undefined),
    );
    render(<App />);
    await screen.findByRole("button", { name: "Open settings" });
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("the browser preview", () => {
  it("keeps the theme it remembered instead of applying the default over it", async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    window.localStorage.setItem("pomodoro.theme", "dark");
    document.documentElement.dataset.theme = "dark"; // as theme-init.js leaves it
    try {
      render(<App />);
      await screen.findByRole("button", { name: /^Theme: Dark/ });
      expect(document.documentElement.dataset.theme).toBe("dark");
      expect(window.localStorage.getItem("pomodoro.theme")).toBe("dark");
    } finally {
      window.localStorage.clear();
      delete document.documentElement.dataset.theme;
    }
  });
});
