// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppSnapshot } from "../types";
import { defaultSnapshot } from "../types";

const { listen, snapshot } = vi.hoisted(() => ({ listen: vi.fn(), snapshot: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("./api", () => ({ api: { snapshot } }));

import { useAppSnapshot } from "./useAppSnapshot";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.useFakeTimers();
  listen.mockReset();
  snapshot.mockReset();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("the local data connection", () => {
  it("retries registration every three seconds until it can recover", async () => {
    const stop = vi.fn();
    listen
      .mockRejectedValueOnce(new Error("unavailable"))
      .mockRejectedValueOnce(new Error("still unavailable"))
      .mockResolvedValue(stop);
    snapshot.mockResolvedValue(defaultSnapshot);
    const notice = vi.fn();
    const { result, unmount } = renderHook(() => useAppSnapshot(true, vi.fn(), notice));

    await act(async () => {});
    expect(result.current.ready).toBe(true);
    expect(result.current.settingsLoaded).toBe(false);
    expect(snapshot).not.toHaveBeenCalled();
    expect(notice).toHaveBeenCalledWith(expect.stringContaining("Retrying"));

    await act(async () => vi.advanceTimersByTimeAsync(2_999));
    expect(listen).toHaveBeenCalledTimes(1);
    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(listen).toHaveBeenCalledTimes(2);
    expect(result.current.settingsLoaded).toBe(false);

    await act(async () => vi.advanceTimersByTimeAsync(3_000));
    expect(listen).toHaveBeenCalledTimes(3);
    expect(result.current.settingsLoaded).toBe(true);
    expect(result.current.snapshot).toEqual(defaultSnapshot);
    await act(async () => vi.advanceTimersByTimeAsync(9_000));
    expect(listen).toHaveBeenCalledTimes(3);
    expect(notice).toHaveBeenCalledTimes(2);

    unmount();
    expect(stop).toHaveBeenCalledTimes(1);
  });

  it("releases a failed read's subscription before registering again", async () => {
    const oldStop = vi.fn();
    const newStop = vi.fn();
    listen.mockResolvedValueOnce(oldStop).mockResolvedValue(newStop);
    snapshot.mockRejectedValueOnce(new Error("read failed")).mockResolvedValue(defaultSnapshot);
    const notice = vi.fn();
    const { result, unmount } = renderHook(() => useAppSnapshot(true, vi.fn(), notice));

    await act(async () => {});
    expect(oldStop).toHaveBeenCalledTimes(1);
    expect(result.current.settingsLoaded).toBe(false);
    await act(async () => vi.advanceTimersByTimeAsync(3_000));
    expect(result.current.settingsLoaded).toBe(true);
    expect(snapshot).toHaveBeenCalledTimes(2);
    expect(newStop).not.toHaveBeenCalled();

    unmount();
    expect(newStop).toHaveBeenCalledTimes(1);
  });

  it.each(["resolve", "reject"] as const)(
    "uses the first broadcast immediately even when the initial read later %ss",
    async (outcome) => {
      const stop = vi.fn();
      const read = deferred<AppSnapshot>();
      let broadcast!: (event: { payload: AppSnapshot }) => void;
      listen.mockImplementation((_event, handler) => {
        broadcast = handler;
        return Promise.resolve(stop);
      });
      snapshot.mockReturnValue(read.promise);
      const notice = vi.fn();
      const { result, unmount } = renderHook(() => useAppSnapshot(true, vi.fn(), notice));
      await act(async () => {});
      expect(result.current.ready).toBe(false);

      const newer: AppSnapshot = {
        ...defaultSnapshot,
        settings: { ...defaultSnapshot.settings, theme: "dark" },
      };
      act(() => broadcast({ payload: newer }));
      expect(result.current.ready).toBe(true);
      expect(result.current.settingsLoaded).toBe(true);
      expect(result.current.snapshot).toEqual(newer);

      await act(async () => {
        if (outcome === "resolve") read.resolve(defaultSnapshot);
        else read.reject(new Error("read failed after broadcast"));
      });
      expect(result.current.snapshot).toEqual(newer);
      expect(notice).not.toHaveBeenCalled();
      expect(stop).not.toHaveBeenCalled();
      await act(async () => vi.advanceTimersByTimeAsync(6_000));
      expect(listen).toHaveBeenCalledTimes(1);

      unmount();
      expect(stop).toHaveBeenCalledTimes(1);
    },
  );

  it("cancels a scheduled reconnect when the window unmounts", async () => {
    listen.mockRejectedValue(new Error("unavailable"));
    const notice = vi.fn();
    const { unmount } = renderHook(() => useAppSnapshot(true, vi.fn(), notice));
    await act(async () => {});
    unmount();

    await act(async () => vi.advanceTimersByTimeAsync(9_000));
    expect(listen).toHaveBeenCalledTimes(1);
    expect(notice).toHaveBeenCalledTimes(1);
  });

  it("releases a subscription that finishes registering after unmount", async () => {
    const registration = deferred<() => void>();
    listen.mockReturnValue(registration.promise);
    const notice = vi.fn();
    const { unmount } = renderHook(() => useAppSnapshot(true, vi.fn(), notice));
    unmount();
    const stop = vi.fn();

    await act(async () => registration.resolve(stop));
    expect(stop).toHaveBeenCalledTimes(1);
    expect(snapshot).not.toHaveBeenCalled();
    expect(notice).not.toHaveBeenCalled();
  });
});
