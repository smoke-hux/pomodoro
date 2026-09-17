// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDayKey } from "./useDayKey";

afterEach(cleanup);
beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("useDayKey", () => {
  it("rolls over at local midnight in a window left open overnight", () => {
    vi.setSystemTime(new Date(2026, 8, 17, 23, 59, 30));
    const { result } = renderHook(() => useDayKey());
    expect(result.current).toBe("2026-09-17");

    act(() => void vi.advanceTimersByTime(20_000));
    expect(result.current).toBe("2026-09-17");
    act(() => void vi.advanceTimersByTime(11_000));
    expect(result.current).toBe("2026-09-18");

    // And again the night after: the timeout re-arms itself.
    act(() => void vi.advanceTimersByTime(24 * 60 * 60 * 1_000));
    expect(result.current).toBe("2026-09-19");
  });

  it("catches up when the window regains focus after a suspend skipped midnight", () => {
    vi.setSystemTime(new Date(2026, 8, 17, 22, 0, 0));
    const { result } = renderHook(() => useDayKey());

    // The clock jumps without the timers having run, as it does across a suspend.
    vi.setSystemTime(new Date(2026, 8, 18, 7, 30, 0));
    expect(result.current).toBe("2026-09-17");
    act(() => void window.dispatchEvent(new Event("focus")));
    expect(result.current).toBe("2026-09-18");
  });
});
