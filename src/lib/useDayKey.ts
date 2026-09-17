import { useEffect, useState } from "react";
import { getLocalDateKey, getLocalDayBounds } from "./metrics";

/**
 * Today's local date as a key that changes at midnight.
 *
 * Pomodoro closes to the tray, so the window routinely stays alive overnight.
 * "Today" used to be worked out only when the snapshot changed, so the next
 * morning the toolbar and ledger still showed yesterday's sessions until
 * something else happened. Anything that shows "today" depends on this key.
 *
 * A timeout set for midnight can fire late after a suspend, so the date is also
 * rechecked whenever the window comes back into view.
 */
export function useDayKey(now: () => number = Date.now): string {
  const [dayKey, setDayKey] = useState(() => getLocalDateKey(now()));

  useEffect(() => {
    let timeout: number | null = null;
    const check = () => {
      const current = now();
      setDayKey(getLocalDateKey(current));
      if (timeout !== null) window.clearTimeout(timeout);
      timeout = window.setTimeout(check, getLocalDayBounds(current).end - current + 50);
    };
    check();
    const onVisible = () => {
      if (document.visibilityState === "visible") check();
    };
    document.addEventListener("visibilitychange", onVisible);
    window.addEventListener("focus", check);
    return () => {
      if (timeout !== null) window.clearTimeout(timeout);
      document.removeEventListener("visibilitychange", onVisible);
      window.removeEventListener("focus", check);
    };
  }, [now]);

  return dayKey;
}
