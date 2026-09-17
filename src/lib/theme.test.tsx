// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyTheme, nextTheme, readCachedTheme, resolveTheme, useSystemDark } from "./theme";

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
  it("puts it where the stylesheet reads it and remembers it for the next launch", () => {
    applyTheme("dark", false);
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(readCachedTheme()).toBe("dark");
  });

  it("falls back to System when nothing usable is remembered", () => {
    expect(readCachedTheme()).toBe("system");
    window.localStorage.setItem("pomodoro.theme", "sepia");
    expect(readCachedTheme()).toBe("system");
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
