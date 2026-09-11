import { useEffect, useState } from "react";
import type { TimerState } from "../types";

/**
 * The seconds left on a timer, kept current locally.
 *
 * The backend hands the window an absolute deadline (`endsAt`) and it is on the
 * same machine with the same clock, so the window can count down by itself.
 * Before this the window asked the backend every second, and the backend
 * answered by cloning and serialising the entire store — tasks, history, every
 * captured notification body — to deliver one changed number.
 *
 * Ticks are aligned to the deadline's second boundaries, so the display never
 * skips or holds a digit, and nothing runs at all unless the timer is running.
 * Reaching zero is not a phase change: the backend owns that, and announces it
 * on `state-changed` like any other change.
 */
export function useCountdown(timer: TimerState, now: () => number = Date.now): number {
  const { status, endsAt, remainingSeconds } = timer;
  const running = status === "running" && endsAt !== null;

  const [remaining, setRemaining] = useState(() =>
    running ? secondsUntil(endsAt, now()) : remainingSeconds,
  );

  useEffect(() => {
    if (!running) {
      setRemaining(remainingSeconds);
      return;
    }
    let timeout: number | null = null;
    const tick = () => {
      const current = now();
      setRemaining(secondsUntil(endsAt, current));
      if (current >= endsAt) return;
      // Wake on the next whole-second boundary of the deadline, not on a
      // free-running interval that drifts against it. Exactly on a boundary
      // the next one is a full second away, not zero.
      const untilNextBoundary = (endsAt - current) % 1_000 || 1_000;
      timeout = window.setTimeout(tick, untilNextBoundary);
    };
    tick();
    return () => {
      if (timeout !== null) window.clearTimeout(timeout);
    };
  }, [running, endsAt, remainingSeconds, now]);

  return remaining;
}

/** Whole seconds from `nowMs` to `endsAt`, never negative. */
export function secondsUntil(endsAt: number, nowMs: number): number {
  return Math.max(0, Math.ceil((endsAt - nowMs) / 1_000));
}
