// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import themeInit from "../../public/theme-init.js?raw";
import { applyTheme, nextTheme, resolveTheme, THEME_STORAGE_KEY, useSystemDark } from "./theme";

/** Runs the real pre-paint script the way the page does: as a classic script. */
function runThemeInit() {
  new Function(themeInit)();
}

/** A stand-in for the desktop's appearance setting that tests can flip. */
function installSystemScheme(initiallyDark: boolean) {
  let dark = initiallyDark;
  const listeners = new Set<() => void>();
  const query = {
    get matches() {
      return dark;
    },
    addEventListener: (_: string, listener: () => void) => listeners.add(listener),
    removeEventListener: (_: string, listener: () => void) => listeners.delete(listener),
  };
  vi.stubGlobal("matchMedia", () => query);
  return {
    set(next: boolean) {
      dark = next;
      listeners.forEach((listener) => listener());
    },
    listeners,
  };
}

beforeEach(() => {
  window.localStorage.clear();
  delete document.documentElement.dataset.theme;
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("resolving a preference", () => {
  it("follows the desktop only when set to System", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });

  it("cycles System → Light → Dark and back", () => {
    expect(nextTheme("system")).toBe("light");
    expect(nextTheme("light")).toBe("dark");
    expect(nextTheme("dark")).toBe("system");
  });
});

describe("applying a preference", () => {
  it("puts it where the stylesheet reads it", () => {
    applyTheme("dark", false);
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("is what the pre-paint script restores on the next launch", () => {
    applyTheme("dark", false);
    // A new launch: the markup says "system" until the script has run.
    document.documentElement.dataset.theme = "system";
    runThemeInit();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("leaves the markup's System alone when nothing usable is remembered", () => {
    document.documentElement.dataset.theme = "system";
    runThemeInit();
    expect(document.documentElement.dataset.theme).toBe("system");

    window.localStorage.setItem(THEME_STORAGE_KEY, "sepia");
    runThemeInit();
    expect(document.documentElement.dataset.theme).toBe("system");
  });

  it("keeps the theme-color meta in step with what is on screen", () => {
    const meta = document.createElement("meta");
    meta.name = "theme-color";
    document.head.append(meta);
    applyTheme("system", true);
    expect(meta.content).toBe("#181512");
    applyTheme("light", true);
    expect(meta.content).toBe("#f2eee7");
    meta.remove();
  });
});

describe("useSystemDark", () => {
  it("tracks the desktop switching between light and dark while the app is open", () => {
    const system = installSystemScheme(false);
    const { result, unmount } = renderHook(() => useSystemDark());
    expect(result.current).toBe(false);

    act(() => system.set(true));
    expect(result.current).toBe(true);
    act(() => system.set(false));
    expect(result.current).toBe(false);

    unmount();
    expect(system.listeners.size).toBe(0);
  });

  it("reads as light where the webview cannot say", () => {
    vi.stubGlobal("matchMedia", undefined);
    expect(renderHook(() => useSystemDark()).result.current).toBe(false);
  });
});
