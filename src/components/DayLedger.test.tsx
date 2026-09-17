// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { DayLedger } from "./DayLedger";
import { getLocalDateKey } from "../lib/metrics";
import type { SessionRecord } from "../types";

afterEach(cleanup);

const NOW = new Date(2026, 8, 17, 15, 0, 0).getTime();

function focus(id: string, startedAt: number, durationSeconds: number): SessionRecord {
  return {
    id,
    phase: "focus",
    taskId: null,
    taskTitle: `Session ${id}`,
    durationSeconds,
    startedAt,
    endedAt: startedAt + durationSeconds * 1_000,
    outcome: "completed",
  };
}

function renderLedger(sessions: SessionRecord[]) {
  render(
    <DayLedger dayKey={getLocalDateKey(NOW)} sessions={sessions} tasks={[]} interruptions={[]} />,
  );
}

describe("the day ledger", () => {
  it("shows a total that is the sum of the rows under it", () => {
    // 25 min 29 s each: two rows of 25m. Rounding the summed seconds said 51.
    renderLedger([focus("a", NOW - 7_200_000, 1_529), focus("b", NOW - 3_600_000, 1_529)]);
    expect(screen.getAllByText("25m")).toHaveLength(2);
    expect(screen.getByText(/2 focus · 50m/)).toBeTruthy();
  });

  it("counts a very short session as the one minute its row shows", () => {
    renderLedger([focus("a", NOW - 60_000, 20)]);
    expect(screen.getByText("1m")).toBeTruthy();
    expect(screen.getByText(/1 focus · 1m/)).toBeTruthy();
  });

  it("leaves out a session whose start time no calendar can hold, rather than failing to render", () => {
    renderLedger([focus("bad", 9e18, 1_500), focus("good", NOW - 3_600_000, 1_500)]);
    expect(screen.getByText(/1 focus · 25m/)).toBeTruthy();
  });

  it("lists every session the total counts, not only the newest twelve", () => {
    // Six pomodoros with their breaks is already twelve records.
    const sessions = Array.from({ length: 14 }, (_, index) =>
      focus(`s${index}`, NOW - (index + 1) * 1_800_000, 1_500),
    );
    renderLedger(sessions);

    expect(screen.getAllByRole("listitem")).toHaveLength(14);
    expect(screen.getByText(/14 focus · 350m/)).toBeTruthy();
  });
});
