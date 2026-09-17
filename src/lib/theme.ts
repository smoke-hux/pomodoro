import { useEffect, useState } from "react";
import type { ThemePreference } from "../types";

export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "pomodoro.theme";
const DARK_QUERY = "(prefers-color-scheme: dark)";
/** `--canvas` in each theme, for the `theme-color` meta tag. */
const CANVAS: Record<ResolvedTheme, string> = { light: "#f2eee7", dark: "#181512" };

const ORDER: ThemePreference[] = ["system", "light", "dark"];

function isPreference(value: unknown): value is ThemePreference {
  return value === "system" || value === "light" || value === "dark";
}

/** The preference after `current` in the toolbar button's System → Light → Dark cycle. */
export function nextTheme(current: ThemePreference): ThemePreference {
  return ORDER[(ORDER.indexOf(current) + 1) % ORDER.length];
}

/** What a preference comes out as, given what the desktop is asking for. */
export function resolveTheme(preference: ThemePreference, systemDark: boolean): ResolvedTheme {
  if (preference === "system") return systemDark ? "dark" : "light";
  return preference;
}

/**
 * The preference this window last applied.
 *
 * The real setting lives in the backend store and arrives with the first
 * snapshot, which is after the first paint. Without a local copy a user who
 * chose Dark on a light desktop (or the reverse) sees the wrong theme flash on
 * every launch. Storage can be unavailable; the answer is then "system", which
 * the stylesheet resolves by itself.
 */
export function readCachedTheme(): ThemePreference {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    return isPreference(stored) ? stored : "system";
  } catch {
    return "system";
  }
}

/**
 * Puts a preference on the root element, where the stylesheet reads it, and
 * remembers it for the next launch. "system" is resolved by the stylesheet's
 * own `prefers-color-scheme` block, so a desktop switching between light and
 * dark repaints the window without any script running.
 */
export function applyTheme(preference: ThemePreference, systemDark: boolean) {
  document.documentElement.dataset.theme = preference;
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute("content", CANVAS[resolveTheme(preference, systemDark)]);
  try {
    window.localStorage.setItem(STORAGE_KEY, preference);
  } catch {
    // Remembering is a convenience; the theme itself is already applied.
  }
}

function systemPrefersDark(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia(DARK_QUERY).matches;
}

/**
 * Whether the desktop is currently asking for dark, kept live.
 *
 * On Linux the webview takes this from the desktop's appearance setting (the
 * XDG settings portal, which GNOME's Dark style switch writes), so it changes
 * while the app is open — at sunset, if the desktop is scheduled to.
 */
export function useSystemDark(): boolean {
  const [dark, setDark] = useState(systemPrefersDark);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const query = window.matchMedia(DARK_QUERY);
    const onChange = () => setDark(query.matches);
    onChange();
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  return dark;
}
