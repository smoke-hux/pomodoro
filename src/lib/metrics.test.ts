import { describe, expect, it } from "vitest";
import type { FocusTask, Interruption, SessionRecord } from "../types";
import {
  formatClock,
  formatCountdown,
  formatDuration,
  getCompletedFocusByTask,
  getCompletedFocusCount,
  getCompletedFocusMinutes,
  getInterruptionCounts,
  getLocalDayBounds,
  getSevenDayCompletedFocusSeries,
  getTaskActuals,
  getTaskPlannedTotal,
  getTodayFocusSessions,
  getTodayInterruptionCounts,
} from "./metrics";

const localTime = (
  year: number,
  month: number,
  day: number,
  hour = 0,
  minute = 0,
  second = 0,
) => new Date(year, month, day, hour, minute, second).getTime();

let sessionNumber = 0;
const makeSession = (
  startedAt: number,
  overrides: Partial<SessionRecord> = {},
): SessionRecord => ({
  id: `session-${(sessionNumber += 1)}`,
  phase: "focus",
  taskId: "task-a",
  taskTitle: "Draft proposal",
  durationSeconds: 1_500,
  startedAt,
  endedAt: startedAt + 1_500_000,
  outcome: "completed",
  ...overrides,
});

const makeTask = (id: string, estimate: number, done = false): FocusTask => ({
  id,
  title: `Task ${id}`,
  estimate,
  completedPomodoros: 99,
  done,
  createdAt: localTime(2026, 7, 1),
  completedAt: done ? localTime(2026, 7, 2) : null,
});

describe("local-day metrics", () => {
  it("uses inclusive local midnight and an exclusive next-day boundary", () => {
    const now = localTime(2026, 8, 2, 12);
    const bounds = getLocalDayBounds(now);

    expect(bounds).toEqual({
      start: localTime(2026, 8, 2),
      end: localTime(2026, 8, 3),
    });

    const sessions = [
      makeSession(localTime(2026, 8, 1, 23, 59, 59)),
      makeSession(bounds.start, { outcome: "skipped" }),
      makeSession(localTime(2026, 8, 2, 23, 59, 59), { outcome: "abandoned" }),
      makeSession(bounds.end),
      makeSession(localTime(2026, 8, 2, 9), { phase: "shortBreak" }),
    ];

    expect(getTodayFocusSessions(sessions, now).map((session) => session.startedAt)).toEqual([
      bounds.start,
      localTime(2026, 8, 2, 23, 59, 59),
    ]);
  });

  it("counts only interruptions captured within today's local bounds", () => {
    const now = localTime(2026, 8, 2, 12);
    const interruptions: Interruption[] = [
      {
        id: "before",
        text: "Before",
        category: "internal",
        capturedAt: localTime(2026, 8, 1, 23, 59, 59),
        handled: false,
        taskId: null,
      },
      {
        id: "start",
        text: "Idea",
        category: "internal",
        capturedAt: localTime(2026, 8, 2),
        handled: true,
        taskId: "task-a",
      },
      {
        id: "late",
        text: "Message",
        category: "external",
        capturedAt: localTime(2026, 8, 2, 23, 59, 59),
        handled: false,
        taskId: "task-a",
      },
      {
        id: "after",
        text: "After",
        category: "external",
        capturedAt: localTime(2026, 8, 3),
        handled: true,
        taskId: null,
      },
    ];

    expect(getTodayInterruptionCounts(interruptions, now)).toEqual({
      total: 2,
      internal: 1,
      external: 1,
      handled: 1,
      unhandled: 1,
    });
  });
});

describe("focus totals", () => {
  it("excludes completed breaks and skipped or abandoned focus sessions", () => {
    const start = localTime(2026, 8, 2, 9);
    const sessions = [
      makeSession(start, { durationSeconds: 1_500 }),
      makeSession(start + 2_000_000, { durationSeconds: 3_000 }),
      makeSession(start + 4_000_000, { outcome: "skipped" }),
      makeSession(start + 6_000_000, { outcome: "abandoned" }),
      makeSession(start + 8_000_000, { phase: "longBreak", durationSeconds: 900 }),
    ];

    expect(getCompletedFocusCount(sessions)).toBe(2);
    expect(getCompletedFocusMinutes(sessions)).toBe(75);
  });

  it("counts interruption categories and handling state", () => {
    const interruptions: Interruption[] = [
      {
        id: "one",
        text: "Email",
        category: "external",
        capturedAt: 1,
        handled: true,
        taskId: null,
      },
      {
        id: "two",
        text: "New idea",
        category: "internal",
        capturedAt: 2,
        handled: false,
        taskId: "task-a",
      },
      {
        id: "three",
        text: "Chat",
        category: "external",
        capturedAt: 3,
        handled: false,
        taskId: "task-a",
      },
    ];

    expect(getInterruptionCounts(interruptions)).toEqual({
      total: 3,
      internal: 1,
      external: 2,
      handled: 1,
      unhandled: 2,
    });
  });
});

describe("task metrics", () => {
  it("keeps the original planned total and derives actuals from completed sessions", () => {
    const tasks = [makeTask("task-a", 2), makeTask("task-b", 1, true), makeTask("task-c", 5)];
    const start = localTime(2026, 8, 2, 9);
    const sessions = [
      makeSession(start, { taskId: "task-a" }),
      makeSession(start + 2_000_000, { taskId: "task-a" }),
      makeSession(start + 4_000_000, { taskId: "task-a", outcome: "skipped" }),
      makeSession(start + 6_000_000, { taskId: "task-b" }),
      makeSession(start + 8_000_000, { taskId: "archived-task" }),
      makeSession(start + 10_000_000, { taskId: null }),
    ];

    expect(getTaskPlannedTotal(tasks)).toBe(8);
    expect(getCompletedFocusByTask(sessions)).toEqual({
      "task-a": 2,
      "task-b": 1,
      "archived-task": 1,
    });
    expect(getTaskActuals(tasks, sessions).map(({ taskId, actual }) => ({ taskId, actual }))).toEqual([
      { taskId: "task-a", actual: 2 },
      { taskId: "task-b", actual: 1 },
      { taskId: "task-c", actual: 0 },
    ]);
  });
});

describe("seven-day series", () => {
  it("returns the six previous local days plus today in order, including zero days", () => {
    const now = localTime(2026, 8, 2, 12);
    const sessions = [
      makeSession(localTime(2026, 7, 27, 9)),
      makeSession(localTime(2026, 7, 29, 9)),
      makeSession(localTime(2026, 7, 29, 10)),
      makeSession(localTime(2026, 8, 2, 8)),
      makeSession(localTime(2026, 8, 2, 9), { outcome: "abandoned" }),
      makeSession(localTime(2026, 8, 2, 10), { phase: "shortBreak" }),
      makeSession(localTime(2026, 7, 26, 23, 59, 59)),
      makeSession(localTime(2026, 8, 3)),
    ];

    expect(
      getSevenDayCompletedFocusSeries(sessions, now).map(({ date, count }) => ({ date, count })),
    ).toEqual([
      { date: "2026-08-27", count: 1 },
      { date: "2026-08-28", count: 0 },
      { date: "2026-08-29", count: 2 },
      { date: "2026-08-30", count: 0 },
      { date: "2026-08-31", count: 0 },
      { date: "2026-09-01", count: 0 },
      { date: "2026-09-02", count: 1 },
    ]);
  });
});

describe("formatting", () => {
  it("formats human durations and clamps invalid values", () => {
    expect(formatDuration(0)).toBe("0s");
    expect(formatDuration(65)).toBe("1m 5s");
    expect(formatDuration(3_900)).toBe("1h 5m");
    expect(formatDuration(Number.NaN)).toBe("0s");
  });

  it("formats countdowns without displaying negative time", () => {
    expect(formatCountdown(1_500)).toBe("25:00");
    expect(formatCountdown(3_661)).toBe("1:01:01");
    expect(formatCountdown(0.2)).toBe("00:01");
    expect(formatCountdown(-10)).toBe("00:00");
  });

  it("formats a local clock value as zero-padded 24-hour time", () => {
    expect(formatClock(localTime(2026, 8, 2, 5, 7, 59))).toBe("05:07");
  });
});
