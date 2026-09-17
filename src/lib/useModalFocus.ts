import { useEffect } from "react";
import type { RefObject } from "react";

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), summary, [tabindex]:not([tabindex="-1"])';

function focusableIn(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (element) => !element.closest("[hidden]") && !element.closest(".visually-hidden"),
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

    const frame = requestAnimationFrame(() => {
      if (container.contains(document.activeElement)) return;
      (initial?.current ?? focusableIn(container)[0] ?? container).focus();
    });

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const items = focusableIn(container);
      if (items.length === 0) {
        event.preventDefault();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement;
      if (!container.contains(active)) {
        event.preventDefault();
        first.focus();
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
    };
  }, [open, dialog, initial]);
}
