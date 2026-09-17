// @vitest-environment jsdom
import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { placeMenu, useRowMenus } from "./useRowMenus";

afterEach(() => {
  cleanup();
  document.body.innerHTML = "";
});

function menu(): HTMLDetailsElement {
  const details = document.createElement("details");
  details.className = "row-menu";
  details.innerHTML = '<summary>More</summary><div class="menu-popover"><button>Edit</button></div>';
  document.body.append(details);
  return details;
}

/** What the browser does when a <details> is opened or closed by any means. */
function toggle(details: HTMLDetailsElement, open: boolean) {
  details.open = open;
  details.dispatchEvent(new Event("toggle"));
}

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

describe("useRowMenus", () => {
  it("closes the open menu when another is opened from the keyboard", () => {
    // No pointer press happens on this path, so pointerdown alone left both open.
    renderHook(() => useRowMenus());
    const first = menu();
    const second = menu();

    toggle(first, true);
    toggle(second, true);
    expect(first.open).toBe(false);
    expect(second.open).toBe(true);
  });

  it("closes on a press outside, and not on a press inside", () => {
    renderHook(() => useRowMenus());
    const details = menu();
    toggle(details, true);

    details.querySelector("button")!.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(details.open).toBe(true);
    document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(details.open).toBe(false);
  });

  it("places a menu on every open, never reusing where it was last time", () => {
    renderHook(() => useRowMenus());
    const details = menu();

    toggle(details, true);
    expect(details.dataset.placed).toBe("");
    expect(details.style.getPropertyValue("--menu-top")).not.toBe("");
    toggle(details, false);
    expect(details.dataset.placed).toBeUndefined();
  });

  it("closes when the list underneath scrolls", () => {
    renderHook(() => useRowMenus());
    const details = menu();
    toggle(details, true);
    document.body.dispatchEvent(new Event("scroll"));
    expect(details.open).toBe(false);
  });
});
