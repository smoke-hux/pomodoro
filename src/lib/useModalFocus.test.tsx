// @vitest-environment jsdom
import { act, cleanup, fireEvent, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useModalFocus } from "./useModalFocus";

let fixture: HTMLDivElement;

beforeEach(() => {
  vi.useFakeTimers();
  fixture = document.createElement("div");
  document.body.append(fixture);
});

afterEach(() => {
  cleanup();
  fixture.remove();
  vi.useRealTimers();
});

function openDialog(contents: string, initialId?: string) {
  fixture.innerHTML = `<button id="outside">Outside</button><section role="dialog" aria-label="Test dialog">${contents}</section>`;
  const container = fixture.querySelector<HTMLElement>("section")!;
  const initial = initialId ? fixture.querySelector<HTMLElement>(`#${initialId}`) : null;
  const dialogRef = { current: container };
  const initialRef = { current: initial };
  fixture.querySelector<HTMLButtonElement>("#outside")!.focus();
  const hook = renderHook(() => useModalFocus(true, dialogRef, initialRef));
  act(() => void vi.advanceTimersByTime(20));
  return { ...hook, container };
}

describe("modal keyboard focus", () => {
  it.each([
    ["a disabled fieldset", "<fieldset disabled><button>Unavailable</button></fieldset>"],
    ["a negative tab index", '<button tabindex="-1">Not in the tab order</button>'],
    ["a hidden input", '<input type="hidden" value="data">'],
    ["an inert section", "<div inert><button>Unavailable</button></div>"],
    ["a hidden ancestor", '<div style="display:none"><button>Unavailable</button></div>'],
    ["hidden visibility", '<button style="visibility:hidden">Unavailable</button>'],
  ])("skips %s on opening and when wrapping Tab", (_label, unavailable) => {
    openDialog(
      `${unavailable}<button id="first">First</button><button id="last">Last</button>${unavailable}`,
    );
    const first = fixture.querySelector<HTMLButtonElement>("#first")!;
    const last = fixture.querySelector<HTMLButtonElement>("#last")!;
    expect(document.activeElement).toBe(first);

    last.focus();
    fireEvent.keyDown(last, { key: "Tab" });
    expect(document.activeElement).toBe(first);
    fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(last);
  });

  it("falls back to an available control when the requested initial input is disabled", () => {
    openDialog('<button id="first">Close</button><input id="initial" disabled>', "initial");
    expect(document.activeElement).toBe(fixture.querySelector("#first"));
  });

  it("uses the requested initial input when it is available", () => {
    openDialog('<button>Close</button><input id="initial">', "initial");
    expect(document.activeElement).toBe(fixture.querySelector("#initial"));
  });

  it("recovers escaped focus in the direction of the Tab press", () => {
    openDialog('<button id="first">First</button><button id="last">Last</button>');
    fixture.querySelector<HTMLButtonElement>("#outside")!.focus();
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(fixture.querySelector("#last"));
  });

  it("focuses the dialog itself while it has no enabled controls", () => {
    const { container } = openDialog("<fieldset disabled><button>Working</button></fieldset>");
    expect(document.activeElement).toBe(container);
    expect(container.tabIndex).toBe(-1);
    fireEvent.keyDown(container, { key: "Tab" });
    expect(document.activeElement).toBe(container);
  });

  it("re-evaluates available controls when a fieldset becomes disabled", () => {
    openDialog(
      '<button id="first">Close</button><fieldset><button id="last">Import</button></fieldset>',
    );
    fixture.querySelector("fieldset")!.disabled = true;
    const first = fixture.querySelector<HTMLButtonElement>("#first")!;
    fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(first);
  });

  it("leaves outside controls alone after the dialog unmounts", () => {
    const { unmount } = openDialog("<button>Close</button>");
    unmount();
    const outside = fixture.querySelector<HTMLButtonElement>("#outside")!;
    outside.focus();
    const event = new KeyboardEvent("keydown", { key: "Tab", cancelable: true, bubbles: true });
    outside.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(outside);
  });
});
