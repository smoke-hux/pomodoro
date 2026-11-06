import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface DesktopAppearance {
  colorScheme: "light" | "dark" | null;
  accentColor: string | null;
  highContrast: boolean;
  reducedMotion: boolean;
}

type RGB = [number, number, number];

function rgb(hex: string): RGB {
  return [1, 3, 5].map((offset) => parseInt(hex.slice(offset, offset + 2), 16)) as RGB;
}

function luminance(channels: RGB): number {
  const linear = channels.map((channel) => {
    const value = channel / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
}

export function contrastRatio(first: string, second: string): number {
  const a = luminance(rgb(first));
  const b = luminance(rgb(second));
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

/** Keep the desktop hue, adjusting brightness for readable text and buttons. */
export function accessibleAccent(color: string, dark: boolean): string | null {
  if (!/^#[\da-f]{6}$/i.test(color)) return null;
  const channels = rgb(color);
  const target = dark ? 255 : 0;
  for (let step = 0; step <= 100; step++) {
    const candidate =
      "#" +
      channels
        .map((channel) =>
          Math.round(channel + (target - channel) * (step / 100))
            .toString(16)
            .padStart(2, "0"),
        )
        .join("");
    // Test the least favorable regular surface, which also guarantees the
    // button foreground (white in light, #1e1e1e in dark) remains readable.
    if (contrastRatio(candidate, dark ? "#444444" : "#ebebeb") >= 4.5) return candidate;
  }
  return null;
}

function applyAppearance(appearance: DesktopAppearance | null) {
  const root = document.documentElement;
  if (appearance?.colorScheme === "dark" || appearance?.colorScheme === "light") {
    root.dataset.systemTheme = appearance.colorScheme;
  } else {
    delete root.dataset.systemTheme;
  }
  for (const dark of [false, true]) {
    const key = `--desktop-accent-${dark ? "dark" : "light"}`;
    const color = appearance?.accentColor && accessibleAccent(appearance.accentColor, dark);
    if (color) root.style.setProperty(key, color);
    else root.style.removeProperty(key);
  }
  if (appearance?.highContrast) root.dataset.highContrast = "true";
  else delete root.dataset.highContrast;
  if (appearance?.reducedMotion) root.dataset.reducedMotion = "true";
  else delete root.dataset.reducedMotion;
}

/** A single subscription; unavailable desktop services leave browser defaults intact. */
export function useDesktopAppearance(inTauri: boolean): DesktopAppearance | null {
  const [appearance, setAppearance] = useState<DesktopAppearance | null>(null);

  useEffect(() => {
    if (!inTauri) return;
    let cancelled = false;
    let stop: UnlistenFn | undefined;
    let receivedEvent = false;
    async function connect() {
      try {
        const unlisten = await listen<DesktopAppearance>("desktop-appearance-changed", (event) => {
          if (cancelled) return;
          receivedEvent = true;
          setAppearance(event.payload);
        });
        if (cancelled) {
          unlisten();
          return;
        }
        stop = unlisten;
      } catch {
        // A one-time read can still apply the desktop style without events.
      }
      if (cancelled) return;
      try {
        const initial = await invoke<DesktopAppearance>("get_desktop_appearance");
        if (!cancelled && !receivedEvent) setAppearance(initial ?? null);
      } catch {
        // Styling must never prevent the timer from opening or working.
      }
    }
    void connect();
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [inTauri]);

  useEffect(() => {
    applyAppearance(inTauri ? appearance : null);
    return () => applyAppearance(null);
  }, [inTauri, appearance]);

  return inTauri ? appearance : null;
}
