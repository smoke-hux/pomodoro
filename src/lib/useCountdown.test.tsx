// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { secondsUntil, useCountdown } from "./useCountdown";
import { defaultSnapshot } from "../types";
import type { TimerState } from "../types";

afterEach(cleanup);
beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

function timer(patch: Partial<TimerState>): TimerState {
  return { ...defaultSnapshot.timer, ...patch };
}

describe("secondsUntil", () => {
  it("rounds up so 24.2s left reads as 25, the way the backend does", () => {
    expect(secondsUntil(25_000, 800)).toBe(25);
    expect(secondsUntil(25_000, 0)).toBe(25);
    expect(secondsUntil(25_000, 24_999)).toBe(1);
  });

  it("never goes below zero once the deadline has passed", () => {
    expect(secondsUntil(1_000, 5_000)).toBe(0);
  });
});

describe("useCountdown", () => {
  it("shows the stored value and does not tick while idle or paused", () => {
    const idle = renderHook(() => useCountdown(timer({ status: "idle", remainingSeconds: 1_500 })));
    expect(idle.result.current).toBe(1_500);

    const paused = renderHook(() =>
      useCountdown(timer({ status: "paused", remainingSeconds: 731, endsAt: null })),
    );
    expect(paused.result.current).toBe(731);
    act(() => void vi.advanceTimersByTime(5_000));
    expect(paused.result.current).toBe(731);
  });

  it("counts down from the deadline by itself while running", () => {
    vi.setSystemTime(10_000);
    const running = timer({ status: "running", endsAt: 10_000 + 5_000, remainingSeconds: 5 });
    const { result } = renderHook(() => useCountdown(running));
    expect(result.current).toBe(5);

    act(() => void vi.advanceTimersByTime(1_000));
    expect(result.current).toBe(4);
    act(() => void vi.advanceTimersByTime(3_000));
    expect(result.current).toBe(1);
    act(() => void vi.advanceTimersByTime(1_000));
    expect(result.current).toBe(0);

    // At zero it stops scheduling itself and waits for the backend's word.
    act(() => void vi.advanceTimersByTime(60_000));
    expect(result.current).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("re-anchors when the deadline moves, as after a resume", () => {
    vi.setSystemTime(0);
    const first = timer({ status: "running", endsAt: 10_000, remainingSeconds: 10 });
    const { result, rerender } = renderHook(({ state }) => useCountdown(state), {
      initialProps: { state: first },
    });
    expect(result.current).toBe(10);

    act(() => void vi.advanceTimersByTime(2_000));
    expect(result.current).toBe(8);

    // Pause: the backend freezes the value and clears the deadline.
    rerender({ state: timer({ status: "paused", endsAt: null, remainingSeconds: 8 }) });
    act(() => void vi.advanceTimersByTime(30_000));
    expect(result.current).toBe(8);

    // Resume: a fresh deadline from the backend, counting from now.
    vi.setSystemTime(100_000);
    rerender({ state: timer({ status: "running", endsAt: 108_000, remainingSeconds: 8 }) });
    expect(result.current).toBe(8);
    act(() => void vi.advanceTimersByTime(1_000));
    expect(result.current).toBe(7);
  });

  it("corrects itself when the clock jumps, as after a suspend", () => {
    vi.setSystemTime(0);
    const { result } = renderHook(() =>
      useCountdown(timer({ status: "running", endsAt: 600_000, remainingSeconds: 600 })),
    );
    expect(result.current).toBe(600);

    // The laptop was asleep for nine minutes; the next tick sees the real time.
    vi.setSystemTime(540_000);
    act(() => void vi.advanceTimersByTime(1_000));
    expect(result.current).toBe(59);
  });

  it("refreshes immediately when the tray window regains focus", () => {
    vi.setSystemTime(0);
    const { result } = renderHook(() =>
      useCountdown(timer({ status: "running", endsAt: 600_000, remainingSeconds: 600 })),
    );

    // A suspended webview has not delivered its scheduled timeout yet.
    vi.setSystemTime(540_000);
    act(() => window.dispatchEvent(new Event("focus")));
    expect(result.current).toBe(60);
    expect(vi.getTimerCount()).toBe(1);
  });

  it("refreshes when visible again, but does not wake for a hidden window", () => {
    vi.setSystemTime(0);
    const visibility = vi.spyOn(document, "visibilityState", "get");
    try {
      const { result } = renderHook(() =>
        useCountdown(timer({ status: "running", endsAt: 600_000, remainingSeconds: 600 })),
      );
      vi.setSystemTime(540_000);
      visibility.mockReturnValue("hidden");
      act(() => document.dispatchEvent(new Event("visibilitychange")));
      expect(result.current).toBe(600);

      visibility.mockReturnValue("visible");
      act(() => document.dispatchEvent(new Event("visibilitychange")));
      expect(result.current).toBe(60);
      expect(vi.getTimerCount()).toBe(1);
    } finally {
      visibility.mockRestore();
    }
  });

  it("never exceeds the interval duration when the system clock moves backwards", () => {
    vi.setSystemTime(0);
    const { result } = renderHook(() =>
      useCountdown(
        timer({
          status: "running",
          endsAt: 60_000,
          remainingSeconds: 60,
          durationSeconds: 60,
        }),
      ),
    );
    vi.setSystemTime(-60_000);
    act(() => void vi.advanceTimersByTime(1_000));
    expect(result.current).toBe(60);
  });

  it("clamps the first render too when the clock is earlier than the saved start", () => {
    vi.setSystemTime(-60_000);
    const drawn: number[] = [];
    renderHook(() => {
      const remaining = useCountdown(
        timer({
          status: "running",
          endsAt: 60_000,
          remainingSeconds: 60,
          durationSeconds: 60,
        }),
      );
      drawn.push(remaining);
    });
    expect(new Set(drawn)).toEqual(new Set([60]));
  });

  it("keeps one scheduled tick after repeated focus events and cleans up on unmount", () => {
    vi.setSystemTime(0);
    const now = vi.fn(() => Date.now());
    const { unmount } = renderHook(() =>
      useCountdown(timer({ status: "running", endsAt: 60_000, remainingSeconds: 60 }), now),
    );
    act(() => {
      for (let press = 0; press < 10; press += 1) window.dispatchEvent(new Event("focus"));
    });
    expect(vi.getTimerCount()).toBe(1);
    unmount();
    now.mockClear();
    act(() => {
      window.dispatchEvent(new Event("focus"));
      document.dispatchEvent(new Event("visibilitychange"));
      vi.advanceTimersByTime(5_000);
    });
    expect(vi.getTimerCount()).toBe(0);
    expect(now).not.toHaveBeenCalled();
  });

  it("shows zero on returning after expiry and leaves phase changes to the backend", () => {
    vi.setSystemTime(0);
    const { result } = renderHook(() =>
      useCountdown(timer({ status: "running", endsAt: 60_000, remainingSeconds: 60 })),
    );
    vi.setSystemTime(90_000);
    act(() => window.dispatchEvent(new Event("focus")));
    expect(result.current).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("does not restart a paused countdown when the window regains focus", () => {
    vi.setSystemTime(0);
    const { result, rerender } = renderHook(({ state }) => useCountdown(state), {
      initialProps: { state: timer({ status: "running", endsAt: 60_000, remainingSeconds: 60 }) },
    });
    rerender({ state: timer({ status: "paused", endsAt: null, remainingSeconds: 45 }) });
    vi.setSystemTime(90_000);
    act(() => window.dispatchEvent(new Event("focus")));
    expect(result.current).toBe(45);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("never draws a new interval with the previous one's remaining time", () => {
    // The value used to be copied into state by an effect, one render late: a
    // phase change was drawn once as the new phase at the old 00:00.
    vi.setSystemTime(50_000);
    const drawn: number[] = [];
    const { rerender } = renderHook(
      ({ current }: { current: TimerState }) => {
        const remaining = useCountdown(current);
        drawn.push(remaining);
        return remaining;
      },
      {
        initialProps: {
          current: timer({ status: "running", endsAt: 50_000, remainingSeconds: 0 }),
        },
      },
    );
    expect(drawn.at(-1)).toBe(0);

    drawn.length = 0;
    rerender({
      current: timer({ status: "idle", endsAt: null, remainingSeconds: 300, durationSeconds: 300 }),
    });
    expect(drawn).not.toContain(0);
    expect(drawn.at(-1)).toBe(300);

    drawn.length = 0;
    rerender({
      current: timer({ status: "running", endsAt: 50_000 + 300_000, remainingSeconds: 300 }),
    });
    expect(new Set(drawn)).toEqual(new Set([300]));
  });
});
