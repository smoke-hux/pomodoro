// @vitest-environment jsdom
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
      skipRequest={0}
    />,
  );
  const element = container.querySelector(".cycle-count")!;
  return {
    announced: element.getAttribute("aria-label"),
    shown: element.querySelector("span")!.textContent,
  };
}

describe("the round count", () => {
  it.each([
    ["the first focus", { phase: "focus", completedInCycle: 0 }, "Round 1 of 4"],
    [
      "a short break after one round",
      { phase: "shortBreak", completedInCycle: 1 },
      "1 of 4 rounds",
    ],
    // The backend only resets the count after the long break, so this used to
    // be announced as "Round 5 of 4" over a screen reading "Cycle complete".
    ["the long break", { phase: "longBreak", completedInCycle: 4 }, "Cycle complete"],
  ] as const)("says what it shows during %s", (_name, timer, text) => {
    const { announced, shown } = cycleCount(timer);
    expect(shown).toBe(text);
    expect(announced).toBe(text);
  });
});

describe("skip requests", () => {
  it("ignores requests from before mount and handles the next press once", () => {
    const props = {
      timer: defaultSnapshot.timer,
      activeTask: null,
      roundsBeforeLongBreak: 4,
      face: defaultSnapshot.settings.timerFace,
      onSetPhase: vi.fn(),
      onToggleTimer: vi.fn(),
      onReset: vi.fn(),
      onSkip: vi.fn(),
      onAddTask: vi.fn(),
      skipRequest: 2,
    };
    const { rerender } = render(<TimerPanel {...props} />);
    expect(props.onSkip).not.toHaveBeenCalled();

    rerender(<TimerPanel {...props} skipRequest={3} />);
    expect(props.onSkip).toHaveBeenCalledTimes(1);

    rerender(<TimerPanel {...props} skipRequest={3} face="ring" />);
    expect(props.onSkip).toHaveBeenCalledTimes(1);
  });
});

describe("the countdown it owns", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    document.title = "";
  });

  it("counts the window title and the progress bar down by itself while running", () => {
    vi.setSystemTime(60_000);
    const { container } = render(
      <TimerPanel
        timer={{
          ...defaultSnapshot.timer,
          status: "running",
          endsAt: 150_000,
          remainingSeconds: 90,
        }}
        activeTask={null}
        roundsBeforeLongBreak={4}
        face={defaultSnapshot.settings.timerFace}
        onSetPhase={vi.fn()}
        onToggleTimer={vi.fn()}
        onReset={vi.fn()}
        onSkip={vi.fn()}
        onAddTask={vi.fn()}
        skipRequest={0}
      />,
    );

    expect(document.title).toBe("2m · Focus · running — Pomodoro");
    const progress = () =>
      container.querySelector('[role="progressbar"]')!.getAttribute("aria-valuenow");
    expect(progress()).toBe("1410");

    act(() => void vi.advanceTimersByTime(60_000));

    expect(document.title).toBe("1m · Focus · running — Pomodoro");
    expect(progress()).toBe("1470");
  });
});
