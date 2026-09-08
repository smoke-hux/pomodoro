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
});
