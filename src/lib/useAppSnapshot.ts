import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "./api";
import { defaultSnapshot, type AppSnapshot } from "../types";

const RETRY_DELAY_MS = 3_000;

/** Owns the desktop connection, including registration order and retry cleanup. */
export function useAppSnapshot(
  inTauri: boolean,
  preview: () => AppSnapshot,
  showNotice: (message: string) => void,
) {
  const [snapshot, setSnapshot] = useState(() => (inTauri ? defaultSnapshot : preview()));
  const [ready, setReady] = useState(!inTauri);
  const [settingsLoaded, setSettingsLoaded] = useState(!inTauri);

  useEffect(() => {
    if (!inTauri) return;
    let cancelled = false;
    let stop: UnlistenFn | undefined;
    let retry: number | undefined;

    async function connect() {
      let receivedBroadcast = false;
      try {
        // Subscribe before reading: otherwise a change can fall into the gap
        // between the initial snapshot and registration and never reach the UI.
        const unlisten = await listen<AppSnapshot>("state-changed", (event) => {
          if (cancelled) return;
          receivedBroadcast = true;
          setSnapshot(event.payload);
          setSettingsLoaded(true);
          setReady(true);
        });
        if (cancelled) {
          unlisten();
          return;
        }
        stop = unlisten;
        const next = await api.snapshot();
        if (cancelled) return;
        // A broadcast is newer than a read that was already in flight.
        if (!receivedBroadcast) setSnapshot(next);
        setSettingsLoaded(true);
        setReady(true);
      } catch {
        if (cancelled || receivedBroadcast) return;
        stop?.();
        stop = undefined;
        setReady(true);
        showNotice("Pomodoro could not connect to its local data. Retrying…");
        // Keep retrying while mounted so a temporary backend failure can
        // recover without a restart. Each retry refreshes the notice. Only a
        // successful read or broadcast makes stored settings safe to edit.
        retry = window.setTimeout(() => void connect(), RETRY_DELAY_MS);
      }
    }

    void connect();
    return () => {
      cancelled = true;
      if (retry !== undefined) window.clearTimeout(retry);
      stop?.();
    };
  }, [inTauri, showNotice]);

  return { snapshot, setSnapshot, ready, settingsLoaded };
}
