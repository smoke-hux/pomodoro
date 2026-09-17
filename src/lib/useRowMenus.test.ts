import { describe, expect, it } from "vitest";
import { placeMenu } from "./useRowMenus";

const viewport = { width: 1080, height: 720 };

describe("placing a row menu", () => {
  it("opens below the only task in the list, where it used to be clipped away", () => {
    // One task: its row is both the first and the last in the list.
    const placement = placeMenu({ top: 100, bottom: 140, right: 250 }, 82, viewport);
    expect(placement.top).toBe(142);
    expect(placement.bottom).toBeNull();
  });

  it("opens above a row near the bottom of the window", () => {
    const placement = placeMenu({ top: 650, bottom: 690, right: 250 }, 120, viewport);
    expect(placement.top).toBeNull();
    expect(placement.bottom).toBe(72);
  });

  it("leaves room for the menu growing into a delete confirmation", () => {
    // 82px would fit below, but the confirmation that can replace it would not.
    const placement = placeMenu({ top: 540, bottom: 580, right: 250 }, 82, viewport);
    expect(placement.top).toBeNull();
  });

  it("stays right-aligned to its button and inside the window", () => {
    expect(placeMenu({ top: 100, bottom: 140, right: 250 }, 82, viewport).right).toBe(830);
    expect(placeMenu({ top: 100, bottom: 140, right: 1079 }, 82, viewport).right).toBe(8);
  });

  it("prefers below when the window is too short for either direction", () => {
    const placement = placeMenu({ top: 60, bottom: 100, right: 250 }, 82, { width: 760, height: 200 });
    expect(placement.top).toBe(102);
  });
});
