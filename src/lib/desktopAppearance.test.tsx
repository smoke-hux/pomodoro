// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

import {
  accessibleAccent,
  contrastRatio,
  useDesktopAppearance,
  type DesktopAppearance,
} from "./desktopAppearance";

const desktop: DesktopAppearance = {
  colorScheme: "dark",
  accentColor: "#e62d42",
  highContrast: true,
  reducedMotion: true,
};
let broadcast: (event: { payload: DesktopAppearance }) => void;
const stop = vi.fn();

beforeEach(() => {
  invoke.mockReset().mockResolvedValue(desktop);
  listen.mockReset().mockImplementation((_name, callback) => {
    broadcast = callback;
    return Promise.resolve(stop);
  });
  stop.mockReset();
});
afterEach(cleanup);

describe("desktop accents", () => {
  it.each(["#ffffff", "#000000", "#ffff00", "#e62d42", "#3584e4", "#9141ac"])(
    "keeps %s readable on selected surfaces and action buttons",
    (color) => {
      const light = accessibleAccent(color, false)!;
      const dark = accessibleAccent(color, true)!;
      expect(contrastRatio(light, "#ebebeb")).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(light, "#ffffff")).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(dark, "#444444")).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(dark, "#1e1e1e")).toBeGreaterThanOrEqual(4.5);
    },
  );

  it("rejects malformed colors and leaves an already readable accent alone", () => {
    expect(accessibleAccent("var(--text)", false)).toBeNull();
    expect(accessibleAccent("#fff", false)).toBeNull();
    expect(accessibleAccent("#b83b0e", false)).toBe("#b83b0e");
    expect(accessibleAccent("#ff9866", true)).toBe("#ff9866");
  });
});

describe("native appearance subscription", () => {
  it("applies live preferences and removes obsolete overrides", async () => {
    const { result, unmount } = renderHook(() => useDesktopAppearance(true));
    await waitFor(() => expect(result.current).toEqual(desktop));
    const root = document.documentElement;
    expect(root.dataset.systemTheme).toBe("dark");
    expect(root.dataset.highContrast).toBe("true");
    expect(root.dataset.reducedMotion).toBe("true");
    expect(root.style.getPropertyValue("--desktop-accent-dark")).toBeTruthy();

    act(() => {
      broadcast({
        payload: {
          colorScheme: null,
          accentColor: null,
          highContrast: false,
          reducedMotion: false,
        },
      });
    });
    expect(root.dataset.systemTheme).toBeUndefined();
    expect(root.dataset.highContrast).toBeUndefined();
    expect(root.dataset.reducedMotion).toBeUndefined();
    expect(root.style.getPropertyValue("--desktop-accent-dark")).toBe("");
    unmount();
    expect(stop).toHaveBeenCalledOnce();
  });

  it("never overwrites a newer event with a late initial read", async () => {
    let finish!: (value: DesktopAppearance) => void;
    invoke.mockReturnValue(new Promise((resolve) => (finish = resolve)));
    const { result } = renderHook(() => useDesktopAppearance(true));
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    act(() => broadcast({ payload: { ...desktop, colorScheme: "light" } }));
    await act(async () => finish(desktop));
    expect(result.current?.colorScheme).toBe("light");
  });

  it("cleans up a listener which finishes registering after unmount", async () => {
    let finish!: (value: typeof stop) => void;
    listen.mockReturnValue(new Promise((resolve) => (finish = resolve)));
    const { unmount } = renderHook(() => useDesktopAppearance(true));
    unmount();
    await act(async () => finish(stop));
    expect(stop).toHaveBeenCalledOnce();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("still reads preferences when event registration fails", async () => {
    listen.mockRejectedValue(new Error("events unavailable"));
    const { result } = renderHook(() => useDesktopAppearance(true));
    await waitFor(() => expect(result.current).toEqual(desktop));
  });

  it("falls back quietly when the native command is unavailable", async () => {
    invoke.mockRejectedValue(new Error("desktop unavailable"));
    const { result } = renderHook(() => useDesktopAppearance(true));
    await act(async () => {});
    expect(result.current).toBeNull();
    expect(document.documentElement.dataset.systemTheme).toBeUndefined();
  });

  it("does not connect to desktop services in browser preview", () => {
    expect(renderHook(() => useDesktopAppearance(false)).result.current).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
    expect(listen).not.toHaveBeenCalled();
  });
});
