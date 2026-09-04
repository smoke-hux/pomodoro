// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { activatesOnSpace } from "./App";

/**
 * Space starts and pauses the timer from anywhere in the window. It is also how
 * a keyboard user presses whatever they have just tabbed to, and the global
 * handler used to win both times: the focused button was left unpressed and the
 * timer toggled behind it.
 */
function element(html: string): HTMLElement {
  const host = document.createElement("div");
  host.innerHTML = html;
  return host.firstElementChild as HTMLElement;
}

describe("Space belongs to the focused control", () => {
  it.each([
    ["a button", "<button>Skip</button>"],
    ["a checkbox", '<input type="checkbox" />'],
    ["a text field", '<input type="text" />'],
    ["a select", "<select><option>a</option></select>"],
    ["a textarea", "<textarea></textarea>"],
    ["a link", '<a href="#x">Docs</a>'],
    ["a details summary", "<summary>Actions</summary>"],
    ["a custom radio", '<div role="radio" tabindex="0">Ring</div>'],
    ["a custom switch", '<div role="switch" tabindex="0">On</div>'],
    ["a custom button", '<div role="button" tabindex="0">Go</div>'],
  ])("yields to %s", (_label, html) => {
    expect(activatesOnSpace(element(html))).toBe(true);
  });
});

describe("Space reaches the timer everywhere else", () => {
  it.each([
    ["the page body", "<main>Focus</main>"],
    ["a heading", "<h1>25:00</h1>"],
    ["a plain list item", '<div role="listitem">Notification</div>'],
    ["a non-interactive container", "<section><p>Today</p></section>"],
  ])("passes through %s", (_label, html) => {
    expect(activatesOnSpace(element(html))).toBe(false);
  });

  it("passes through a keydown with no target at all", () => {
    expect(activatesOnSpace(null)).toBe(false);
  });
});
