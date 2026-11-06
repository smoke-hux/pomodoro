import { useEffect, useState } from "react";
import type { ThemePreference } from "../types";

export type ResolvedTheme = "light" | "dark";

/** Read back by public/theme-init.js, which cannot import it. */
export const THEME_STORAGE_KEY = "pomodoro.theme";
const DARK_QUERY = "(prefers-color-scheme: dark)";
/** `--canvas` in each theme, for the `theme-color` meta tag. */
const CANVAS: Record<ResolvedTheme, string> = { light: "#fafafa", dark: "#242424" };

const ORDER: ThemePreference[] = ["system", "light", "dark"];

/**
 * The preference applyTheme last remembered, or "system". For the browser
 * preview, which has no backend to hold the setting: without it every reload
 * applied the default over the choice theme-init.js had just painted.
 */
export function readRememberedTheme(): ThemePreference {
  try {
    const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
    return stored === "light" || stored === "dark" ? stored : "system";
  } catch {
    return "system";
  }
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
 * Puts a preference on the root element, where the stylesheet reads it, and
 * remembers it for the next launch. The real setting lives in the backend
 * store and arrives with the first snapshot, which is after the first paint;
 * public/theme-init.js reads this local copy back before anything is drawn.
 * For System, the stylesheet uses the native desktop preference supplied by
 * useDesktopAppearance, with `prefers-color-scheme` as the browser fallback.
 */
export function applyTheme(preference: ThemePreference, systemDark: boolean) {
  document.documentElement.dataset.theme = preference;
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute("content", CANVAS[resolveTheme(preference, systemDark)]);
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, preference);
  } catch {
    // Remembering is a convenience; the theme itself is already applied.
  }
}

function systemPrefersDark(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia(DARK_QUERY).matches;
}

/**
 * The WebView/browser preference, kept live. The app uses this when the native
 * desktop portal has no color-scheme preference or cannot be reached.
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
