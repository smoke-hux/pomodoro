// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { TimerFace } from "./TimerFace";
import type { TimerFace as TimerFaceId } from "../types";

afterEach(cleanup);

// 15:00 left of a 25:00 interval — 40% elapsed, so every proportional face
// should be in a partial state rather than empty or full.
const DURATION = 25 * 60;
const REMAINING = 15 * 60;

function renderFace(face: TimerFaceId, remaining = REMAINING, duration = DURATION) {
  return render(
    <TimerFace
      face={face}
      phase="focus"
      remainingSeconds={remaining}
      durationSeconds={duration}
      phaseLabel="Focus"
    />,
  );
}

const ALL_FACES: TimerFaceId[] = [
  "digits", "ring", "bar", "pips", "words",
  "analog", "vessel", "arc", "blocks", "orbit",
];

describe("accessibility parity", () => {
  it.each(ALL_FACES)(
    "%s exposes the same value to assistive tech",
    (face) => {
      renderFace(face);
      // Choosing a face must never cost a screen-reader user information.
      expect(screen.getByLabelText("15 minutes remaining, Focus")).toBeDefined();
    },
  );

  it("singularises the label at one minute", () => {
    renderFace("ring", 60);
    expect(screen.getByLabelText("1 minute remaining, Focus")).toBeDefined();
  });
});

describe("digits face", () => {
  it("shows the exact clock", () => {
    const { container } = renderFace("digits");
    expect(container.querySelector(".face-digits")?.textContent).toBe("15:00");
  });
});

describe("ring face", () => {
  it("offsets the arc by the elapsed fraction", () => {
    const { container } = renderFace("ring");
    const progress = container.querySelector(".ring-progress");
    const circumference = 2 * Math.PI * 44;
    expect(Number(progress?.getAttribute("stroke-dasharray"))).toBeCloseTo(circumference, 3);
    // 40% elapsed.
    expect(Number(progress?.getAttribute("stroke-dashoffset"))).toBeCloseTo(
      circumference * 0.4,
      3,
    );
  });

  it("keeps the arc within bounds when the timer has run out", () => {
    const { container } = renderFace("ring", 0);
    const offset = Number(container.querySelector(".ring-progress")?.getAttribute("stroke-dashoffset"));
    expect(offset).toBeCloseTo(2 * Math.PI * 44, 3);
  });
});

describe("bar face", () => {
  it("scales the fill to the time remaining, not elapsed", () => {
    const { container } = renderFace("bar");
    const fill = container.querySelector<HTMLElement>(".bar-fill");
    expect(fill?.style.transform).toBe("scaleX(0.6)");
  });

  it("does not go negative past expiry", () => {
    const { container } = renderFace("bar", 0);
    expect(container.querySelector<HTMLElement>(".bar-fill")?.style.transform).toBe("scaleX(0)");
  });
});

describe("pips face", () => {
  it("draws one mark per minute and lights the remainder", () => {
    const { container } = renderFace("pips");
    expect(container.querySelectorAll(".pip")).toHaveLength(25);
    expect(container.querySelectorAll(".pip.lit")).toHaveLength(15);
  });

  it("collapses to five-minute marks past an hour so the grid stays countable", () => {
    const { container } = renderFace("pips", 90 * 60, 90 * 60);
    expect(container.querySelectorAll(".pip")).toHaveLength(18);
    // The scale change is stated rather than left to guesswork.
    expect(container.querySelector(".pip-readout")?.textContent).toContain(
      "one mark is five minutes",
    );
  });
});

describe("words face", () => {
  it("renders no numerals at all", () => {
    const { container } = renderFace("words");
    const text = container.querySelector(".words-line")?.textContent ?? "";
    expect(text).toBe("about fifteen minutes left");
    expect(text).not.toMatch(/\d/);
  });
});

describe("analog face", () => {
  it("winds a wedge covering the time still to run", () => {
    const { container } = renderFace("analog");
    // 60% remaining -> 216 degrees, which is past the half turn.
    const d = container.querySelector(".dial-wedge")?.getAttribute("d") ?? "";
    expect(d).toContain("A 34 34 0 1 1");
  });

  it("draws no wedge at all once the interval is spent", () => {
    const { container } = renderFace("analog", 0);
    expect(container.querySelector(".dial-wedge")).toBeNull();
  });

  it("does not collapse the wedge to nothing at a full interval", () => {
    // A literal 360 degree sweep puts both arc endpoints on one coordinate and
    // renders as empty — the wedge must still be drawn when nothing has run.
    const { container } = renderFace("analog", DURATION);
    const d = container.querySelector(".dial-wedge")?.getAttribute("d") ?? "";
    expect(d).not.toBe("");
    expect(d).toContain("A 34 34 0 1 1");
  });

  it("marks the quarters more heavily than the rest", () => {
    const { container } = renderFace("analog");
    expect(container.querySelectorAll(".dial-tick")).toHaveLength(12);
    expect(container.querySelectorAll(".dial-tick.major")).toHaveLength(4);
  });
});

describe("vessel face", () => {
  it("drains to the remaining fraction", () => {
    const { container } = renderFace("vessel");
    expect(container.querySelector<HTMLElement>(".vessel-fill")?.style.transform).toBe(
      "scaleY(0.6)",
    );
  });

  it("empties completely at expiry", () => {
    const { container } = renderFace("vessel", 0);
    expect(container.querySelector<HTMLElement>(".vessel-fill")?.style.transform).toBe(
      "scaleY(0)",
    );
  });
});

describe("arc face", () => {
  it("offsets a semicircle, not a full circle", () => {
    const { container } = renderFace("arc");
    const progress = container.querySelector(".arc-progress");
    const length = Math.PI * 40;
    expect(Number(progress?.getAttribute("stroke-dasharray"))).toBeCloseTo(length, 3);
    expect(Number(progress?.getAttribute("stroke-dashoffset"))).toBeCloseTo(length * 0.4, 3);
  });
});

describe("blocks face", () => {
  it("always draws twelve segments regardless of interval length", () => {
    const { container } = renderFace("blocks");
    expect(container.querySelectorAll(".block")).toHaveLength(12);
    expect(container.querySelectorAll(".block.lit")).toHaveLength(8);
  });

  it("keeps the same glance-weight for a short break", () => {
    const { container } = renderFace("blocks", 150, 300);
    expect(container.querySelectorAll(".block")).toHaveLength(12);
    expect(container.querySelectorAll(".block.lit")).toHaveLength(6);
  });
});

describe("orbit face", () => {
  it("starts the dot at twelve o'clock", () => {
    const { container } = renderFace("orbit", DURATION);
    const dot = container.querySelector(".orbit-dot");
    expect(Number(dot?.getAttribute("cx"))).toBeCloseTo(50, 3);
    expect(Number(dot?.getAttribute("cy"))).toBeCloseTo(10, 3);
  });

  it("puts the dot at the bottom of the circle at the halfway mark", () => {
    const { container } = renderFace("orbit", DURATION / 2);
    const dot = container.querySelector(".orbit-dot");
    expect(Number(dot?.getAttribute("cx"))).toBeCloseTo(50, 3);
    expect(Number(dot?.getAttribute("cy"))).toBeCloseTo(90, 3);
  });
});
