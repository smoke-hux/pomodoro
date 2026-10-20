import { useEffect } from "react";
import type { RefObject } from "react";

const FOCUSABLE =
  'a[href], button, input:not([type="hidden"]), select, textarea, summary, [tabindex]';

function available(element: HTMLElement): boolean {
  // :disabled includes controls disabled by a parent fieldset, whereas the
  // [disabled] attribute only describes the control itself.
  if (element.matches(':disabled, input[type="hidden"]')) return false;
  if (element.closest("[hidden], [inert], .visually-hidden")) return false;
  const visibility = window.getComputedStyle(element).visibility;
  if (visibility === "hidden" || visibility === "collapse") return false;
  // A child's own display value does not tell us that its parent is hidden.
  for (let ancestor: HTMLElement | null = element; ancestor; ancestor = ancestor.parentElement) {
    if (window.getComputedStyle(ancestor).display === "none") return false;
  }
  return true;
}

function focusableIn(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (element) => element.tabIndex >= 0 && available(element),
  );
}

/**
 * Keeps the keyboard inside a modal dialog while it is open.
 *
 * `aria-modal` is a promise to assistive technology, not behaviour: nothing in
 * the browser acts on it. Focus stayed wherever it was when a shortcut opened
 * the dialog — on "Skip", say — and Space then pressed that button underneath
 * the dialog, because the window-level key handler steps aside for an open
 * modal without stopping the key. Tab walked out of the dialog into the page
 * behind it.
 *
 * On open, focus moves into the dialog (to `initial` if given and present,
 * otherwise the first control) unless it is already inside. Tab and Shift+Tab
 * wrap at the ends. Handing focus back on close is the caller's job; App
 * already does that for both dialogs.
 */
export function useModalFocus(
  open: boolean,
  dialog: RefObject<HTMLElement | null>,
  initial?: RefObject<HTMLElement | null>,
) {
  useEffect(() => {
    if (!open) return;
    const container = dialog.current;
    if (!container) return;
    // Give an empty or temporarily disabled dialog a fallback focus target.
    // A negative index permits programmatic focus without adding a Tab stop.
    const addedTabIndex = !container.hasAttribute("tabindex");
    if (addedTabIndex) container.tabIndex = -1;

    const frame = requestAnimationFrame(() => {
      if (container.contains(document.activeElement)) return;
      const preferred = initial?.current;
      const target =
        preferred &&
        container.contains(preferred) &&
        preferred.matches(FOCUSABLE) &&
        available(preferred)
          ? preferred
          : (focusableIn(container)[0] ?? container);
      target.focus();
    });

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const items = focusableIn(container);
      if (items.length === 0) {
        event.preventDefault();
        container.focus();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement;
      if (!container.contains(active) || active === container) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      } else if (event.shiftKey && active === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("keydown", onKeyDown);
      if (addedTabIndex && container.getAttribute("tabindex") === "-1") {
        container.removeAttribute("tabindex");
      }
    };
  }, [open, dialog, initial]);
}
