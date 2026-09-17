// @vitest-environment jsdom
import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TimerPanel } from "./TimerPanel";
import { defaultSnapshot } from "../types";
import type { TimerState } from "../types";

afterEach(cleanup);

function cycleCount(timer: Partial<TimerState>) {
  const { container } = render(
    <TimerPanel
      timer={{ ...defaultSnapshot.timer, ...timer }}
      activeTask={null}
      roundsBeforeLongBreak={4}
      face={defaultSnapshot.settings.timerFace}
      onSetPhase={vi.fn()}
      onToggleTimer={vi.fn()}
      onReset={vi.fn()}
      onSkip={vi.fn()}
      onAddTask={vi.fn()}
    />,
  );
  const element = container.querySelector(".cycle-count")!;
  return { announced: element.getAttribute("aria-label"), shown: element.querySelector("span")!.textContent };
}

describe("the round count", () => {
  it.each([
    ["the first focus", { phase: "focus", completedInCycle: 0 }, "Round 1 of 4"],
    ["a short break after one round", { phase: "shortBreak", completedInCycle: 1 }, "1 of 4 rounds"],
    // The backend only resets the count after the long break, so this used to
    // be announced as "Round 5 of 4" over a screen reading "Cycle complete".
    ["the long break", { phase: "longBreak", completedInCycle: 4 }, "Cycle complete"],
  ] as const)("says what it shows during %s", (_name, timer, text) => {
    const { announced, shown } = cycleCount(timer);
    expect(shown).toBe(text);
    expect(announced).toBe(text);
  });
});
